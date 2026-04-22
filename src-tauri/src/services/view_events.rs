use std::path::Path;

use tauri::{AppHandle, Emitter};

use crate::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_ADDED,
    EVENT_STREAM_ITEM_UPDATED,
};
use crate::models::ClipboardEntry;
use crate::services::projection::project_entry_to_list_item;

// View-facing event adapter.
//
// Business services call this after domain changes have already happened. The
// payloads here are shaped for UI list consumption, especially the default
// history stream. They are not intended to be a complete domain event log, and
// snapshot query views should treat them as stale signals rather than replaying
// every mutation as exact membership reconciliation.
pub fn emit_stream_item_added(
    app: &AppHandle,
    data_dir: &Path,
    entry: &ClipboardEntry,
) -> Result<(), String> {
    let item = project_entry_to_list_item(entry, data_dir, None);
    app.emit(EVENT_STREAM_ITEM_ADDED, item)
        .map_err(|e| e.to_string())
}

/// Emit an incremental update for the default stream view.
///
/// This means "the list item representation changed", not "every possible
/// domain subscriber has seen a canonical entry-updated event".
pub fn emit_stream_item_updated(
    app: &AppHandle,
    data_dir: &Path,
    entry: &ClipboardEntry,
) -> Result<(), String> {
    let item = project_entry_to_list_item(entry, data_dir, None);
    app.emit(EVENT_STREAM_ITEM_UPDATED, item)
        .map_err(|e| e.to_string())
}

pub fn emit_entries_removed(app: &AppHandle, ids: Vec<String>) -> Result<(), String> {
    app.emit(EVENT_ENTRIES_REMOVED, ids)
        .map_err(|e| e.to_string())
}

pub fn emit_query_results_stale(app: &AppHandle, reason: &str) -> Result<(), String> {
    app.emit(EVENT_QUERY_RESULTS_STALE, reason)
        .map_err(|e| e.to_string())
}

pub fn emit_entries_removed_and_mark_query_stale(
    app: &AppHandle,
    ids: Vec<String>,
    reason: &str,
) -> Result<(), String> {
    emit_entries_removed(app, ids)?;
    emit_query_results_stale(app, reason)
}
