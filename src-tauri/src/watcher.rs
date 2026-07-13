use std::sync::mpsc::TrySendError;
use std::sync::Arc;
use std::thread;

use arboard::{Clipboard, Error as ClipboardError};
use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use log::{error, info, warn};
use tauri::AppHandle;

use crate::clipboard::{CapturedPayload, ClipboardEngineHandle};
use crate::models::{RuntimeStatusPatch, RuntimeStatusState};
use crate::services;
use crate::utils::os::get_foreground_process_name;

fn report_capture_available(
    app_handle: &AppHandle,
    runtime_status: &Arc<RuntimeStatusState>,
    available: bool,
) {
    if let Err(error) = services::runtime::apply_patch(
        app_handle,
        runtime_status,
        RuntimeStatusPatch {
            clipboard_capture_available: Some(available),
            ..RuntimeStatusPatch::default()
        },
    ) {
        error!("Failed to update clipboard capture availability: {error}");
    }
}

fn sample_clipboard(
    clipboard: &mut Clipboard,
    source_app: String,
) -> Result<Option<CapturedPayload>, ClipboardError> {
    match clipboard.get_text() {
        Ok(content) if !content.is_empty() => {
            return Ok(Some(CapturedPayload::Text {
                content,
                source_app,
            }));
        }
        Ok(_) | Err(ClipboardError::ContentNotAvailable) => {}
        Err(error) => return Err(error),
    }

    match clipboard.get_image() {
        Ok(image) => Ok(Some(CapturedPayload::Image {
            rgba: image.bytes.into_owned(),
            width: image.width as u32,
            height: image.height as u32,
            source_app,
        })),
        Err(ClipboardError::ContentNotAvailable) => Ok(None),
        Err(error) => Err(error),
    }
}

fn log_mailbox_error<T>(error: TrySendError<T>, operation: &str) {
    match error {
        TrySendError::Full(_) => {
            warn!("Clipboard engine mailbox is full; dropping {operation}")
        }
        TrySendError::Disconnected(_) => {
            error!("Clipboard engine mailbox is closed; dropping {operation}")
        }
    }
}

/// Windows clipboard listener. All mutable clipboard-domain state belongs to
/// `ClipboardEngine`; this type only owns the OS listener thread.
#[derive(Default)]
pub struct ClipboardWatcher;

pub struct WatcherStartContext {
    pub app_handle: AppHandle,
    pub engine: ClipboardEngineHandle,
    pub runtime_status: Arc<RuntimeStatusState>,
}

impl ClipboardWatcher {
    pub fn new() -> Self {
        Self
    }

    pub fn start(&self, context: WatcherStartContext) {
        let WatcherStartContext {
            app_handle,
            engine,
            runtime_status,
        } = context;
        let thread_app_handle = app_handle.clone();
        let thread_runtime_status = runtime_status.clone();

        let spawn_result = thread::Builder::new()
            .name("clipboard-listener".to_string())
            .spawn(move || {
                let mut clipboard = match Clipboard::new() {
                    Ok(clipboard) => clipboard,
                    Err(error) => {
                        error!("Failed to initialize clipboard listener: {error}");
                        report_capture_available(&thread_app_handle, &thread_runtime_status, false);
                        return;
                    }
                };

                match sample_clipboard(&mut clipboard, get_foreground_process_name()) {
                    Ok(Some(payload)) => {
                        report_capture_available(&thread_app_handle, &thread_runtime_status, true);
                        if let Err(error) = engine.prime(payload) {
                            log_mailbox_error(error, "initial clipboard payload");
                        }
                    }
                    Ok(None) => {
                        report_capture_available(&thread_app_handle, &thread_runtime_status, true)
                    }
                    Err(error) => {
                        error!("Failed to read initial clipboard content: {error}");
                        report_capture_available(&thread_app_handle, &thread_runtime_status, false);
                    }
                }

                info!("Clipboard listener started");
                let handler = WatcherHandler {
                    clipboard,
                    app_handle: thread_app_handle.clone(),
                    engine,
                    runtime_status: thread_runtime_status.clone(),
                };

                if let Err(error) = Master::new(handler).run() {
                    error!("Clipboard listener exited: {error}");
                    report_capture_available(&thread_app_handle, &thread_runtime_status, false);
                }
            });

        if let Err(error) = spawn_result {
            error!("Failed to start clipboard listener thread: {error}");
            report_capture_available(&app_handle, &runtime_status, false);
        }
    }
}

struct WatcherHandler {
    clipboard: Clipboard,
    app_handle: AppHandle,
    engine: ClipboardEngineHandle,
    runtime_status: Arc<RuntimeStatusState>,
}

impl ClipboardHandler for WatcherHandler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        let source_app = get_foreground_process_name();
        match sample_clipboard(&mut self.clipboard, source_app) {
            Ok(Some(payload)) => {
                report_capture_available(&self.app_handle, &self.runtime_status, true);
                if let Err(error) = self.engine.try_capture(payload) {
                    log_mailbox_error(error, "clipboard capture payload");
                }
            }
            Ok(None) => {
                report_capture_available(&self.app_handle, &self.runtime_status, true);
                if let Err(error) = self.engine.try_observe_no_capture() {
                    log_mailbox_error(error, "clipboard no-capture observation");
                }
            }
            Err(error) => {
                error!("Failed to read clipboard content: {error}");
                report_capture_available(&self.app_handle, &self.runtime_status, false);
            }
        }

        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, error: std::io::Error) -> CallbackResult {
        error!("Clipboard listener error: {error}");
        report_capture_available(&self.app_handle, &self.runtime_status, false);
        CallbackResult::Next
    }
}
