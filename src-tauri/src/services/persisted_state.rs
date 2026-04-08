use std::sync::{Arc, RwLock};

use log::warn;
use tauri::{AppHandle, Manager};

use crate::constants::MAIN_WINDOW_LABEL;
use crate::db::SettingsStore;
use crate::i18n::I18n;
use crate::models::PersistedState;

fn apply_always_on_top(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        win.set_always_on_top(enabled).map_err(|e| e.to_string())
    } else {
        Err("Main window not found".to_string())
    }
}

fn apply_always_on_top_with_i18n(
    app: &AppHandle,
    enabled: bool,
    tr: &I18n,
) -> Result<(), String> {
    apply_always_on_top(app, enabled).map_err(|e| {
        let prefix = if enabled {
            tr.t("errWindowPinEnable")
        } else {
            tr.t("errWindowPinDisable")
        };
        format!("{prefix}: {e}")
    })
}

pub fn get_persisted_state(
    app: &AppHandle,
    store: &SettingsStore,
) -> Result<PersistedState, String> {
    let mut state = store.load_persisted_state()?;
    if let Some(win) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Ok(actual) = win.is_always_on_top() {
            if state.always_on_top != actual {
                state.always_on_top = actual;
                if let Err(e) = store.save_always_on_top(actual) {
                    warn!(
                        "Failed to persist always_on_top while syncing runtime state: {}",
                        e
                    );
                }
            }
        }
    }
    Ok(state)
}

pub fn set_always_on_top(
    app: &AppHandle,
    store: &SettingsStore,
    i18n: &Arc<RwLock<I18n>>,
    enabled: bool,
) -> Result<(), String> {
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;
    apply_always_on_top_with_i18n(app, enabled, &tr)?;

    if let Err(e) = store.save_always_on_top(enabled) {
        warn!(
            "Failed to persist always_on_top setting after applying runtime state: {}",
            e
        );
    }

    Ok(())
}

pub fn get_window_position(store: &SettingsStore) -> Result<Option<(i32, i32)>, String> {
    let state = store.load_persisted_state()?;
    Ok(state.window_x.zip(state.window_y))
}

pub fn save_window_position(store: &SettingsStore, x: i32, y: i32) -> Result<(), String> {
    let previous = store.load_persisted_state()?;
    if previous.window_x == Some(x) && previous.window_y == Some(y) {
        return Ok(());
    }
    store.save_window_position(Some(x), Some(y))
}

pub fn restore_window_always_on_top(
    app: &AppHandle,
    store: &SettingsStore,
) -> Result<(), String> {
    let state = store.load_persisted_state()?;
    apply_always_on_top(app, state.always_on_top)
}
