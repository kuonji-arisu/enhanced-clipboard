use std::sync::{Arc, RwLock};

use log::{error, info, warn};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_autostart::AutoLaunchManager;

use crate::constants::{LOG_LEVEL_OPTIONS, MAX_HISTORY_ENTRIES, MIN_HISTORY_ENTRIES};
use crate::db::{Database, SettingsStore};
use crate::i18n::I18n;
use crate::models::{
    AppSettings, AppSettingsPatch, ClipboardQueryStaleReason, EffectResult, PersistenceDomain,
    SaveSettingsEffects, SaveSettingsResult, SaveStrategy, SettingsEffectKey, SettingsField,
};
use crate::services::view_events::EventEmitter;
use crate::services::{prune, view_events};
use crate::watcher::ClipboardWatcher;

pub trait SettingsApp: EventEmitter {
    fn apply_autostart(&self, enabled: bool) -> Result<(), String>;
    fn register_hotkey(&self, hotkey: &str) -> Result<(), String>;
}

impl<R: Runtime> SettingsApp for AppHandle<R> {
    fn apply_autostart(&self, enabled: bool) -> Result<(), String> {
        let manager = self
            .try_state::<AutoLaunchManager>()
            .ok_or_else(|| "autostart manager unavailable".to_string())?;
        if enabled {
            manager.enable().map_err(|e| e.to_string())
        } else {
            manager.disable().map_err(|e| e.to_string())
        }
    }

    fn register_hotkey(&self, hotkey: &str) -> Result<(), String> {
        crate::utils::hotkey::register_hotkey(self, hotkey)
    }
}

fn merge_settings_patch(current: &AppSettings, patch: AppSettingsPatch) -> AppSettings {
    AppSettings {
        hotkey: patch
            .hotkey
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|| current.hotkey.clone()),
        autostart: patch.autostart.unwrap_or(current.autostart),
        max_history: patch.max_history.unwrap_or(current.max_history),
        theme_mode: patch
            .theme_mode
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_else(|| current.theme_mode.clone()),
        expiry_seconds: patch.expiry_seconds.unwrap_or(current.expiry_seconds),
        capture_images: patch.capture_images.unwrap_or(current.capture_images),
        log_level: patch
            .log_level
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_else(|| current.log_level.clone()),
    }
}

