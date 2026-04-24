use crate::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_ADDED,
    EVENT_STREAM_ITEM_UPDATED,
};
use crate::models::{ClipboardPreview, ClipboardQueryStaleReason};
use crate::services::view_events::{
    emit_entries_removed, emit_entries_removed_and_mark_query_stale, emit_stream_item_added,
    emit_stream_item_updated,
};

use super::support::{image_entry, text_entry, TestApp, TestContext};

#[test]
fn view_events_emit_projected_stream_payloads_and_typed_stale_reasons() {
    let ctx = TestContext::new();
    let app = TestApp::new();

    let text = text_entry("text", 100, "Alpha Beta");
    let image = image_entry("image", 101);

    emit_stream_item_added(&app, &ctx.data_dir, &text).expect("emit text added");
    emit_stream_item_updated(&app, &ctx.data_dir, &image).expect("emit image updated");
    emit_entries_removed(&app, vec!["gone".to_string()]).expect("emit removed");
    emit_entries_removed_and_mark_query_stale(
        &app,
        vec!["stale".to_string()],
        ClipboardQueryStaleReason::EntryRemoved,
    )
    .expect("emit removed and stale");

    let added_payloads = app.captured_event::<crate::models::ClipboardListItem>(EVENT_STREAM_ITEM_ADDED);
    let updated_payloads =
        app.captured_event::<crate::models::ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(added_payloads[0].id, "text");
    match &added_payloads[0].preview {
        ClipboardPreview::Text { text, .. } => assert_eq!(text, "Alpha Beta"),
        ClipboardPreview::Image { .. } => panic!("expected text preview"),
    }
    assert_eq!(updated_payloads[0].id, "image");
    match &updated_payloads[0].preview {
        ClipboardPreview::Image { .. } => {}
        ClipboardPreview::Text { .. } => panic!("expected image preview"),
    }
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["gone".to_string()], vec!["stale".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::EntryRemoved]
    );
}
