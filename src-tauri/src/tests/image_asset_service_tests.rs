use crate::constants::{EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE};
use crate::models::ClipboardQueryStaleReason;
use crate::services::image_assets::{repair_startup_image_assets, write_image_assets};

use super::support::{image_entry, insert_entry, touch_file, TestApp, TestContext};

fn rgba(width: usize, height: usize, value: u8) -> Vec<u8> {
    vec![value; width * height * 4]
}

#[test]
fn startup_repair_removes_broken_image_rows_and_orphan_files_idempotently() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let broken = image_entry("broken", 10);
    insert_entry(&ctx, &broken);
    touch_file(&ctx, "images/orphan.png");
    touch_file(&ctx, "thumbnails/orphan.jpg");

    let report = repair_startup_image_assets(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, vec!["broken".to_string()]);
    assert_eq!(report.orphan_files_removed, 2);
    assert!(ctx
        .db
        .get_entry_by_id("broken")
        .expect("broken lookup")
        .is_none());
    assert!(!ctx.data_dir.join("images/orphan.png").exists());
    assert!(!ctx.data_dir.join("thumbnails/orphan.jpg").exists());
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
    assert_eq!(second.rebuilt_thumbnails, Vec::<String>::new());
    assert_eq!(second.orphan_files_removed, 0);
}

#[test]
fn startup_repair_rebuilds_missing_thumbnail_for_small_original() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_image_assets(&ctx.data_dir, "small", &rgba(2, 2, 255), 2, 2).expect("write original");
    std::fs::remove_file(ctx.data_dir.join("thumbnails/small.jpg")).expect("remove thumb");

    let mut entry = image_entry("small", 10);
    entry.image_path = Some("images/small.png".to_string());
    entry.thumbnail_path = Some("thumbnails/small.jpg".to_string());
    insert_entry(&ctx, &entry);

    let report = repair_startup_image_assets(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, Vec::<String>::new());
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
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );
}

#[test]
fn startup_repair_keeps_valid_assets_and_removes_unreferenced_files() {
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
    touch_file(&ctx, "images/orphan.png");

    let report = repair_startup_image_assets(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, Vec::<String>::new());
    assert_eq!(report.rebuilt_thumbnails, Vec::<String>::new());
    assert_eq!(report.orphan_files_removed, 1);
    assert!(ctx.db.get_entry_by_id("valid").expect("lookup").is_some());
    assert!(!ctx.data_dir.join("images/orphan.png").exists());
}
