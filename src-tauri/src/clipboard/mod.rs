//! Single-writer clipboard core.
//!
//! The public handle is the only way application code can access clipboard
//! persistence.  The SQLite connection and managed artifact directories are
//! owned by the engine thread.

mod artifacts;
mod engine;
mod read_model;
mod repository;

pub use crate::constants::EVENT_CLIPBOARD_CHANGED;
pub use engine::{
    ClipboardEngine, ClipboardEngineConfig, ClipboardEngineHandle, ClipboardError, ClipboardPolicy,
    ClipboardWriter, SystemClipboardWriter,
};
pub use read_model::{
    CapturedPayload, ClipboardChanged, ClipboardContentType, ClipboardEntriesQuery,
    ClipboardListItem, ClipboardListPage, ClipboardPreview, ClipboardQueryCursor,
    ClipboardTextPreviewMode, ImagePreviewRepairOutcome, TextRange,
};
