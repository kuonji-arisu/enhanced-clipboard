use enhanced_clipboard_lib::models::{
    ArtifactRole, ClipboardArtifact, ClipboardContentType, ClipboardImagePreviewMode,
    ClipboardPreview, ClipboardTextPreviewMode,
};
use enhanced_clipboard_lib::services::projection::{
    project_entries_to_list_items, project_entry_to_list_item,
};

mod common;

use common::{
    image_artifact_records, image_entry, image_original_path, image_preview_path,
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
    assert!(pending_item.original_path.is_none());
    assert!(pending_item.preview_path.is_none());

    let ready = image_entry("image-2", 101);
    touch_file(&ctx, &image_original_path("image-2"));
    touch_file(&ctx, &image_preview_path("image-2"));
    let artifacts = image_artifact_records("image-2");
    let ready_item = project_entry_to_list_item(&ready, &artifacts, &ctx.data_dir, None);
    match ready_item.preview {
        ClipboardPreview::Image { mode } => {
            assert_eq!(mode, ClipboardImagePreviewMode::Ready);
        }
        ClipboardPreview::Text { .. } => panic!("expected image preview"),
    }
    assert!(ready_item
        .preview_path
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
    assert!(repairing_item.original_path.is_some());
    assert!(repairing_item.preview_path.is_none());
}

#[test]
fn image_projection_treats_missing_preview_file_as_repairing() {
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
    assert!(item.original_path.is_some());
    assert!(item.preview_path.is_none());
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
        },
        ClipboardArtifact {
            entry_id: ready.id.clone(),
            role: ArtifactRole::Preview,
            rel_path: "C:/outside.png".to_string(),
            mime_type: "image/png".to_string(),
        },
    ];

    let item = project_entry_to_list_item(&ready, &artifacts, &ctx.data_dir, None);

    assert!(matches!(
        item.preview,
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Repairing
        }
    ));
    assert!(item.original_path.is_none());
    assert!(item.preview_path.is_none());
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

#[test]
fn file_projection_uses_text_placeholder_preview() {
    let ctx = TestContext::new();
    let mut entry = text_entry("file-1", 20, "");
    entry.content_type = ClipboardContentType::File;

    let item = project_entry_to_list_item(&entry, &[], &ctx.data_dir, None);

    match item.preview {
        ClipboardPreview::Text { mode, text, .. } => {
            assert_eq!(mode, ClipboardTextPreviewMode::Prefix);
            assert_eq!(text, "File clipboard entry preview is not supported yet.");
        }
        ClipboardPreview::Image { .. } => panic!("expected file placeholder text preview"),
    }
    assert!(item.original_path.is_none());
    assert!(item.preview_path.is_none());
}
