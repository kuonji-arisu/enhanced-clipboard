use enhanced_clipboard_lib::db::{JobFinalizeOutcome, PinToggleResult};
use enhanced_clipboard_lib::models::{
    ArtifactRole, ClipboardEntriesQuery, ClipboardEntryType, ClipboardQueryCursor, EntryStatus,
};

mod common;

use common::{
    finalize_pending_image, image_display_path, image_entry, image_original_path, insert_entry,
    insert_entry_with_tags, insert_pending_image_with_job, local_date, local_month, pinned,
    text_entry, touch_file, TestContext,
};

#[test]
fn query_filters_respect_text_tags_and_cursor_ordering() {
    let ctx = TestContext::new();
    insert_entry_with_tags(
        &ctx,
        &pinned(text_entry("p1", 300, "Alpha pinned")),
        &["url"],
    );
    insert_entry_with_tags(&ctx, &text_entry("b", 200, "Alpha body"), &["url"]);
    insert_entry_with_tags(
        &ctx,
        &text_entry("a", 200, "Alpha older same second"),
        &["email"],
    );
    insert_entry(&ctx, &image_entry("img", 100));

    let query = ClipboardEntriesQuery {
        text: Some("alpha".to_string()),
        tag: Some("url".to_string()),
        ..ClipboardEntriesQuery::default()
    };
    let pinned_entries = ctx.db.get_pinned(&query).expect("get pinned");
    let normal_entries = ctx.db.get_normal_page(&query, 0).expect("get normals");

    assert_eq!(
        pinned_entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["p1"]
    );
    assert_eq!(
        normal_entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b"]
    );

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
    assert_eq!(
        second_page
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );
}

