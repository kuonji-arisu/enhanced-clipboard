use std::path::Path;

use log::{debug, info, warn};

use crate::constants::MAX_PINNED_ENTRIES;
use crate::db::{Database, PinToggleResult, SettingsStore};
use crate::i18n::I18n;
use crate::models::{ClipboardEntry, ClipboardQueryStaleReason};
use crate::services::entry_tags::attach_tags;
use crate::services::view_events::EventEmitter;
use crate::services::{prune, view_events};
use crate::utils::clipboard::{write_file_to_clipboard, write_text_to_clipboard};
use crate::watcher::ClipboardWatcher;

/// Write the selected entry back to the system clipboard.
pub fn copy_to_clipboard(
    db: &Database,
    watcher: &ClipboardWatcher,
    data_dir: &Path,
    id: &str,
    tr: &I18n,
) -> Result<(), String> {
    let entry = db
        .get_entry_by_id(id)?
        .ok_or_else(|| tr.t("errEntryNotFound"))?;

    match entry.content_type.as_str() {
        "text" => {
            watcher.begin_text_suppression(entry.content.clone());
            if let Err(err) = write_text_to_clipboard(&entry.content) {
                watcher.rollback_text_suppression(&entry.content);
                return Err(err);
            }
            debug!("Copied text entry back to clipboard: id={}", id);
        }
        "image" => {
            let img_rel = entry
                .image_path
                .as_deref()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| tr.t("errImagePathMissing"))?;
            let img_path = data_dir.join(img_rel);
            if !img_path.exists() {
                return Err(tr.t("errImageFileMissing"));
            }
            write_file_to_clipboard(&img_path)?;
            debug!("Copied image entry back to clipboard: id={}", id);
        }
        _ => return Err(tr.t("errUnknownType")),
    }
    Ok(())
}

pub fn handle_image_load_failed(db: &Database, data_dir: &Path, id: &str) -> Result<bool, String> {
    let Some(entry) = db.get_entry_by_id(id)? else {
        return Ok(false);
    };
    if entry.content_type != "image" {
        warn!("Ignoring image-load failure for non-image entry: {}", id);
        return Ok(false);
    }
    let removed = remove_entry(db, data_dir, id)?;
    if removed {
        info!("Removed broken image entry after frontend load failure: id={}", id);
    }
    Ok(removed)
}

/// Delete the target entry and any associated image assets.
pub fn remove_entry(db: &Database, data_dir: &Path, id: &str) -> Result<bool, String> {
    if let Some(paths) = db.delete_entry_with_assets(id)? {
        for path in paths {
            let _ = std::fs::remove_file(data_dir.join(path));
        }
        info!("Deleted entry: id={}", id);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Toggle pinned state for an entry, enforcing the max-pinned limit.
pub fn toggle_pin_entry(
    app: &impl EventEmitter,
    db: &Database,
    settings: &SettingsStore,
    data_dir: &Path,
    id: &str,
    tr: &I18n,
) -> Result<(), String> {
    let new_state = match db.toggle_pinned_with_limit(id, MAX_PINNED_ENTRIES)? {
        PinToggleResult::Updated(new_state) => new_state,
        PinToggleResult::NotFound => return Err(tr.t("errEntryNotFound")),
        PinToggleResult::LimitExceeded => {
            return Err(tr.t_fmt(
                "pinLimitMessage",
                &[("count", MAX_PINNED_ENTRIES.to_string())],
            ));
        }
    };
    info!("Updated pin state: id={}, pinned={}", id, new_state);

    let mut retention_stale_emitted = false;
    if !new_state {
        let settings = settings.load_runtime_app_settings()?;
        retention_stale_emitted = prune::prune(
            app,
            db,
            data_dir,
            settings.expiry_seconds,
            settings.max_history,
            ClipboardQueryStaleReason::UnpinRetention,
        )?;
    }

    match db.get_entry_by_id(id)? {
        Some(updated_entry) => emit_stream_item_updated(app, db, data_dir, updated_entry),
        None if new_state => warn!(
            "Entry disappeared before clipboard_stream_item_updated emit: {}",
            id
        ),
        None => {}
    }
    if !retention_stale_emitted {
        let _ = view_events::emit_query_results_stale(app, ClipboardQueryStaleReason::PinChanged);
    }
    Ok(())
}

fn emit_stream_item_updated(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    mut entry: ClipboardEntry,
) {
    if let Err(err) = attach_tags(db, std::slice::from_mut(&mut entry)) {
        warn!(
            "Failed to attach tags before clipboard_stream_item_updated emit for entry {}: {}",
            entry.id, err
        );
    }

    if let Err(err) = view_events::emit_stream_item_updated(app, data_dir, &entry) {
        warn!(
            "Failed to emit clipboard_stream_item_updated for entry {}: {}",
            entry.id, err
        );
    }
}

/// Remove every entry and all persisted image files.
pub fn clear_all_entries(db: &Database, data_dir: &Path) -> Result<Vec<String>, String> {
    let (ids, paths) = db.clear_all_with_assets()?;
    for path in paths {
        let _ = std::fs::remove_file(data_dir.join(path));
    }
    if !ids.is_empty() {
        info!("Cleared all entries: count={}", ids.len());
    }
    Ok(ids)
}
