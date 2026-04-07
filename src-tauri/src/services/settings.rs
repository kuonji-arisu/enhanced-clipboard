use std::sync::{Arc, RwLock};

use log::{error, info};
use tauri::{
    menu::{Menu, MenuItem},
    AppHandle,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

use crate::db::Database;
use crate::i18n::I18n;
use crate::models::AppSettings;
use crate::services::prune;
use crate::settings::SettingsStore;
use crate::watcher::ClipboardWatcher;

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

    if let Err(e) = store.save_app_settings(previous) {
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

pub fn get_settings(app: &AppHandle, store: &SettingsStore) -> Result<AppSettings, String> {
    let mut settings = store.load_app_settings()?;
    if let Ok(actual) = app.autolaunch().is_enabled() {
        if settings.autostart != actual {
            settings.autostart = actual;
            store.save_app_settings(&settings)?;
        }
    }
    Ok(settings)
}

#[allow(clippy::too_many_arguments)]
pub fn save_settings(
    app: &AppHandle,
    db: &Database,
    store: &SettingsStore,
    watcher: &ClipboardWatcher,
    data_dir: &std::path::Path,
    i18n: &Arc<RwLock<I18n>>,
    settings: AppSettings,
) -> Result<(), String> {
    let previous = store.load_app_settings()?;
    let previous_autostart = app.autolaunch().is_enabled().unwrap_or(previous.autostart);
    let next = SettingsStore::sanitize_app_settings(&settings);
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;

    store.save_app_settings(&next)?;

    if next.autostart != previous_autostart {
        if let Err(e) = apply_autostart_with_i18n(app, next.autostart, &tr) {
            error!("Failed to apply autostart while saving settings: {}", e);
            let rollback_err = rollback_settings_state(app, store, &previous, previous_autostart).err();
            if let Some(ref rollback_err) = rollback_err {
                error!("Settings rollback failed after autostart error: {}", rollback_err);
            }
            return Err(with_rollback_notice(e, rollback_err, &tr));
        }
    }

    if let Err(e) = crate::utils::hotkey::register_hotkey(app, &next.hotkey) {
        error!("Failed to register hotkey while saving settings: {}", e);
        watcher.refresh_settings(
            previous.expiry_seconds,
            previous.max_history,
            previous.capture_images,
        );
        let rollback_err = rollback_settings_state(app, store, &previous, previous_autostart).err();
        if let Some(ref rollback_err) = rollback_err {
            error!("Settings rollback failed after hotkey error: {}", rollback_err);
        }
        return Err(with_rollback_notice(
            format!("{}: {}", tr.t("errHotkeyRegister"), e),
            rollback_err,
            &tr,
        ));
    }

    watcher.refresh_settings(next.expiry_seconds, next.max_history, next.capture_images);
    if let Err(e) = prune::prune(app, db, data_dir, next.expiry_seconds, next.max_history) {
        error!("Failed to apply prune after saving settings: {}", e);
        watcher.refresh_settings(
            previous.expiry_seconds,
            previous.max_history,
            previous.capture_images,
        );
        let rollback_err = rollback_settings_state(app, store, &previous, previous_autostart).err();
        if let Some(ref rollback_err) = rollback_err {
            error!("Settings rollback failed after prune error: {}", rollback_err);
        }
        return Err(with_rollback_notice(
            format!("{}: {}", tr.t("errSettingsPrune"), e),
            rollback_err,
            &tr,
        ));
    }

    crate::utils::logging::set_level(&next.log_level);
    info!(
        "Settings saved: autostart={}, max_history={}, expiry_seconds={}, capture_images={}, log_level={}",
        next.autostart,
        next.max_history,
        next.expiry_seconds,
        next.capture_images,
        next.log_level
    );
    drop(tr);

    let new_tr = crate::i18n::load(&next.language);
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
    Ok(())
}
