use std::path::Path;

use log::{debug, info, warn};
use tauri::{AppHandle, Emitter};

use crate::constants::{EVENT_ENTRY_UPDATED, MAX_PINNED_ENTRIES};
use crate::db::{Database, SettingsStore};
use crate::i18n::I18n;
use crate::models::ClipboardEntry;
use crate::services::entry_tags::attach_tags;
use crate::services::{prune, query};
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
            watcher.suppress_text(entry.content.clone());
            write_text_to_clipboard(&entry.content)?;
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
    app: &AppHandle,
    db: &Database,
    settings: &SettingsStore,
    data_dir: &Path,
    id: &str,
    tr: &I18n,
) -> Result<(), String> {
    let entry = db
        .get_entry_by_id(id)?
        .ok_or_else(|| tr.t("errEntryNotFound"))?;
    let new_state = !entry.is_pinned;
    if new_state {
        let count = db.count_pinned()?;
        if count >= MAX_PINNED_ENTRIES {
            return Err(tr.t_fmt(
                "pinLimitMessage",
                &[("count", MAX_PINNED_ENTRIES.to_string())],
            ));
        }
    }
    db.set_pinned(id, new_state)?;
    info!("Updated pin state: id={}, pinned={}", id, new_state);

    if !new_state {
        let settings = settings.load_runtime_app_settings()?;
        prune::prune(
            app,
            db,
            data_dir,
            settings.expiry_seconds,
            settings.max_history,
            "unpin_retention",
        )?;
    }

    match db.get_entry_by_id(id)? {
        Some(updated_entry) => emit_entry_updated(app, db, data_dir, updated_entry),
        None if new_state => warn!("Entry disappeared before entry_updated emit: {}", id),
        None => {}
    }
    Ok(())
}

fn emit_entry_updated(app: &AppHandle, db: &Database, data_dir: &Path, mut entry: ClipboardEntry) {
    if let Err(err) = attach_tags(db, std::slice::from_mut(&mut entry)) {
        warn!(
            "Failed to attach tags before entry_updated emit for entry {}: {}",
            entry.id, err
        );
    }

    query::post_process_entry(&mut entry, data_dir);
    if let Err(err) = app.emit(EVENT_ENTRY_UPDATED, &entry) {
        warn!(
            "Failed to emit entry_updated for entry {}: {}",
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
