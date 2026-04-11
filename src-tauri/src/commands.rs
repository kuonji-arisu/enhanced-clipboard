/// 命令薄层：接收参数 → 调用服务层 → 返回结果。
use std::sync::{Arc, RwLock};

use tauri::{Emitter, State};

use crate::constants::EVENT_ENTRIES_REMOVED;
use crate::db::{Database, SettingsStore};
use crate::i18n::I18n;
use crate::models::{
    AppInfo, AppInfoState, AppSettings, AppSettingsPatch, ClipboardEntriesQuery, ClipboardEntry,
    DataDir, PersistedState, PersistedStatePatch, RuntimeStatus, RuntimeStatusState,
    SavePersistedResult, SaveSettingsResult,
};
use crate::services as svc;
use crate::watcher::ClipboardWatcher;

// ── 剪贴板命令 ───────────────────────────────────────────────────────────────

/// 统一查询入口：基于复合游标分页（cursor_ts + cursor_id）。
/// 首页先返回命中的置顶条目，再返回第一页非置顶条目；后续翻页仅返回非置顶条目。
#[tauri::command]
pub fn get_app_info(app_info: State<'_, AppInfoState>) -> Result<AppInfo, String> {
    Ok(svc::app_info::get_app_info(app_info.inner()))
}

#[tauri::command]
pub fn get_entries(
    db: State<'_, Arc<Database>>,
    settings: State<'_, Arc<SettingsStore>>,
    data_dir: State<'_, DataDir>,
    query: ClipboardEntriesQuery,
) -> Result<Vec<ClipboardEntry>, String> {
    let s = settings.load_runtime_app_settings()?;
    let ws = svc::prune::window_start(s.expiry_seconds);
    let include_pinned = query.is_first_page();

    let normal = svc::query::get_normal_page(&db, &data_dir.0, &query, ws)?;

    // 首页同时返回命中的置顶（置顶不参与分页）
    if include_pinned {
        let mut pinned = svc::query::get_pinned_entries(&db, &data_dir.0, &query)?;
        pinned.extend(normal);
        Ok(pinned)
    } else {
        Ok(normal)
    }
}

#[tauri::command]
pub fn resolve_entry_for_query(
    db: State<'_, Arc<Database>>,
    settings: State<'_, Arc<SettingsStore>>,
    data_dir: State<'_, DataDir>,
    id: String,
    query: ClipboardEntriesQuery,
) -> Result<Option<ClipboardEntry>, String> {
    let s = settings.load_runtime_app_settings()?;
    let ws = svc::prune::window_start(s.expiry_seconds);
    svc::query::resolve_entry_for_query(&db, &data_dir.0, &query, ws, &id)
}

#[tauri::command]
pub fn copy_entry(
    db: State<'_, Arc<Database>>,
    watcher: State<'_, ClipboardWatcher>,
    data_dir: State<'_, DataDir>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    id: String,
) -> Result<(), String> {
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;
    svc::entry::copy_to_clipboard(&db, &watcher, &data_dir.0, &id, &tr)
}

#[tauri::command]
pub fn delete_entry(
    app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    data_dir: State<'_, DataDir>,
    id: String,
) -> Result<(), String> {
    if svc::entry::remove_entry(&db, &data_dir.0, &id)? {
        app.emit(EVENT_ENTRIES_REMOVED, vec![id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn clear_all(
    app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    data_dir: State<'_, DataDir>,
) -> Result<(), String> {
    let removed_ids = svc::entry::clear_all_entries(&db, &data_dir.0)?;
    if !removed_ids.is_empty() {
        app.emit(EVENT_ENTRIES_REMOVED, removed_ids)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_active_dates(
    db: State<'_, Arc<Database>>,
    settings: State<'_, Arc<SettingsStore>>,
    year_month: String,
) -> Result<Vec<String>, String> {
    let ws = svc::prune::window_start(settings.load_runtime_app_settings()?.expiry_seconds);
    svc::query::get_active_dates(&db, &year_month, ws)
}

#[tauri::command]
pub fn get_earliest_month(
    db: State<'_, Arc<Database>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Option<String>, String> {
    let ws = svc::prune::window_start(settings.load_runtime_app_settings()?.expiry_seconds);
    svc::query::get_earliest_month(&db, ws)
}

#[tauri::command]
pub fn toggle_pin(
    app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    settings: State<'_, Arc<SettingsStore>>,
    data_dir: State<'_, DataDir>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    id: String,
) -> Result<(), String> {
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;
    svc::entry::toggle_pin_entry(&app, &db, &settings, &data_dir.0, &id, &tr)
}

// ── 设置命令 ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(store: State<'_, Arc<SettingsStore>>) -> Result<AppSettings, String> {
    svc::settings::get_settings(&store)
}

#[tauri::command]
pub fn save_settings(
    app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    store: State<'_, Arc<SettingsStore>>,
    watcher: State<'_, ClipboardWatcher>,
    data_dir: State<'_, DataDir>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    patch: AppSettingsPatch,
) -> Result<SaveSettingsResult, String> {
    svc::settings::save_settings(&app, &db, &store, &watcher, &data_dir.0, &i18n, patch)
}

#[tauri::command]
pub fn get_persisted(store: State<'_, Arc<SettingsStore>>) -> Result<PersistedState, String> {
    svc::persisted_state::get_persisted(&store)
}

#[tauri::command]
pub fn save_persisted(
    app: tauri::AppHandle,
    store: State<'_, Arc<SettingsStore>>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    patch: PersistedStatePatch,
) -> Result<SavePersistedResult, String> {
    svc::persisted_state::save_persisted(&app, &store, &i18n, patch)
}

#[tauri::command]
pub fn get_runtime_status(
    runtime_status: State<'_, Arc<RuntimeStatusState>>,
) -> Result<RuntimeStatus, String> {
    svc::runtime::get_runtime_status(runtime_status.inner())
}

// ── 热键命令 ─────────────────────────────────────────────────────────────────

/// 录制快捷键时暂停监听，避免旧热键触发窗口隐藏。
#[tauri::command]
pub fn pause_hotkey(app: tauri::AppHandle) -> Result<(), String> {
    crate::utils::hotkey::unregister_hotkey(&app)
}

/// 录制结束后恢复当前已保存的热键。
#[tauri::command]
pub fn resume_hotkey(
    app: tauri::AppHandle,
    store: State<'_, Arc<SettingsStore>>,
) -> Result<(), String> {
    let hotkey = store.load_runtime_app_settings()?.hotkey;
    crate::utils::hotkey::register_hotkey(&app, &hotkey)
}
