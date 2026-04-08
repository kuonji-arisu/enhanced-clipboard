use std::sync::{Arc, RwLock};

use tauri::{AppHandle, Manager};

use crate::constants::MAIN_WINDOW_LABEL;
use crate::db::SettingsStore;
use crate::i18n::I18n;
use crate::models::{PersistedState, PersistedStatePatch};

struct PersistedStateChanges {
    always_on_top_changed: bool,
}

impl PersistedStateChanges {
    fn between(previous: &PersistedState, next: &PersistedState) -> Self {
        Self {
            always_on_top_changed: next.always_on_top != previous.always_on_top,
        }
    }
}

struct PersistedStateSavePlan {
    previous: PersistedState,
    next: PersistedState,
    changes: PersistedStateChanges,
}

impl PersistedStateSavePlan {
    // Persisted state uses the same patch/save-plan shape as settings, but only
    // changed fields with runtime effects (currently always_on_top) need effect handling.
    fn build(
        app: &AppHandle,
        store: &SettingsStore,
        patch: PersistedStatePatch,
    ) -> Result<Option<Self>, String> {
        let previous = load_persisted_state_snapshot(app, store)?;
        let next = merge_persisted_state_patch(&previous, patch);
        if next == previous {
            return Ok(None);
        }

        Ok(Some(Self {
            changes: PersistedStateChanges::between(&previous, &next),
            previous,
            next,
        }))
    }
}

fn apply_always_on_top(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        win.set_always_on_top(enabled).map_err(|e| e.to_string())
    } else {
        Err("Main window not found".to_string())
    }
}

fn apply_always_on_top_with_i18n(app: &AppHandle, enabled: bool, tr: &I18n) -> Result<(), String> {
    apply_always_on_top(app, enabled).map_err(|e| {
        let prefix = if enabled {
            tr.t("errWindowPinEnable")
        } else {
            tr.t("errWindowPinDisable")
        };
        format!("{prefix}: {e}")
    })
}

pub fn load_persisted_state_snapshot(
    app: &AppHandle,
    store: &SettingsStore,
) -> Result<PersistedState, String> {
    let mut state = store.load_persisted_state()?;
    // Pin state can drift from the stored value, so reads overlay the live window state
    // without mutating the DB during a getter.
    if let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Ok(actual) = win.is_always_on_top() {
            state.always_on_top = actual;
        }
    }
    Ok(state)
}

pub fn get_persisted_state(app: &AppHandle, store: &SettingsStore) -> Result<PersistedState, String> {
    load_persisted_state_snapshot(app, store)
}

fn merge_persisted_state_patch(
    current: &PersistedState,
    patch: PersistedStatePatch,
) -> PersistedState {
    PersistedState {
        window_x: patch.window_x.unwrap_or(current.window_x),
        window_y: patch.window_y.unwrap_or(current.window_y),
        always_on_top: patch.always_on_top.unwrap_or(current.always_on_top),
    }
}

pub fn save_persisted_state(
    app: &AppHandle,
    store: &SettingsStore,
    i18n: &Arc<RwLock<I18n>>,
    patch: PersistedStatePatch,
) -> Result<(), String> {
    let Some(plan) = PersistedStateSavePlan::build(app, store, patch)? else {
        return Ok(());
    };

    // This save is atomic at the plan level: if a changed runtime effect fails,
    // the persisted state rolls back to the previous snapshot, including position fields.
    store.save_persisted_state(&plan.next)?;

    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;
    if plan.changes.always_on_top_changed {
        if let Err(err) = apply_always_on_top_with_i18n(app, plan.next.always_on_top, &tr) {
            if let Err(rollback_err) = store.save_persisted_state(&plan.previous) {
                return Err(format!(
                    "{err}\nrollback persisted_state failed: {rollback_err}"
                ));
            }
            return Err(err);
        }
    }
    Ok(())
}

pub fn get_window_position(store: &SettingsStore) -> Result<Option<(i32, i32)>, String> {
    let state = store.load_persisted_state()?;
    Ok(state.window_x.zip(state.window_y))
}

pub fn restore_window_always_on_top(app: &AppHandle, store: &SettingsStore) -> Result<(), String> {
    let state = store.load_persisted_state()?;
    apply_always_on_top(app, state.always_on_top)
}
