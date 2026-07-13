use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// Minimal Tauri event boundary used by runtime services and their integration
/// tests. Clipboard list invalidation is emitted directly by ClipboardEngine.
pub trait EventEmitter {
    fn emit_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String>;
}

impl<R: Runtime> EventEmitter for AppHandle<R> {
    fn emit_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String> {
        self.emit(event, payload).map_err(|error| error.to_string())
    }
}

impl<T: EventEmitter + ?Sized> EventEmitter for Arc<T> {
    fn emit_event<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String> {
        self.as_ref().emit_event(event, payload)
    }
}
