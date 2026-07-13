//! Thin Tauri command boundary. Clipboard commands only exchange request/reply
//! messages with the single-writer engine.

use std::sync::{Arc, RwLock};

use log::error;
use tauri::State;

use crate::clipboard::{
    ClipboardEngineHandle, ClipboardEntriesQuery, ClipboardError, ClipboardListPage,
    ImagePreviewRepairOutcome,
};
use crate::db::SettingsStore;
use crate::i18n::I18n;
use crate::models::{
    AppInfo, AppInfoState, AppSettings, AppSettingsPatch, PersistedState, PersistedStatePatch,
    RuntimeStatus, RuntimeStatusState, SavePersistedResult, SaveSettingsResult,
};
use crate::services as svc;

fn localize_dispatch_error(
    i18n: &Arc<RwLock<I18n>>,
    error_key: &str,
    operation: &str,
    dispatch_error: &tauri::Error,
) -> String {
    error!("{operation} dispatch task failed: {dispatch_error}");
    let Ok(tr) = i18n.read() else {
        error!("Failed to acquire i18n lock while reporting {operation} dispatch failure");
        return dispatch_error.to_string();
    };
    tr.t(error_key)
}

async fn dispatch_blocking<T, F>(
    i18n: &Arc<RwLock<I18n>>,
    error_key: &str,
    operation: &str,
    task: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|dispatch_error| {
            localize_dispatch_error(i18n, error_key, operation, &dispatch_error)
        })
}

async fn dispatch_clipboard_request<T, F>(
    clipboard: ClipboardEngineHandle,
    i18n: Arc<RwLock<I18n>>,
    operation: &'static str,
    request: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&ClipboardEngineHandle) -> Result<T, ClipboardError> + Send + 'static,
{
    let result = dispatch_blocking(&i18n, "errClipboardOperation", operation, move || {
        request(&clipboard)
    })
    .await?;
    clipboard_result(result, &i18n)
}

fn localize_clipboard_error(i18n: &Arc<RwLock<I18n>>, error: ClipboardError) -> String {
    let fallback = error.to_string();
    let Ok(tr) = i18n.read() else {
        error!("Failed to acquire i18n lock while reporting clipboard error: {fallback}");
        return fallback;
    };

    match error {
        ClipboardError::EntryNotFound => tr.t("errEntryNotFound"),
        ClipboardError::PinLimitExceeded { limit } => {
            tr.t_fmt("pinLimitMessage", &[("count", limit.to_string())])
        }
        ClipboardError::ImageOriginalUnavailable => tr.t("errImageFileMissing"),
        other => {
            error!("Clipboard command failed: {other}");
            tr.t("errClipboardOperation")
        }
    }
}

fn clipboard_result<T>(
    result: Result<T, ClipboardError>,
    i18n: &Arc<RwLock<I18n>>,
) -> Result<T, String> {
    result.map_err(|error| localize_clipboard_error(i18n, error))
}

#[tauri::command]
pub fn get_app_info(app_info: State<'_, AppInfoState>) -> Result<AppInfo, String> {
    Ok(svc::app_info::get_app_info(app_info.inner()))
}

#[tauri::command]
pub async fn get_clipboard_list_items(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    query: ClipboardEntriesQuery,
) -> Result<ClipboardListPage, String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard list",
        move |clipboard| clipboard.list(query),
    )
    .await
}

#[tauri::command]
pub async fn copy_entry(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    id: String,
) -> Result<(), String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard copy",
        move |clipboard| clipboard.copy(id),
    )
    .await
}

#[tauri::command]
pub async fn delete_entry(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    id: String,
) -> Result<(), String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard delete",
        move |clipboard| clipboard.delete(id),
    )
    .await
}

#[tauri::command]
pub async fn report_image_load_failed(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    id: String,
) -> Result<ImagePreviewRepairOutcome, String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard preview repair",
        move |clipboard| clipboard.repair_preview(id),
    )
    .await
}

#[tauri::command]
pub async fn clear_all(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
) -> Result<(), String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard clear",
        ClipboardEngineHandle::clear,
    )
    .await
}

#[tauri::command]
pub async fn get_active_dates(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    year_month: String,
) -> Result<Vec<String>, String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard active dates",
        move |clipboard| clipboard.get_active_dates(year_month),
    )
    .await
}

#[tauri::command]
pub async fn get_earliest_month(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
) -> Result<Option<String>, String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard earliest month",
        ClipboardEngineHandle::get_earliest_month,
    )
    .await
}

#[tauri::command]
pub async fn toggle_pin(
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    id: String,
) -> Result<(), String> {
    dispatch_clipboard_request(
        clipboard.inner().clone(),
        Arc::clone(i18n.inner()),
        "clipboard pin toggle",
        move |clipboard| clipboard.toggle_pin(id),
    )
    .await
}

#[tauri::command]
pub fn get_settings(store: State<'_, Arc<SettingsStore>>) -> Result<AppSettings, String> {
    svc::settings::get_settings(&store)
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    store: State<'_, Arc<SettingsStore>>,
    clipboard: State<'_, ClipboardEngineHandle>,
    i18n: State<'_, Arc<RwLock<I18n>>>,
    patch: AppSettingsPatch,
) -> Result<SaveSettingsResult, String> {
    let store = Arc::clone(store.inner());
    let clipboard = clipboard.inner().clone();
    let i18n = Arc::clone(i18n.inner());
    let result = dispatch_blocking(&i18n, "errSettingsPersist", "settings save", {
        let i18n = Arc::clone(&i18n);
        move || svc::settings::save_settings(&app, &store, &clipboard, &i18n, patch)
    })
    .await?;
    result
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

#[tauri::command]
pub fn pause_hotkey(app: tauri::AppHandle) -> Result<(), String> {
    crate::utils::hotkey::unregister_hotkey(&app)
}

#[tauri::command]
pub fn resume_hotkey(
    app: tauri::AppHandle,
    store: State<'_, Arc<SettingsStore>>,
) -> Result<(), String> {
    let hotkey = store.load_runtime_app_settings()?.hotkey;
    crate::utils::hotkey::register_hotkey(&app, &hotkey)
}
