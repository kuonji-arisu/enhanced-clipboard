use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::models::{
    ClipboardListItem, ClipboardPreview, ClipboardQueryStaleReason,
};
use enhanced_clipboard_lib::services::image_assets::{
    repair_startup_image_assets, run_image_asset_maintenance, write_image_assets,
};

mod common;

use common::{image_entry, insert_entry, touch_file, TestApp, TestContext};

fn rgba(width: usize, height: usize, value: u8) -> Vec<u8> {
    vec![value; width * height * 4]
}

fn make_old_file(ctx: &TestContext, rel_path: &str) {
    use filetime::{set_file_mtime, FileTime};

    let path = ctx.data_dir.join(rel_path);
    let old = FileTime::from_unix_time(1_600_000_000, 0);
    set_file_mtime(path, old).expect("set old mtime");
}

#[test]
fn startup_repair_removes_only_broken_image_rows_without_orphan_scan() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let broken = image_entry("broken", 10);
    insert_entry(&ctx, &broken);
    touch_file(&ctx, "images/orphan.png");
    touch_file(&ctx, "thumbnails/orphan.jpg");

    let report = repair_startup_image_assets(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, vec!["broken".to_string()]);
    assert_eq!(report.cleared_thumbnail_ids, Vec::<String>::new());
    assert!(ctx
        .db
        .get_entry_by_id("broken")
        .expect("broken lookup")
        .is_none());
    assert!(ctx.data_dir.join("images/orphan.png").exists());
    assert!(ctx.data_dir.join("thumbnails/orphan.jpg").exists());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["broken".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );

    let second = repair_startup_image_assets(&app, &ctx.db, &ctx.data_dir).expect("second repair");
    assert_eq!(second.removed_ids, Vec::<String>::new());
    assert_eq!(second.cleared_thumbnail_ids, Vec::<String>::new());
}

#[test]
fn startup_repair_keeps_original_when_thumbnail_missing_and_clears_display_path() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/small.png");

    let mut entry = image_entry("small", 10);
    entry.image_path = Some("images/small.png".to_string());
    entry.thumbnail_path = Some("thumbnails/small.jpg".to_string());
    insert_entry(&ctx, &entry);

    let report = repair_startup_image_assets(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, Vec::<String>::new());
    assert_eq!(report.cleared_thumbnail_ids, vec!["small".to_string()]);
    let repaired = ctx
        .db
        .get_entry_by_id("small")
        .expect("lookup")
        .expect("entry remains");
    assert_eq!(repaired.image_path.as_deref(), Some("images/small.png"));
    assert_eq!(repaired.thumbnail_path, None);
    assert!(!ctx.data_dir.join("thumbnails/small.jpg").exists());
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        Vec::<ClipboardQueryStaleReason>::new()
    );
}

#[test]
fn background_maintenance_rebuilds_missing_thumbnail_without_deleting_entry() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_image_assets(&ctx.data_dir, "small", &rgba(2, 2, 255), 2, 2).expect("write original");
    std::fs::remove_file(ctx.data_dir.join("thumbnails/small.jpg")).expect("remove thumb");

    let mut entry = image_entry("small", 10);
    entry.image_path = Some("images/small.png".to_string());
    entry.thumbnail_path = None;
    insert_entry(&ctx, &entry);

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.rebuilt_thumbnails, vec!["small".to_string()]);
    let repaired = ctx
        .db
        .get_entry_by_id("small")
        .expect("lookup")
        .expect("entry remains");
    assert_eq!(
        repaired.thumbnail_path.as_deref(),
        Some("thumbnails/small.jpg")
    );
    assert_ne!(repaired.thumbnail_path, repaired.image_path);
    assert!(ctx.data_dir.join("thumbnails/small.jpg").exists());
    let updated = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, "small");
    assert!(updated[0].thumbnail_path.is_some());
    assert!(matches!(updated[0].preview, ClipboardPreview::Image { .. }));
}

#[test]
fn background_maintenance_rereads_db_records_before_orphan_cleanup() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/latest.png");
    make_old_file(&ctx, "images/latest.png");
    let mut entry = image_entry("latest", 10);
    entry.image_path = Some("images/latest.png".to_string());
    entry.thumbnail_path = None;
    insert_entry(&ctx, &entry);

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.orphan_files_removed, 0);
    assert!(ctx.data_dir.join("images/latest.png").exists());
}

#[test]
fn background_maintenance_keeps_recent_orphan_files() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/recent-orphan.png");

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.orphan_files_removed, 0);
    assert!(ctx.data_dir.join("images/recent-orphan.png").exists());
}

#[test]
fn background_maintenance_removes_old_orphans_and_is_idempotent() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/orphan.png");
    touch_file(&ctx, "thumbnails/orphan.jpg");
    make_old_file(&ctx, "images/orphan.png");
    make_old_file(&ctx, "thumbnails/orphan.jpg");

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.orphan_files_removed, 2);
    assert!(!ctx.data_dir.join("images/orphan.png").exists());
    assert!(!ctx.data_dir.join("thumbnails/orphan.jpg").exists());
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );

    let second =
        run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("second maintenance");
    assert_eq!(second.rebuilt_thumbnails, Vec::<String>::new());
    assert_eq!(second.orphan_files_removed, 0);
}

#[test]
fn maintenance_keeps_valid_assets() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let outcome =
        write_image_assets(&ctx.data_dir, "valid", &rgba(2, 2, 128), 2, 2).expect("write assets");
    assert_eq!(outcome.rel_image, "images/valid.png");
    assert_eq!(outcome.final_thumb_rel, "thumbnails/valid.jpg");
    assert_ne!(outcome.rel_image, outcome.final_thumb_rel);
    assert!(ctx.data_dir.join(&outcome.final_thumb_rel).exists());
    let mut entry = image_entry("valid", 10);
    entry.image_path = Some(outcome.rel_image);
    entry.thumbnail_path = Some(outcome.final_thumb_rel);
    insert_entry(&ctx, &entry);

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.rebuilt_thumbnails, Vec::<String>::new());
    assert_eq!(report.orphan_files_removed, 0);
    assert!(ctx.db.get_entry_by_id("valid").expect("lookup").is_some());
}
