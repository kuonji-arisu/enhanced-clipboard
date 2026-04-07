use std::path::Path;

use log::{debug, info};

use crate::constants::MAX_PINNED_ENTRIES;
use crate::db::Database;
use crate::i18n::I18n;
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
pub fn toggle_pin_entry(db: &Database, id: &str, tr: &I18n) -> Result<bool, String> {
    let entry = db
        .get_entry_by_id(id)?
        .ok_or_else(|| tr.t("errEntryNotFound"))?;
    let new_state = !entry.is_pinned;
    if new_state {
        let count = db.count_pinned()?;
        if count >= MAX_PINNED_ENTRIES {
            return Err(format!(
                "{}{}{}",
                tr.t("pinLimitPrefix"),
                MAX_PINNED_ENTRIES,
                tr.t("pinLimitUnit")
            ));
        }
    }
    db.set_pinned(id, new_state)?;
    info!("Updated pin state: id={}, pinned={}", id, new_state);
    Ok(new_state)
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
