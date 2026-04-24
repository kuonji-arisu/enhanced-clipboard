use crate::models::{ClipboardImagePreviewMode, ClipboardPreview, ClipboardTextPreviewMode};
use crate::services::projection::{project_entries_to_list_items, project_entry_to_list_item};

use super::support::{image_entry, text_entry, text_preview_text, TestContext};

#[test]
fn text_projection_uses_search_preview_for_query_text() {
    let ctx = TestContext::new();
    let entry = text_entry("text-1", 100, "Alpha Beta Gamma");

    let item = project_entry_to_list_item(&entry, &ctx.data_dir, Some("beta"));

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
    let mut pending = image_entry("image-1", 100);
    pending.thumbnail_path = None;

    let pending_item = project_entry_to_list_item(&pending, &ctx.data_dir, None);
    match pending_item.preview {
        ClipboardPreview::Image { mode } => {
            assert_eq!(mode, ClipboardImagePreviewMode::Pending);
        }
        ClipboardPreview::Text { .. } => panic!("expected image preview"),
    }
    assert!(pending_item.image_path.as_deref().unwrap().contains("/images/image-1.png"));
    assert!(pending_item.thumbnail_path.is_none());

    let ready = image_entry("image-2", 101);
    let ready_item = project_entry_to_list_item(&ready, &ctx.data_dir, None);
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
}

#[test]
fn batch_projection_preserves_input_order() {
    let ctx = TestContext::new();
    let entries = vec![text_entry("a", 10, "Alpha"), text_entry("b", 9, "Beta")];

    let items = project_entries_to_list_items(&entries, &ctx.data_dir, None);

    assert_eq!(items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
}