#[test]
fn query_filters_keep_pinned_matches_strict_and_escape_like_wildcards() {
    let ctx = TestContext::new();
    insert_entry_with_tags(
        &ctx,
        &pinned(text_entry("pinned-url", 400, "Literal 100%_\\ marker")),
        &["url"],
    );
    insert_entry_with_tags(
        &ctx,
        &pinned(text_entry("pinned-email", 300, "Alpha pinned email")),
        &["email"],
    );
    insert_entry_with_tags(
        &ctx,
        &text_entry("normal-url", 200, "Literal 100%_\\ body"),
        &["url"],
    );
    insert_entry(&ctx, &image_entry("image", 100));

    let literal_query = ClipboardEntriesQuery {
        text: Some("%_\\".to_string()),
        tag: Some("url".to_string()),
        ..ClipboardEntriesQuery::default()
    };
    assert_eq!(
        ctx.db
            .get_pinned(&literal_query)
            .expect("literal pinned")
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["pinned-url"]
    );
    assert_eq!(
        ctx.db
            .get_normal_page(&literal_query, 0)
            .expect("literal normal")
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["normal-url"]
    );

    let image_query = ClipboardEntriesQuery {
        text: Some("alpha".to_string()),
        entry_type: Some(ClipboardEntryType::Image),
        ..ClipboardEntriesQuery::default()
    };
    assert!(ctx
        .db
        .get_pinned(&image_query)
        .expect("image pinned")
        .is_empty());
    assert!(ctx
        .db
        .get_normal_page(&image_query, 0)
        .expect("image normal")
        .is_empty());
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
        ctx.db
            .get_earliest_month(window_start)
            .expect("earliest month"),
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
fn image_ingest_finalize_does_not_resurrect_deleted_placeholder() {
    let ctx = TestContext::new();
    insert_pending_image_with_job(&ctx, "pending-image", 10);
    let running = ctx
        .db
        .claim_next_image_ingest_job()
        .expect("claim image job")
        .expect("job");

    let deleted_paths = ctx
        .db
        .delete_entry_with_assets("pending-image")
        .expect("delete placeholder")
        .expect("placeholder existed");
    assert!(deleted_paths.is_empty());

    let finalized = ctx
        .db
        .finalize_running_image_ingest_job(&running.id, &common::image_artifacts("pending-image"))
        .expect("finalize deleted placeholder");
    assert!(matches!(finalized, JobFinalizeOutcome::Skipped));
    assert!(ctx
        .db
        .get_entry_by_id("pending-image")
        .expect("lookup deleted placeholder")
        .is_none());
}

#[test]
fn prune_removes_expired_entries_before_trimming_and_preserves_pinned_entries() {
    let ctx = TestContext::new();
    let expired = image_entry("expired", 10);
    touch_file(&ctx, &image_original_path("expired"));
    touch_file(&ctx, &image_display_path("expired"));
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
    assert!(ctx
        .db
        .get_entry_by_id("pinned")
        .expect("pinned lookup")
        .is_some());
}

#[test]
fn pending_images_do_not_participate_in_retention_prune() {
    let ctx = TestContext::new();
    insert_pending_image_with_job(&ctx, "first", 10);
    insert_pending_image_with_job(&ctx, "second", 20);

    finalize_pending_image(&ctx, "first").expect("first remains");
    let (ids, _) = ctx.db.prune(0, 1).expect("prune after first");
    assert!(ids.is_empty());
    assert!(ctx
        .db
        .get_entry_by_id("second")
        .expect("second lookup")
        .is_some());

    finalize_pending_image(&ctx, "second").expect("second remains");
    let (ids, _) = ctx.db.prune(0, 1).expect("prune after second");
    assert_eq!(ids, vec!["first".to_string()]);
    assert!(ctx
        .db
        .get_entry_by_id("first")
        .expect("first lookup")
        .is_none());
    assert!(ctx
        .db
        .get_entry_by_id("second")
        .expect("second lookup")
        .is_some());
}

#[test]
fn out_of_order_image_finalize_prunes_by_created_at_not_finalize_order() {
    let ctx = TestContext::new();
    insert_pending_image_with_job(&ctx, "older", 10);
    insert_pending_image_with_job(&ctx, "newer", 20);

    finalize_pending_image(&ctx, "newer").expect("newer remains");
    let (ids, _) = ctx.db.prune(0, 1).expect("prune after newer");
    assert!(ids.is_empty());

    finalize_pending_image(&ctx, "older").expect("older remains before prune");
    let (ids, _) = ctx.db.prune(0, 1).expect("prune after older");

    assert_eq!(ids, vec!["older".to_string()]);
    assert!(ctx
        .db
        .get_entry_by_id("older")
        .expect("older lookup")
        .is_none());
    assert!(ctx
        .db
        .get_entry_by_id("newer")
        .expect("newer lookup")
        .is_some());
}

#[test]
fn image_finalize_after_newer_text_does_not_delete_newer_text() {
    let ctx = TestContext::new();
    insert_pending_image_with_job(&ctx, "image", 10);
    insert_entry(&ctx, &text_entry("text", 20, "Newer text"));

    finalize_pending_image(&ctx, "image").expect("image remains before prune");
    let (ids, _) = ctx.db.prune(0, 1).expect("prune after image");

    assert_eq!(ids, vec!["image".to_string()]);
    assert!(ctx
        .db
        .get_entry_by_id("image")
        .expect("image lookup")
        .is_none());
    assert!(ctx
        .db
        .get_entry_by_id("text")
        .expect("text lookup")
        .is_some());
}

#[test]
fn text_entries_still_participate_in_retention_prune() {
    let ctx = TestContext::new();
    insert_entry(&ctx, &text_entry("old", 10, "Old"));
    insert_entry(&ctx, &text_entry("new", 20, "New"));

    let (ids, _) = ctx.db.prune(0, 1).expect("prune text");

    assert_eq!(ids, vec!["old".to_string()]);
    assert!(ctx.db.get_entry_by_id("old").expect("old lookup").is_none());
    assert!(ctx.db.get_entry_by_id("new").expect("new lookup").is_some());
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
    touch_file(&ctx, &image_original_path("image"));
    touch_file(&ctx, &image_display_path("image"));
    insert_entry(&ctx, &image);

    let deleted_paths = ctx
        .db
        .delete_entry_with_assets("image")
        .expect("delete entry")
        .expect("asset paths");
    assert_eq!(deleted_paths.len(), 2);
    assert!(ctx
        .db
        .get_entry_by_id("image")
        .expect("deleted lookup")
        .is_none());
}

#[test]
fn unknown_status_and_role_are_rejected_instead_of_silently_becoming_ready() {
    let ctx = TestContext::new();
    assert!(EntryStatus::from_db("bogus").is_err());
    assert!(ArtifactRole::from_db("preview").is_err());

    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(
        "PRAGMA key = \"x'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\";",
    )
    .expect("apply key");
    let status_result = conn.execute(
        "INSERT INTO clipboard_entries
         (id, content_type, status, content, canonical_search_text, created_at, is_pinned, source_app)
         VALUES ('bad-status', 'text', 'failed', '', '', 1, 0, '')",
        [],
    );
    assert!(status_result.is_err());

    insert_entry(&ctx, &text_entry("entry", 10, "Alpha"));
    let role_result = conn.execute(
        "INSERT INTO clipboard_entry_artifacts
         (entry_id, role, rel_path, mime_type)
         VALUES ('entry', 'preview', 'previews/entry.png', 'image/png')",
        [],
    );
    assert!(role_result.is_err());
}
