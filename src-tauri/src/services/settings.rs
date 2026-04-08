use std::sync::{Arc, RwLock};

use log::{error, info};
use tauri::{
    menu::{Menu, MenuItem},
    AppHandle,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

use crate::db::{Database, SettingsStore};
use crate::i18n::I18n;
use crate::models::{AppSettings, AppSettingsPatch};
use crate::services::prune;
use crate::watcher::ClipboardWatcher;

struct SettingsChanges {
    autostart_changed: bool,
    hotkey_changed: bool,
    retention_changed: bool,
    language_changed: bool,
    log_level_changed: bool,
}

impl SettingsChanges {
    fn between(previous: &AppSettings, next: &AppSettings, previous_autostart: bool) -> Self {
        Self {
            autostart_changed: next.autostart != previous_autostart,
            hotkey_changed: next.hotkey != previous.hotkey,
            retention_changed: next.expiry_seconds != previous.expiry_seconds
                || next.max_history != previous.max_history
                || next.capture_images != previous.capture_images,
            language_changed: next.language != previous.language,
            log_level_changed: next.log_level != previous.log_level,
        }
    }
}

struct SettingsSavePlan {
    previous: AppSettings,
    next: AppSettings,
    changes: SettingsChanges,
    previous_autostart: bool,
}

impl SettingsSavePlan {
    // Save plan is built from a runtime-reconciled snapshot so the getter baseline,
    // diff baseline, and rollback baseline all describe the same state.
    fn build(
        app: &AppHandle,
        store: &SettingsStore,
        patch: AppSettingsPatch,
    ) -> Result<Option<Self>, String> {
        let previous = load_settings_snapshot(app, store)?;
        let previous_autostart = previous.autostart;
        let next = SettingsStore::sanitize_app_settings(&merge_settings_patch(&previous, patch));
        if next == previous {
            return Ok(None);
        }

        Ok(Some(Self {
            changes: SettingsChanges::between(&previous, &next, previous_autostart),
            previous,
            next,
            previous_autostart,
        }))
    }
}

fn load_settings_snapshot(app: &AppHandle, store: &SettingsStore) -> Result<AppSettings, String> {
    let mut settings = store.load_app_settings()?;
    // Autostart can drift outside the DB, so reads return the actual runtime state
    // without writing back as a side effect of the getter.
    if let Ok(actual) = app.autolaunch().is_enabled() {
        settings.autostart = actual;
    }
    Ok(settings)
}

fn merge_settings_patch(previous: &AppSettings, patch: AppSettingsPatch) -> AppSettings {
    AppSettings {
        hotkey: patch.hotkey.unwrap_or_else(|| previous.hotkey.clone()),
        autostart: patch.autostart.unwrap_or(previous.autostart),
        max_history: patch.max_history.unwrap_or(previous.max_history),
        theme: patch.theme.unwrap_or_else(|| previous.theme.clone()),
        language: patch.language.unwrap_or_else(|| previous.language.clone()),
        expiry_seconds: patch.expiry_seconds.unwrap_or(previous.expiry_seconds),
        capture_images: patch.capture_images.unwrap_or(previous.capture_images),
        log_level: patch
            .log_level
            .unwrap_or_else(|| previous.log_level.clone()),
    }
}

fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable().map_err(|e| e.to_string())
    } else {
        app.autolaunch().disable().map_err(|e| e.to_string())
    }
}

fn apply_autostart_with_i18n(app: &AppHandle, enabled: bool, tr: &I18n) -> Result<(), String> {
    apply_autostart(app, enabled).map_err(|e| {
        let prefix = if enabled {
            tr.t("errAutostartEnable")
        } else {
            tr.t("errAutostartDisable")
        };
        format!("{prefix}: {e}")
    })
}

