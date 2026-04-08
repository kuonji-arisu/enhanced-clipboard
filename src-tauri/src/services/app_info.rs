use tauri::AppHandle;

use crate::constants::{
    DEFAULT_HOTKEY, DEFAULT_MAX_HISTORY, EXPIRY_PRESETS, LOG_LEVEL_OPTIONS, MAX_HISTORY_ENTRIES,
    MAX_PINNED_ENTRIES, MIN_HISTORY_ENTRIES, PAGE_SIZE,
};
use crate::models::{AppInfo, AppInfoState};

pub fn build_app_info(app: &AppHandle) -> AppInfo {
    AppInfo {
        locale: crate::i18n::current_locale(),
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS.to_string(),
        default_hotkey: DEFAULT_HOTKEY.to_string(),
        default_max_history: DEFAULT_MAX_HISTORY,
        min_history_limit: MIN_HISTORY_ENTRIES,
        max_history_limit: MAX_HISTORY_ENTRIES,
        page_size: PAGE_SIZE,
        max_pinned_entries: MAX_PINNED_ENTRIES,
        expiry_presets: EXPIRY_PRESETS.to_vec(),
        log_level_options: LOG_LEVEL_OPTIONS
            .iter()
            .map(|level| level.to_string())
            .collect(),
    }
}

pub fn get_app_info(state: &AppInfoState) -> AppInfo {
    state.0.clone()
}
