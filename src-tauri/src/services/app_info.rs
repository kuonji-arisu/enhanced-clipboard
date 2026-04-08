use tauri::AppHandle;

use crate::constants::{
    DEFAULT_HOTKEY, DEFAULT_MAX_HISTORY, EXPIRY_PRESETS, LOG_LEVEL_OPTIONS, MAX_HISTORY_ENTRIES,
    MAX_PINNED_ENTRIES, MIN_HISTORY_ENTRIES, PAGE_SIZE,
};
use crate::models::{AppConstants, AppInfo, AppRuntimeInfo};

pub fn get_app_info(app: &AppHandle) -> AppInfo {
    AppInfo {
        runtime: AppRuntimeInfo {
            locale: sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string()),
            version: app.package_info().version.to_string(),
            os: std::env::consts::OS.to_string(),
        },
        constants: AppConstants {
            default_hotkey: DEFAULT_HOTKEY.to_string(),
            default_max_history: DEFAULT_MAX_HISTORY,
            min_history_limit: MIN_HISTORY_ENTRIES,
            max_history_limit: MAX_HISTORY_ENTRIES,
            page_size: PAGE_SIZE,
            max_pinned_entries: MAX_PINNED_ENTRIES,
            expiry_presets: EXPIRY_PRESETS.to_vec(),
            log_level_options: LOG_LEVEL_OPTIONS.iter().map(|level| level.to_string()).collect(),
        },
    }
}
