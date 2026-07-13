use crate::constants::EVENT_RUNTIME_STATUS_UPDATED;
use crate::models::{RuntimeStatus, RuntimeStatusPatch, RuntimeStatusState};
use crate::services::view_events::EventEmitter;
use log::warn;
use tauri::{AppHandle, Manager, Theme};

pub fn initial_status() -> RuntimeStatus {
    RuntimeStatus::default()
}

/// 统一 merge 运行时 patch。
/// 后续新增动态字段时，只需要同时扩展 RuntimeStatus / RuntimeStatusPatch
/// 并在这里补充字段合并即可，事件流和前端 store 无需重做。
fn merge_runtime_patch(
    status: &mut RuntimeStatus,
    patch: RuntimeStatusPatch,
) -> RuntimeStatusPatch {
    let mut changed = RuntimeStatusPatch::default();

    if let Some(available) = patch.clipboard_capture_available {
        if status.clipboard_capture_available != available {
            status.clipboard_capture_available = available;
            changed.clipboard_capture_available = Some(available);
        }
    }

    if let Some(system_theme) = patch.system_theme {
        if status.system_theme != system_theme {
            status.system_theme = system_theme.clone();
            changed.system_theme = Some(system_theme);
        }
    }

    changed
}

pub fn get_runtime_status(state: &RuntimeStatusState) -> Result<RuntimeStatus, String> {
    state
        .0
        .lock()
        .map(|status| status.clone())
        .map_err(|e| e.to_string())
}

pub fn apply_patch(
    app: &impl EventEmitter,
    state: &RuntimeStatusState,
    patch: RuntimeStatusPatch,
) -> Result<RuntimeStatus, String> {
    let (snapshot, changed_patch) = {
        let mut status = state.0.lock().map_err(|e| e.to_string())?;
        let changed_patch = merge_runtime_patch(&mut status, patch);
        if changed_patch.is_empty() {
            return Ok(status.clone());
        }
        (status.clone(), changed_patch)
    };

    app.emit_event(EVENT_RUNTIME_STATUS_UPDATED, changed_patch)?;

    Ok(snapshot)
}

/// Publish the current Windows theme as a runtime fact. Theme intent remains in
/// saved settings; this helper only reports the live system value.
pub fn report_system_theme(
    app: &AppHandle,
    state: &RuntimeStatusState,
    theme: Theme,
) -> Result<RuntimeStatus, String> {
    let system_theme = match theme {
        Theme::Dark => "dark",
        _ => "light",
    };
    apply_patch(
        app,
        state,
        RuntimeStatusPatch {
            system_theme: Some(system_theme.to_string()),
            ..RuntimeStatusPatch::default()
        },
    )
}

pub fn initialize_system_theme(app: &AppHandle, state: &RuntimeStatusState) {
    let Some(window) = app.get_webview_window(crate::constants::MAIN_WINDOW_LABEL) else {
        warn!("Main window not found while initializing system theme");
        return;
    };
    match window.theme() {
        Ok(theme) => {
            if let Err(err) = report_system_theme(app, state, theme) {
                warn!("Failed to publish initial system theme: {err}");
            }
        }
        Err(err) => warn!("Failed to read initial system theme: {err}"),
    }
}
