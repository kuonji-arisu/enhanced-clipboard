use crate::db::PinToggleResult;
use crate::models::{ClipboardEntriesQuery, ClipboardEntryType, ClipboardQueryCursor};

use super::support::{
    image_entry, insert_entry, insert_entry_with_tags, local_date, local_month, pinned, text_entry,
    touch_file, TestContext,
};

#[test]
fn query_filters_respect_text_tags_and_cursor_ordering() {
    let ctx = TestContext::new();
    insert_entry_with_tags(&ctx, &pinned(text_entry("p1", 300, "Alpha pinned")), &["url"]);
    insert_entry_with_tags(&ctx, &text_entry("b", 200, "Alpha body"), &["url"]);
    insert_entry_with_tags(&ctx, &text_entry("a", 200, "Alpha older same second"), &["email"]);
    insert_entry(&ctx, &image_entry("img", 100));

    let query = ClipboardEntriesQuery {
        text: Some("alpha".to_string()),
        tag: Some("url".to_string()),
        ..ClipboardEntriesQuery::default()
    };
    let pinned_entries = ctx.db.get_pinned(&query).expect("get pinned");
    let normal_entries = ctx.db.get_normal_page(&query, 0).expect("get normals");

    assert_eq!(pinned_entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(), vec!["p1"]);
    assert_eq!(normal_entries.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(), vec!["b"]);

    let cursor_query = ClipboardEntriesQuery {
        text: Some("alpha".to_string()),
        entry_type: Some(ClipboardEntryType::Text),
        cursor: Some(ClipboardQueryCursor {
            created_at: 200,
            id: "b".to_string(),
        }),
        ..ClipboardEntriesQuery::default()
    };
    let second_page = ctx
        .db
        .get_normal_page(&cursor_query, 0)
        .expect("get second page");
    assert_eq!(second_page.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
}

#[test]
fn visible_date_queries_apply_ttl_to_non_pinned_but_keep_pinned_entries() {
    let ctx = TestContext::new();
    let old_ts = 1_700_000_000;
    let fresh_ts = 1_800_000_000;
    insert_entry(&ctx, &pinned(text_entry("pinned", old_ts, "Pinned")));
    insert_entry(&ctx, &text_entry("old", old_ts, "Expired"));
    insert_entry(&ctx, &text_entry("fresh", fresh_ts, "Fresh"));

    let window_start = fresh_ts - 10;
    let old_month = local_month(old_ts);
    let fresh_month = local_month(fresh_ts);

    assert_eq!(
        ctx.db.get_earliest_month(window_start).expect("earliest month"),
        Some(old_month.clone())
    );
    assert_eq!(
        ctx.db
            .get_active_dates_in_month(&old_month, window_start)
            .expect("old dates"),
        vec![local_date(old_ts)]
    );
    assert_eq!(
        ctx.db
            .get_active_dates_in_month(&fresh_month, window_start)
            .expect("fresh dates"),
        vec![local_date(fresh_ts)]
    );
}

#[test]
fn prune_removes_expired_entries_before_trimming_and_preserves_pinned_entries() {
    let ctx = TestContext::new();
    let expired = image_entry("expired", 10);
    touch_file(&ctx, expired.image_path.as_deref().expect("image path"));
    touch_file(&ctx, expired.thumbnail_path.as_deref().expect("thumbnail path"));
    insert_entry(&ctx, &expired);
    insert_entry(&ctx, &text_entry("newest", 40, "Newest"));
    insert_entry(&ctx, &text_entry("middle", 30, "Middle"));
    insert_entry(&ctx, &text_entry("oldest", 20, "Oldest"));
    insert_entry(&ctx, &pinned(text_entry("pinned", 1, "Pinned")));

    let (ids, paths) = ctx.db.prune(25, 2).expect("prune db");

    let mut ids = ids;
    ids.sort();
    assert_eq!(ids, vec!["expired".to_string(), "oldest".to_string()]);
    assert_eq!(paths.len(), 2);
    assert_eq!(ctx.db.count_normal().expect("count normals"), 2);
    assert!(ctx.db.get_entry_by_id("pinned").expect("pinned lookup").is_some());
}

#[test]
fn toggle_pin_limit_and_asset_deletion_contracts_are_enforced() {
    let ctx = TestContext::new();
    insert_entry(&ctx, &pinned(text_entry("p1", 3, "Pinned one")));
    insert_entry(&ctx, &pinned(text_entry("p2", 2, "Pinned two")));
    insert_entry(&ctx, &pinned(text_entry("p3", 1, "Pinned three")));
    insert_entry(&ctx, &text_entry("candidate", 4, "Candidate"));

    let toggle = ctx
        .db
        .toggle_pinned_with_limit("candidate", 3)
        .expect("toggle pin");
    assert!(matches!(toggle, PinToggleResult::LimitExceeded));

    let image = image_entry("image", 10);
    touch_file(&ctx, image.image_path.as_deref().expect("image path"));
    touch_file(&ctx, image.thumbnail_path.as_deref().expect("thumbnail path"));
    insert_entry(&ctx, &image);

    let deleted_paths = ctx
        .db
        .delete_entry_with_assets("image")
        .expect("delete entry")
        .expect("asset paths");
    assert_eq!(deleted_paths.len(), 2);
    assert!(ctx.db.get_entry_by_id("image").expect("deleted lookup").is_none());
}
