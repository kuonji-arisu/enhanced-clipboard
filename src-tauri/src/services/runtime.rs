use tauri::{AppHandle, Emitter};

use crate::constants::EVENT_RUNTIME_STATUS_UPDATED;
use crate::models::{RuntimeStatus, RuntimeStatusPatch, RuntimeStatusState};

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
    app: &AppHandle,
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

    app.emit(EVENT_RUNTIME_STATUS_UPDATED, changed_patch)
        .map_err(|e| e.to_string())?;

    Ok(snapshot)
}
