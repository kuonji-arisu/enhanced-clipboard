use std::sync::{Arc, RwLock};

use log::{error, warn};
use tauri::{AppHandle, Manager, Runtime};

use crate::constants::MAIN_WINDOW_LABEL;
use crate::db::SettingsStore;
use crate::i18n::I18n;
use crate::models::{
    EffectResult, PersistedEffectKey, PersistedField, PersistedState, PersistedStatePatch,
    PersistenceDomain, SavePersistedEffects, SavePersistedResult, SaveStrategy,
};

pub trait PersistedApp {
    fn set_always_on_top(&self, enabled: bool) -> Result<(), String>;
    fn restore_window_position(&self, x: i32, y: i32) -> Result<(), String>;
}

impl<R: Runtime> PersistedApp for AppHandle<R> {
    fn set_always_on_top(&self, enabled: bool) -> Result<(), String> {
        if let Some(win) = self.get_webview_window(MAIN_WINDOW_LABEL) {
            win.set_always_on_top(enabled).map_err(|e| e.to_string())
        } else {
            Err("Main window not found".to_string())
        }
    }

    fn restore_window_position(&self, x: i32, y: i32) -> Result<(), String> {
        if let Some(win) = self.get_webview_window(MAIN_WINDOW_LABEL) {
            win.set_position(tauri::PhysicalPosition::new(x, y))
                .map_err(|e| e.to_string())
        } else {
            Err("Main window not found".to_string())
        }
    }
}

fn merge_persisted_patch(current: &PersistedState, patch: PersistedStatePatch) -> PersistedState {
    PersistedState {
        window_x: patch.window_x.unwrap_or(current.window_x),
        window_y: patch.window_y.unwrap_or(current.window_y),
        always_on_top: patch.always_on_top.unwrap_or(current.always_on_top),
    }
}

fn effect_ok() -> EffectResult {
    EffectResult {
        ok: true,
        error: None,
    }
}

fn effect_error(message: String) -> EffectResult {
    EffectResult {
        ok: false,
        error: Some(message),
    }
}

fn apply_always_on_top_effect(app: &impl PersistedApp, enabled: bool, tr: &I18n) -> EffectResult {
    match app.set_always_on_top(enabled) {
        Ok(()) => effect_ok(),
        Err(e) => {
            let prefix = if enabled {
                tr.t("errWindowPinEnable")
            } else {
                tr.t("errWindowPinDisable")
            };
            let message = format!("{prefix}: {e}");
            error!("Persisted effect failed (always_on_top)");
            effect_error(message)
        }
    }
}

fn collect_changed_fields(current: &PersistedState, next: &PersistedState) -> Vec<PersistedField> {
    PersistedField::ALL
        .into_iter()
        .filter(|field| field.changed(current, next))
        .collect()
}

fn select_fields_by_strategy(
    fields: &[PersistedField],
    strategy: SaveStrategy,
) -> Vec<PersistedField> {
    fields
        .iter()
        .copied()
        .filter(|field| {
            let metadata = field.metadata();
            debug_assert_eq!(metadata.domain, PersistenceDomain::Persisted);
            metadata.strategy == strategy
        })
        .collect()
}

fn persist_persisted_fields(
    store: &SettingsStore,
    state: &PersistedState,
    fields: &[PersistedField],
    tr: &I18n,
) -> Result<(), String> {
    store
        .save_persisted_state_fields(state, fields)
        .map_err(|e| format!("{}: {}", tr.t("errPersistedStatePersist"), e))
}

fn collect_effect_keys(
    fields: &[PersistedField],
    strategy: SaveStrategy,
) -> Vec<PersistedEffectKey> {
    let mut keys = Vec::new();
    for field in fields {
        let metadata = field.metadata();
        if metadata.strategy != strategy {
            continue;
        }
        if let Some(effect) = metadata.effect {
            if !keys.contains(&effect) {
                keys.push(effect);
            }
        }
    }
    keys
}

fn record_effect_result(
    effects: &mut SavePersistedEffects,
    key: PersistedEffectKey,
    result: EffectResult,
) {
    match key {
        PersistedEffectKey::AlwaysOnTop => effects.always_on_top = Some(result),
    }
}

fn copy_changed_persisted_field(
    target: &mut PersistedState,
    source: &PersistedState,
    field: PersistedField,
) {
    match field {
        PersistedField::WindowX => target.window_x = source.window_x,
        PersistedField::WindowY => target.window_y = source.window_y,
        PersistedField::AlwaysOnTop => target.always_on_top = source.always_on_top,
    }
}

pub fn get_persisted(store: &SettingsStore) -> Result<PersistedState, String> {
    store.load_persisted_state()
}

pub fn save_persisted(
    app: &impl PersistedApp,
    store: &SettingsStore,
    i18n: &Arc<RwLock<I18n>>,
    patch: PersistedStatePatch,
) -> Result<SavePersistedResult, String> {
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;
    let current = store.load_persisted_state()?;
    let next = merge_persisted_patch(&current, patch);
    let changed_fields = collect_changed_fields(&current, &next);

    if changed_fields.is_empty() {
        return Ok(SavePersistedResult {
            persisted: current,
            effects: None,
        });
    }

    let persist_only_fields = select_fields_by_strategy(&changed_fields, SaveStrategy::PersistOnly);
    let apply_then_persist_fields =
        select_fields_by_strategy(&changed_fields, SaveStrategy::ApplyThenPersist);

    persist_persisted_fields(store, &next, &persist_only_fields, &tr)?;

    let mut final_state = current.clone();
    for field in &persist_only_fields {
        copy_changed_persisted_field(&mut final_state, &next, *field);
    }

    let mut effects = SavePersistedEffects::default();
    for effect in collect_effect_keys(&apply_then_persist_fields, SaveStrategy::ApplyThenPersist) {
        match effect {
            PersistedEffectKey::AlwaysOnTop => {
                let result = apply_always_on_top_effect(app, next.always_on_top, &tr);
                let effect_ok = result.ok;
                record_effect_result(&mut effects, effect, result);
                if effect_ok {
                    let fields_to_persist = apply_then_persist_fields
                        .iter()
                        .copied()
                        .filter(|field| {
                            field.metadata().effect == Some(PersistedEffectKey::AlwaysOnTop)
                        })
                        .collect::<Vec<_>>();
                    persist_persisted_fields(store, &next, &fields_to_persist, &tr)?;
                    for field in &fields_to_persist {
                        copy_changed_persisted_field(&mut final_state, &next, *field);
                    }
                }
            }
        }
    }

    Ok(SavePersistedResult {
        persisted: final_state,
        effects: (!effects.is_empty()).then_some(effects),
    })
}

pub fn restore_persisted_effects(
    app: &impl PersistedApp,
    store: &SettingsStore,
) -> Result<(), String> {
    let state = store.load_persisted_state()?;

    if let Some((x, y)) = state.window_x.zip(state.window_y) {
        if let Err(err) = app.restore_window_position(x, y) {
            warn!("Failed to restore window position: {}", err);
        }
    }

    if let Err(err) = app.set_always_on_top(state.always_on_top) {
        warn!("Failed to restore always-on-top state: {}", err);
    }

    Ok(())
}
