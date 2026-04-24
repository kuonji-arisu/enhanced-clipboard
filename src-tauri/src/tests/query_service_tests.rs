use crate::models::{ClipboardEntriesQuery, ClipboardPreview};
use crate::services::query::{get_list_item_by_id, get_normal_list_page, get_pinned_list_items};

use super::support::{insert_entry, insert_entry_with_tags, pinned, text_entry, TestContext};

#[test]
fn query_services_attach_tags_project_previews_and_apply_ttl_visibility() {
    let ctx = TestContext::new();
    let pinned_entry = pinned(text_entry("pinned", 10, "Alpha pinned"));
    let fresh_entry = text_entry("fresh", 100, "Alpha fresh");
    let expired_entry = text_entry("expired", 10, "Alpha expired");

    insert_entry_with_tags(&ctx, &pinned_entry, &["url"]);
    insert_entry_with_tags(&ctx, &fresh_entry, &["email"]);
    insert_entry(&ctx, &expired_entry);

    let text_query = ClipboardEntriesQuery {
        text: Some("alpha".to_string()),
        ..ClipboardEntriesQuery::default()
    };

    let pinned_items = get_pinned_list_items(&ctx.db, &ctx.data_dir, &text_query).expect("pinned items");
    assert_eq!(pinned_items[0].tags, vec!["url".to_string()]);
    match &pinned_items[0].preview {
        ClipboardPreview::Text { text, .. } => assert!(text.contains("Alpha")),
        ClipboardPreview::Image { .. } => panic!("expected text preview"),
    }

    let normal_items = get_normal_list_page(&ctx.db, &ctx.data_dir, &text_query, 50).expect("normal page");
    assert_eq!(normal_items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), vec!["fresh"]);
    assert_eq!(normal_items[0].tags, vec!["email".to_string()]);

    assert!(get_list_item_by_id(&ctx.db, &ctx.data_dir, "expired", &ClipboardEntriesQuery::default(), 50)
        .expect("expired item")
        .is_none());
    assert!(get_list_item_by_id(&ctx.db, &ctx.data_dir, "pinned", &ClipboardEntriesQuery::default(), 50)
        .expect("pinned item")
        .is_some());
}
