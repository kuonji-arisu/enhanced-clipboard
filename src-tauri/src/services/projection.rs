use std::path::Path;

use crate::models::{ClipboardEntry, ClipboardListItem, ClipboardPreviewKind};
use crate::services::search_preview::build_text_preview;
use crate::utils::string::path_to_url_str;

pub fn project_text_entry_to_list_item(
    entry: &ClipboardEntry,
    search_text: Option<&str>,
) -> ClipboardListItem {
    let preview = build_text_preview(&entry.content, search_text);
    ClipboardListItem {
        id: entry.id.clone(),
        content_type: entry.content_type.clone(),
        tags: entry.tags.clone(),
        created_at: entry.created_at,
        is_pinned: entry.is_pinned,
        source_app: entry.source_app.clone(),
        preview_text: preview.text,
        preview_kind: preview.kind,
        match_ranges: preview.match_ranges,
        image_path: None,
        thumbnail_path: None,
    }
}

pub fn project_entry_to_list_item(
    entry: &ClipboardEntry,
    data_dir: &Path,
    search_text: Option<&str>,
) -> ClipboardListItem {
    if entry.content_type == "text" {
        return project_text_entry_to_list_item(entry, search_text);
    }

    ClipboardListItem {
        id: entry.id.clone(),
        content_type: entry.content_type.clone(),
        tags: entry.tags.clone(),
        created_at: entry.created_at,
        is_pinned: entry.is_pinned,
        source_app: entry.source_app.clone(),
        preview_text: String::new(),
        preview_kind: if entry.thumbnail_path.is_some() {
            ClipboardPreviewKind::ImageReady
        } else {
            ClipboardPreviewKind::ImagePending
        },
        match_ranges: Vec::new(),
        image_path: entry
            .image_path
            .as_deref()
            .map(|p| path_to_url_str(&data_dir.join(p))),
        thumbnail_path: entry
            .thumbnail_path
            .as_deref()
            .map(|p| path_to_url_str(&data_dir.join(p))),
    }
}

pub fn project_entries_to_list_items(
    entries: &[ClipboardEntry],
    data_dir: &Path,
    search_text: Option<&str>,
) -> Vec<ClipboardListItem> {
    entries
        .iter()
        .map(|entry| project_entry_to_list_item(entry, data_dir, search_text))
        .collect()
}
