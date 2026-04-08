/// 命令薄层：接收参数 → 调用服务层 → 返回结果。
use std::sync::{Arc, RwLock};

use tauri::{Emitter, State};

use crate::constants::{EVENT_ENTRIES_REMOVED, PAGE_SIZE};
use crate::db::{Database, SettingsStore};
use crate::i18n::I18n;
use crate::models::{
    AppInfo, AppSettings, AppSettingsPatch, ClipboardEntry, DataDir, RuntimeStatus,
    RuntimeStatusState,
};
use crate::services as svc;
use crate::watcher::ClipboardWatcher;

// ── 剪贴板命令 ───────────────────────────────────────────────────────────────

/// 统一查询入口：基于复合游标分页（cursor_ts + cursor_id），query 非空时走搜索。
/// 仅未搜索、未按日期筛选的首页（cursor_ts 为 None）返回全部置顶 + 第一页非置顶；
/// 其他情况只返回严格命中的结果。
#[tauri::command]
pub fn get_app_info(app: tauri::AppHandle) -> Result<AppInfo, String> {
    Ok(svc::app_info::get_app_info(&app))
}

#[tauri::command]
pub fn get_entries(
    db: State<'_, Arc<Database>>,
    settings: State<'_, Arc<SettingsStore>>,
    data_dir: State<'_, DataDir>,
    query: Option<String>,
    date: Option<String>,
    cursor_ts: Option<i64>,
    cursor_id: Option<String>,
    limit: u32,
) -> Result<Vec<ClipboardEntry>, String> {
    let s = settings.load_app_settings()?;
    let ws = svc::prune::window_start(s.expiry_seconds);
    let has_query = query.as_deref().is_some_and(|q| !q.trim().is_empty());
    let include_pinned = cursor_ts.is_none() && !has_query && date.is_none();
    let limit = limit.clamp(1, PAGE_SIZE);

    let normal = svc::query::get_normal_page(
        &db,
        &data_dir.0,
        query.as_deref(),
        date,
        ws,
        cursor_ts,
        cursor_id.as_deref(),
        limit,
    )?;

    // 仅默认首页同时返回置顶（置顶不参与分页）
    if include_pinned {
        let mut pinned = svc::query::get_pinned_entries(&db, &data_dir.0)?;
        pinned.extend(normal);
        Ok(pinned)
    } else {
        Ok(normal)
    }
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
    let ws = svc::prune::window_start(settings.load_app_settings()?.expiry_seconds);
    svc::query::get_active_dates(&db, &year_month, ws)
}

#[tauri::command]
pub fn get_earliest_month(
    db: State<'_, Arc<Database>>,
    settings: State<'_, Arc<SettingsStore>>,
) -> Result<Option<String>, String> {
    let ws = svc::prune::window_start(settings.load_app_settings()?.expiry_seconds);
    svc::query::get_earliest_month(&db, ws)
}

#[tauri::command]
pub fn toggle_pin(
    db: State<'_, Arc<Database>>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    id: String,
) -> Result<bool, String> {
    let tr = i18n.read().map_err(|_| "i18n lock poisoned".to_string())?;
    svc::entry::toggle_pin_entry(&db, &id, &tr)
}

// ── 设置命令 ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(
    app: tauri::AppHandle,
    store: State<'_, Arc<SettingsStore>>,
) -> Result<AppSettings, String> {
    svc::settings::get_settings(&app, &store)
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
) -> Result<(), String> {
    svc::settings::save_settings(&app, &db, &store, &watcher, &data_dir.0, &i18n, patch)
}

#[tauri::command]
pub fn get_runtime_status(
    runtime_status: State<'_, Arc<RuntimeStatusState>>,
) -> Result<RuntimeStatus, String> {
    runtime_status
        .0
        .lock()
        .map(|status| status.clone())
        .map_err(|e| e.to_string())
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
    let hotkey = store.load_app_settings()?.hotkey;
    crate::utils::hotkey::register_hotkey(&app, &hotkey)
}
