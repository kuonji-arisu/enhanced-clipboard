use std::path::Path;

use crate::models::{
    ArtifactRole, ClipboardArtifact, ClipboardContentType, ClipboardEntry,
    ClipboardImagePreviewMode, ClipboardListItem, ClipboardPreview, EntryStatus,
};
use crate::services::artifacts::store;
use crate::services::search_preview::build_text_preview;
use crate::utils::string::path_to_url_str;

pub fn project_text_entry_to_list_item(
    entry: &ClipboardEntry,
    query_text: Option<&str>,
) -> ClipboardListItem {
    let preview = build_text_preview(&entry.content, query_text);
    ClipboardListItem {
        id: entry.id.clone(),
        content_type: entry.content_type,
        tags: entry.tags.clone(),
        created_at: entry.created_at,
        is_pinned: entry.is_pinned,
        source_app: entry.source_app.clone(),
        preview,
        original_path: None,
        preview_path: None,
    }
}

pub fn project_entry_to_list_item(
    entry: &ClipboardEntry,
    artifacts: &[ClipboardArtifact],
    data_dir: &Path,
    query_text: Option<&str>,
) -> ClipboardListItem {
    if entry.content_type == ClipboardContentType::Text {
        return project_text_entry_to_list_item(entry, query_text);
    }

    let original_path = artifacts
        .iter()
        .find(|artifact| artifact.role == ArtifactRole::Original)
        .map(|artifact| artifact.rel_path.as_str());
    let preview_rel_path = artifacts
        .iter()
        .find(|artifact| artifact.role == ArtifactRole::Preview)
        .map(|artifact| artifact.rel_path.as_str());
    let original_path = original_path.and_then(|path| existing_artifact_url(data_dir, path));
    let preview_path = preview_rel_path.and_then(|path| existing_artifact_url(data_dir, path));

    ClipboardListItem {
        id: entry.id.clone(),
        content_type: entry.content_type,
        tags: entry.tags.clone(),
        created_at: entry.created_at,
        is_pinned: entry.is_pinned,
        source_app: entry.source_app.clone(),
        preview: ClipboardPreview::Image {
            mode: match (entry.status, preview_path.is_some()) {
                (EntryStatus::Pending, _) => ClipboardImagePreviewMode::Pending,
                (EntryStatus::Ready, true) => ClipboardImagePreviewMode::Ready,
                (EntryStatus::Ready, false) => ClipboardImagePreviewMode::Repairing,
            },
        },
        original_path,
        preview_path,
    }
}

fn existing_artifact_url(data_dir: &Path, rel_path: &str) -> Option<String> {
    store::validate_relative_path(data_dir, rel_path)
        .filter(|path| path.is_file())
        .map(|path| path_to_url_str(&path))
}

pub fn project_entries_to_list_items(
    entries: &[ClipboardEntry],
    artifacts_by_entry: &std::collections::HashMap<String, Vec<ClipboardArtifact>>,
    data_dir: &Path,
    query_text: Option<&str>,
) -> Vec<ClipboardListItem> {
    entries
        .iter()
        .map(|entry| {
            let artifacts = artifacts_by_entry
                .get(&entry.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            project_entry_to_list_item(entry, artifacts, data_dir, query_text)
        })
        .collect()
}