fn rollback_settings_state(
    app: &AppHandle,
    store: &SettingsStore,
    previous: &AppSettings,
    previous_autostart: bool,
) -> Result<(), String> {
    let mut failures = Vec::new();

    if let Err(e) = store.save_user_settings(previous) {
        failures.push(format!("settings store: {e}"));
    }
    if let Err(e) = apply_autostart(app, previous_autostart) {
        failures.push(format!("autostart: {e}"));
    }
    if let Err(e) = crate::utils::hotkey::register_hotkey(app, &previous.hotkey) {
        failures.push(format!("hotkey: {e}"));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn with_rollback_notice(base: String, rollback_err: Option<String>, tr: &I18n) -> String {
    if let Some(err) = rollback_err {
        format!("{base}\n{}: {err}", tr.t("errSettingsRollback"))
    } else {
        base
    }
}

fn refresh_runtime_settings(watcher: &ClipboardWatcher, settings: &AppSettings) {
    watcher.refresh_settings(
        settings.expiry_seconds,
        settings.max_history,
        settings.capture_images,
    );
}

fn update_tray_language(app: &AppHandle, i18n: &Arc<RwLock<I18n>>, language: &str) {
    let new_tr = crate::i18n::load(language);
    let (show_txt, quit_txt) = (new_tr.t("show"), new_tr.t("quit"));
    if let Ok(mut guard) = i18n.write() {
        *guard = new_tr;
    }
    if let Some(tray) = app.tray_by_id("main_tray") {
        if let (Ok(si), Ok(qi)) = (
            MenuItem::with_id(app, "show", &show_txt, true, None::<&str>),
            MenuItem::with_id(app, "quit", &quit_txt, true, None::<&str>),
        ) {
            if let Ok(menu) = Menu::with_items(app, &[&si, &qi]) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    }
}

fn rollback_after_settings_failure(
    app: &AppHandle,
    store: &SettingsStore,
    plan: &SettingsSavePlan,
    reason: &str,
) -> Option<String> {
    let rollback_err =
        rollback_settings_state(app, store, &plan.previous, plan.previous_autostart).err();
    if let Some(ref rollback_err) = rollback_err {
        error!(
            "Settings rollback failed after {}: {}",
            reason, rollback_err
        );
    }
    rollback_err
}

fn fail_settings_save(
    app: &AppHandle,
    store: &SettingsStore,
    plan: &SettingsSavePlan,
    tr: &I18n,
    reason: &str,
    base_message: String,
) -> Result<(), String> {
    error!(
        "Failed to {} while saving settings: {}",
        reason, base_message
    );
    let rollback_err = rollback_after_settings_failure(app, store, plan, reason);
    Err(with_rollback_notice(base_message, rollback_err, tr))
}

fn apply_autostart_change(
    app: &AppHandle,
    plan: &SettingsSavePlan,
    tr: &I18n,
) -> Result<(), String> {
    if !plan.changes.autostart_changed {
        return Ok(());
    }
    apply_autostart_with_i18n(app, plan.next.autostart, tr)
}

fn apply_hotkey_change(app: &AppHandle, plan: &SettingsSavePlan, tr: &I18n) -> Result<(), String> {
    if !plan.changes.hotkey_changed {
        return Ok(());
    }
    crate::utils::hotkey::register_hotkey(app, &plan.next.hotkey)
        .map_err(|e| format!("{}: {}", tr.t("errHotkeyRegister"), e))
}

fn apply_retention_change(
    app: &AppHandle,
    db: &Database,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    plan: &SettingsSavePlan,
    tr: &I18n,
) -> Result<(), String> {
    if !plan.changes.retention_changed {
        return Ok(());
    }

    refresh_runtime_settings(watcher, &plan.next);
    if let Err(e) = prune::prune(
        app,
        db,
        data_dir,
        plan.next.expiry_seconds,
        plan.next.max_history,
    ) {
        refresh_runtime_settings(watcher, &plan.previous);
        return Err(format!("{}: {}", tr.t("errSettingsPrune"), e));
    }

    Ok(())
}

fn apply_critical_settings_changes(
    app: &AppHandle,
    db: &Database,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    plan: &SettingsSavePlan,
    tr: &I18n,
) -> Result<(), String> {
    apply_autostart_change(app, plan, tr)?;
    apply_hotkey_change(app, plan, tr)?;
    apply_retention_change(app, db, watcher, data_dir, plan, tr)
}

fn apply_post_save_settings_effects(
    app: &AppHandle,
    i18n: &Arc<RwLock<I18n>>,
    plan: &SettingsSavePlan,
) {
    if plan.changes.log_level_changed {
        crate::utils::logging::set_level(&plan.next.log_level);
    }
    info!(
        "Settings saved: autostart={}, max_history={}, expiry_seconds={}, capture_images={}, log_level={}",
        plan.next.autostart,
        plan.next.max_history,
        plan.next.expiry_seconds,
        plan.next.capture_images,
        plan.next.log_level
    );

    if plan.changes.language_changed {
        update_tray_language(app, i18n, &plan.next.language);
    }
}

pub fn get_settings(app: &AppHandle, store: &SettingsStore) -> Result<AppSettings, String> {
    load_settings_snapshot(app, store)
}

#[allow(clippy::too_many_arguments)]
pub fn save_settings(
    app: &AppHandle,
    db: &Database,
    store: &SettingsStore,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    i18n: &Arc<RwLock<I18n>>,
    patch: AppSettingsPatch,
) -> Result<(), String> {
    let Some(plan) = SettingsSavePlan::build(app, store, patch)? else {
        return Ok(());
    };
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;

    // Persist first, then apply changed runtime effects. If any critical effect fails,
    // rollback restores the whole settings state for this save plan.
    store.save_user_settings(&plan.next)?;
    if let Err(base_message) =
        apply_critical_settings_changes(app, db, watcher, data_dir, &plan, &tr)
    {
        return fail_settings_save(
            app,
            store,
            &plan,
            &tr,
            "apply critical changes",
            base_message,
        );
    }
    drop(tr);
    apply_post_save_settings_effects(app, i18n, &plan);
    Ok(())
}
