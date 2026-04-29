use enhanced_clipboard_lib::models::{
    ArtifactRole, ClipboardArtifact, ClipboardImagePreviewMode, ClipboardPreview,
    ClipboardTextPreviewMode,
};
use enhanced_clipboard_lib::services::projection::{
    project_entries_to_list_items, project_entry_to_list_item,
};

mod common;

use common::{
    image_artifact_records, image_display_path, image_entry, image_original_path,
    pending_image_entry, text_entry, text_preview_text, touch_file, TestContext,
};

#[test]
fn text_projection_uses_search_preview_for_query_text() {
    let ctx = TestContext::new();
    let entry = text_entry("text-1", 100, "Alpha Beta Gamma");

    let item = project_entry_to_list_item(&entry, &[], &ctx.data_dir, Some("beta"));

    assert_eq!(item.id, "text-1");
    assert_eq!(text_preview_text(&item.preview), "Alpha Beta Gamma");
    match item.preview {
        ClipboardPreview::Text {
            mode,
            highlight_ranges,
            ..
        } => {
            assert_eq!(mode, ClipboardTextPreviewMode::SearchSnippet);
            assert_eq!(highlight_ranges.len(), 1);
        }
        ClipboardPreview::Image { .. } => panic!("expected text preview"),
    }
}

#[test]
fn image_projection_uses_pending_and_ready_preview_modes() {
    let ctx = TestContext::new();
    let pending = pending_image_entry("image-1", 100);

    let pending_item = project_entry_to_list_item(&pending, &[], &ctx.data_dir, None);
    match pending_item.preview {
        ClipboardPreview::Image { mode } => {
            assert_eq!(mode, ClipboardImagePreviewMode::Pending);
        }
        ClipboardPreview::Text { .. } => panic!("expected image preview"),
    }
    assert!(pending_item.image_path.is_none());
    assert!(pending_item.thumbnail_path.is_none());

    let ready = image_entry("image-2", 101);
    touch_file(&ctx, &image_original_path("image-2"));
    touch_file(&ctx, &image_display_path("image-2"));
    let artifacts = image_artifact_records("image-2");
    let ready_item = project_entry_to_list_item(&ready, &artifacts, &ctx.data_dir, None);
    match ready_item.preview {
        ClipboardPreview::Image { mode } => {
            assert_eq!(mode, ClipboardImagePreviewMode::Ready);
        }
        ClipboardPreview::Text { .. } => panic!("expected image preview"),
    }
    assert!(ready_item
        .thumbnail_path
        .as_deref()
        .unwrap()
        .contains("/thumbnails/image-2.png"));

    let repairing = image_entry("image-3", 102);
    touch_file(&ctx, &image_original_path("image-3"));
    let original_only = image_artifact_records("image-3")
        .into_iter()
        .filter(|artifact| artifact.role == enhanced_clipboard_lib::models::ArtifactRole::Original)
        .collect::<Vec<_>>();
    let repairing_item =
        project_entry_to_list_item(&repairing, &original_only, &ctx.data_dir, None);
    match repairing_item.preview {
        ClipboardPreview::Image { mode } => {
            assert_eq!(mode, ClipboardImagePreviewMode::Repairing);
        }
        ClipboardPreview::Text { .. } => panic!("expected image preview"),
    }
    assert!(repairing_item.image_path.is_some());
    assert!(repairing_item.thumbnail_path.is_none());
}

#[test]
fn image_projection_treats_missing_display_file_as_repairing() {
    let ctx = TestContext::new();
    let ready = image_entry("image-missing-display", 101);
    touch_file(&ctx, &image_original_path("image-missing-display"));
    let artifacts = image_artifact_records("image-missing-display");

    let item = project_entry_to_list_item(&ready, &artifacts, &ctx.data_dir, None);

    assert!(matches!(
        item.preview,
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Repairing
        }
    ));
    assert!(item.image_path.is_some());
    assert!(item.thumbnail_path.is_none());
}

#[test]
fn image_projection_rejects_invalid_artifact_paths() {
    let ctx = TestContext::new();
    let ready = image_entry("image-invalid", 101);
    let artifacts = vec![
        ClipboardArtifact {
            entry_id: ready.id.clone(),
            role: ArtifactRole::Original,
            rel_path: "../outside.png".to_string(),
            mime_type: "image/png".to_string(),
            width: Some(2),
            height: Some(2),
            byte_size: Some(4),
        },
        ClipboardArtifact {
            entry_id: ready.id.clone(),
            role: ArtifactRole::Display,
            rel_path: "C:/outside.png".to_string(),
            mime_type: "image/png".to_string(),
            width: Some(2),
            height: Some(2),
            byte_size: Some(4),
        },
    ];

    let item = project_entry_to_list_item(&ready, &artifacts, &ctx.data_dir, None);

    assert!(matches!(
        item.preview,
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Repairing
        }
    ));
    assert!(item.image_path.is_none());
    assert!(item.thumbnail_path.is_none());
}

#[test]
fn batch_projection_preserves_input_order() {
    let ctx = TestContext::new();
    let entries = vec![text_entry("a", 10, "Alpha"), text_entry("b", 9, "Beta")];

    let items = project_entries_to_list_items(
        &entries,
        &std::collections::HashMap::new(),
        &ctx.data_dir,
        None,
    );

    assert_eq!(
        items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}