fn validate_changed_fields(
    settings: &AppSettings,
    changed_fields: &[SettingsField],
    tr: &I18n,
) -> Result<(), String> {
    for field in changed_fields {
        match field {
            SettingsField::Hotkey => {
                crate::utils::hotkey::validate_hotkey(&settings.hotkey)
                    .map_err(|err| format!("{}: {}", tr.t("errInvalidHotkey"), err))?;
            }
            SettingsField::MaxHistory => {
                if !(MIN_HISTORY_ENTRIES..=MAX_HISTORY_ENTRIES).contains(&settings.max_history) {
                    return Err(tr.t("errInvalidMaxHistory"));
                }
            }
            SettingsField::ThemeMode => {
                if !matches!(settings.theme_mode.as_str(), "light" | "dark" | "system") {
                    return Err(tr.t("errInvalidTheme"));
                }
            }
            SettingsField::ExpirySeconds => {
                if settings.expiry_seconds < 0 {
                    return Err(tr.t("errInvalidExpirySeconds"));
                }
            }
            SettingsField::LogLevel => {
                if !LOG_LEVEL_OPTIONS.contains(&settings.log_level.as_str()) {
                    return Err(tr.t("errInvalidLogLevel"));
                }
            }
            SettingsField::Autostart | SettingsField::CaptureImages => {}
        }
    }

    Ok(())
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

fn apply_autostart_effect(app: &impl SettingsApp, enabled: bool, tr: &I18n) -> Result<(), String> {
    app.apply_autostart(enabled).map_err(|e| {
        let prefix = if enabled {
            tr.t("errAutostartEnable")
        } else {
            tr.t("errAutostartDisable")
        };
        format!("{prefix}: {e}")
    })
}

fn apply_hotkey_effect(app: &impl SettingsApp, hotkey: &str, tr: &I18n) -> Result<(), String> {
    app.register_hotkey(hotkey)
        .map_err(|e| format!("{}: {}", tr.t("errHotkeyRegister"), e))
}

fn refresh_runtime_settings(watcher: &ClipboardWatcher, settings: &AppSettings) {
    watcher.refresh_settings(
        settings.expiry_seconds,
        settings.max_history,
        settings.capture_images,
    );
}

fn apply_capture_images_effect(watcher: &ClipboardWatcher, settings: &AppSettings) {
    watcher.refresh_capture_images(settings.capture_images);
}

fn apply_retention_effect(
    app: &impl SettingsApp,
    db: &Database,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    settings: &AppSettings,
    tr: &I18n,
) -> Result<(), String> {
    refresh_runtime_settings(watcher, settings);
    let pruned = prune::prune(
        app,
        db,
        data_dir,
        settings.expiry_seconds,
        settings.max_history,
        ClipboardQueryStaleReason::SettingsOrStartup,
    )
    .map_err(|e| format!("{}: {}", tr.t("errSettingsPrune"), e))?;
    if !pruned {
        view_events::emit_query_results_stale(app, ClipboardQueryStaleReason::SettingsOrStartup)
            .map_err(|e| format!("{}: {}", tr.t("errSettingsPrune"), e))?;
    }
    Ok(())
}

fn apply_log_level_effect(settings: &AppSettings) {
    crate::utils::logging::set_level(&settings.log_level);
}

fn run_settings_effect(
    app: &impl SettingsApp,
    db: &Database,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    settings: &AppSettings,
    effect: SettingsEffectKey,
    tr: &I18n,
) -> EffectResult {
    let result = match effect {
        SettingsEffectKey::Autostart => apply_autostart_effect(app, settings.autostart, tr),
        SettingsEffectKey::Hotkey => apply_hotkey_effect(app, &settings.hotkey, tr),
        SettingsEffectKey::Retention => {
            apply_retention_effect(app, db, watcher, data_dir, settings, tr)
        }
        SettingsEffectKey::CaptureImages => {
            apply_capture_images_effect(watcher, settings);
            Ok(())
        }
        SettingsEffectKey::LogLevel => {
            apply_log_level_effect(settings);
            Ok(())
        }
    };

    match result {
        Ok(()) => effect_ok(),
        Err(err) => {
            error!("Settings effect failed ({effect:?})");
            effect_error(err)
        }
    }
}

fn copy_changed_setting(target: &mut AppSettings, source: &AppSettings, field: SettingsField) {
    match field {
        SettingsField::Hotkey => target.hotkey = source.hotkey.clone(),
        SettingsField::Autostart => target.autostart = source.autostart,
        SettingsField::MaxHistory => target.max_history = source.max_history,
        SettingsField::ThemeMode => target.theme_mode = source.theme_mode.clone(),
        SettingsField::ExpirySeconds => target.expiry_seconds = source.expiry_seconds,
        SettingsField::CaptureImages => target.capture_images = source.capture_images,
        SettingsField::LogLevel => target.log_level = source.log_level.clone(),
    }
}

fn collect_changed_fields(current: &AppSettings, next: &AppSettings) -> Vec<SettingsField> {
    SettingsField::ALL
        .into_iter()
        .filter(|field| field.changed(current, next))
        .collect()
}

fn select_fields_by_strategy(
    fields: &[SettingsField],
    strategy: SaveStrategy,
) -> Vec<SettingsField> {
    fields
        .iter()
        .copied()
        .filter(|field| {
            let metadata = field.metadata();
            debug_assert_eq!(metadata.domain, PersistenceDomain::Settings);
            metadata.strategy == strategy
        })
        .collect()
}

fn collect_effect_keys(fields: &[SettingsField], strategy: SaveStrategy) -> Vec<SettingsEffectKey> {
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
    effects: &mut SaveSettingsEffects,
    key: SettingsEffectKey,
    result: EffectResult,
) {
    match key {
        SettingsEffectKey::Autostart => effects.autostart = Some(result),
        SettingsEffectKey::Hotkey => effects.hotkey = Some(result),
        SettingsEffectKey::Retention => effects.retention = Some(result),
        SettingsEffectKey::CaptureImages => effects.capture_images = Some(result),
        SettingsEffectKey::LogLevel => effects.log_level = Some(result),
    }
}

fn persist_settings_fields(
    store: &SettingsStore,
    settings: &AppSettings,
    fields: &[SettingsField],
    tr: &I18n,
) -> Result<(), String> {
    store
        .save_app_settings_fields(settings, fields)
        .map_err(|e| format!("{}: {}", tr.t("errSettingsPersist"), e))
}

pub fn get_settings(store: &SettingsStore) -> Result<AppSettings, String> {
    store.load_app_settings()
}

#[allow(clippy::too_many_arguments)]
pub fn save_settings(
    app: &impl SettingsApp,
    db: &Database,
    store: &SettingsStore,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    i18n: &Arc<RwLock<I18n>>,
    patch: AppSettingsPatch,
) -> Result<SaveSettingsResult, String> {
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;
    let current = store.load_app_settings()?;
    let next = merge_settings_patch(&current, patch);
    let changed_fields = collect_changed_fields(&current, &next);
    if changed_fields.is_empty() {
        return Ok(SaveSettingsResult {
            settings: current,
            effects: SaveSettingsEffects::default(),
        });
    }
    validate_changed_fields(&next, &changed_fields, &tr)?;

    let persist_only_fields = select_fields_by_strategy(&changed_fields, SaveStrategy::PersistOnly);
    let persist_then_apply_fields =
        select_fields_by_strategy(&changed_fields, SaveStrategy::PersistThenApply);
    let apply_then_persist_fields =
        select_fields_by_strategy(&changed_fields, SaveStrategy::ApplyThenPersist);

    let db_first_fields = persist_only_fields
        .iter()
        .chain(persist_then_apply_fields.iter())
        .copied()
        .collect::<Vec<_>>();
    persist_settings_fields(store, &next, &db_first_fields, &tr)?;

    let mut effects = SaveSettingsEffects::default();
    let mut final_settings = current.clone();
    for field in &db_first_fields {
        copy_changed_setting(&mut final_settings, &next, *field);
    }

    for effect in collect_effect_keys(&persist_then_apply_fields, SaveStrategy::PersistThenApply) {
        let result = run_settings_effect(app, db, watcher, data_dir, &next, effect, &tr);
        record_effect_result(&mut effects, effect, result);
    }

    let mut apply_then_persist_successes = Vec::new();
    for effect in collect_effect_keys(&apply_then_persist_fields, SaveStrategy::ApplyThenPersist) {
        let result = run_settings_effect(app, db, watcher, data_dir, &next, effect, &tr);
        let effect_ok = result.ok;
        record_effect_result(&mut effects, effect, result);
        if effect_ok {
            for field in apply_then_persist_fields.iter().copied().filter(|field| {
                field.metadata().effect == Some(effect)
                    && field.metadata().strategy == SaveStrategy::ApplyThenPersist
            }) {
                apply_then_persist_successes.push(field);
            }
        }
    }

    persist_settings_fields(store, &next, &apply_then_persist_successes, &tr)?;
    for field in &apply_then_persist_successes {
        copy_changed_setting(&mut final_settings, &next, *field);
    }

    info!(
        "Settings saved: autostart={}, hotkey={}, max_history={}, theme_mode={}, expiry_seconds={}, capture_images={}, log_level={}",
        final_settings.autostart,
        final_settings.hotkey,
        final_settings.max_history,
        final_settings.theme_mode,
        final_settings.expiry_seconds,
        final_settings.capture_images,
        final_settings.log_level
    );

    Ok(SaveSettingsResult {
        settings: final_settings,
        effects,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn restore_settings_effects(
    app: &impl SettingsApp,
    db: &Database,
    store: &SettingsStore,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    i18n: &Arc<RwLock<I18n>>,
) -> Result<(), String> {
    let settings = store.load_runtime_app_settings()?;
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;

    // 当前 restore 只恢复设置意图本身，不直接改写 runtime 快照。
    // 如果未来某个设置副作用需要暴露实时结果，应统一通过 services::runtime::apply_patch 写入。
    apply_log_level_effect(&settings);
    refresh_runtime_settings(watcher, &settings);

    if let Err(err) = apply_autostart_effect(app, settings.autostart, &tr) {
        warn!("Failed to restore autostart intent");
        let _ = err;
    }
    if let Err(err) = apply_hotkey_effect(app, &settings.hotkey, &tr) {
        warn!("Failed to restore hotkey intent");
        let _ = err;
    }
    if let Err(err) = apply_retention_effect(app, db, watcher, data_dir, &settings, &tr) {
        warn!("Failed to restore retention intent");
        let _ = err;
    }

    Ok(())
}
