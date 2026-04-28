use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::models::{
    ClipboardListItem, ClipboardPreview, ClipboardQueryStaleReason,
};
use enhanced_clipboard_lib::services::image_assets::{
    cleanup_relative_paths, repair_startup_image_assets, run_image_asset_maintenance,
    write_image_assets,
};
use image::GenericImageView;

mod common;

use common::{image_entry, insert_entry, touch_file, TestApp, TestContext};

fn rgba(width: usize, height: usize, value: u8) -> Vec<u8> {
    rgba_with_alpha(width, height, value, 255)
}

fn rgba_with_alpha(width: usize, height: usize, value: u8, alpha: u8) -> Vec<u8> {
    let mut rgba = vec![value; width * height * 4];
    for px in rgba.chunks_exact_mut(4) {
        px[3] = alpha;
    }
    rgba
}

fn make_old_file(ctx: &TestContext, rel_path: &str) {
    use filetime::{set_file_mtime, FileTime};

    let path = ctx.data_dir.join(rel_path);
    let old = FileTime::from_unix_time(1_600_000_000, 0);
    set_file_mtime(path, old).expect("set old mtime");
}

fn assert_display_size_at_most(ctx: &TestContext, rel_path: &str, max_w: u32, max_h: u32) {
    let img = image::open(ctx.data_dir.join(rel_path)).expect("open display image");
    let (width, height) = img.dimensions();
    assert!(width <= max_w, "display width {width} > {max_w}");
    assert!(height <= max_h, "display height {height} > {max_h}");
}

fn assert_display_has_alpha(ctx: &TestContext, rel_path: &str) {
    let img = image::open(ctx.data_dir.join(rel_path))
        .expect("open display image")
        .to_rgba8();
    assert!(img.as_raw().chunks_exact(4).any(|px| px[3] != 255));
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
    std::fs::remove_file(ctx.data_dir.join("thumbnails/small.png")).expect("remove thumb");

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
        Some("thumbnails/small.png")
    );
    assert_ne!(repaired.thumbnail_path, repaired.image_path);
    assert!(ctx.data_dir.join("thumbnails/small.png").exists());
    let updated = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, "small");
    assert!(updated[0].thumbnail_path.is_some());
    assert!(matches!(updated[0].preview, ClipboardPreview::Image { .. }));
}

#[test]
fn display_asset_policy_uses_png_for_small_transparent_images() {
    let ctx = TestContext::new();
    let outcome = write_image_assets(
        &ctx.data_dir,
        "small-alpha",
        &rgba_with_alpha(2, 2, 42, 128),
        2,
        2,
    )
    .expect("write assets");

    assert_eq!(outcome.rel_image, "images/small-alpha.png");
    assert_eq!(outcome.final_thumb_rel, "thumbnails/small-alpha.png");
    assert_ne!(outcome.rel_image, outcome.final_thumb_rel);
    assert!(!outcome.downscaled);
    assert_display_has_alpha(&ctx, &outcome.final_thumb_rel);
}

#[test]
fn display_asset_policy_uses_png_for_small_opaque_images() {
    let ctx = TestContext::new();
    let outcome = write_image_assets(&ctx.data_dir, "small-opaque", &rgba(2, 2, 255), 2, 2)
        .expect("write assets");

    assert_eq!(outcome.final_thumb_rel, "thumbnails/small-opaque.png");
    assert!(!outcome.downscaled);
}

#[test]
fn display_asset_policy_uses_png_and_downscales_large_transparent_images() {
    let ctx = TestContext::new();
    let outcome = write_image_assets(
        &ctx.data_dir,
        "large-alpha",
        &rgba_with_alpha(601, 301, 42, 128),
        601,
        301,
    )
    .expect("write assets");

    assert_eq!(outcome.final_thumb_rel, "thumbnails/large-alpha.png");
    assert!(outcome.downscaled);
    assert_display_size_at_most(&ctx, &outcome.final_thumb_rel, 600, 300);
    assert_display_has_alpha(&ctx, &outcome.final_thumb_rel);
}

#[test]
fn display_asset_policy_uses_jpeg_and_downscales_large_opaque_images() {
    let ctx = TestContext::new();
    let outcome = write_image_assets(
        &ctx.data_dir,
        "large-opaque",
        &rgba(601, 301, 255),
        601,
        301,
    )
    .expect("write assets");

    assert_eq!(outcome.final_thumb_rel, "thumbnails/large-opaque.jpg");
    assert!(outcome.downscaled);
    assert_display_size_at_most(&ctx, &outcome.final_thumb_rel, 600, 300);
}

#[test]
fn background_maintenance_removes_entries_with_broken_original_assets() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/broken.png");
    let mut entry = image_entry("broken", 10);
    entry.image_path = Some("images/broken.png".to_string());
    entry.thumbnail_path = None;
    insert_entry(&ctx, &entry);

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert!(report.rebuilt_thumbnails.is_empty());
    assert!(ctx.db.get_entry_by_id("broken").expect("lookup").is_none());
    assert!(!ctx.data_dir.join("images/broken.png").exists());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["broken".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );
}

#[test]
fn background_maintenance_keeps_entry_when_display_write_fails() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_image_assets(&ctx.data_dir, "retry", &rgba(2, 2, 255), 2, 2).expect("write assets");
    std::fs::remove_file(ctx.data_dir.join("thumbnails/retry.png")).expect("remove thumb");
    std::fs::create_dir(ctx.data_dir.join("thumbnails/retry.png")).expect("block display path");
    let mut entry = image_entry("retry", 10);
    entry.image_path = Some("images/retry.png".to_string());
    entry.thumbnail_path = None;
    insert_entry(&ctx, &entry);

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert!(report.rebuilt_thumbnails.is_empty());
    assert!(ctx.db.get_entry_by_id("retry").expect("lookup").is_some());
    assert!(app
        .captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED)
        .is_empty());
}

#[test]
fn background_maintenance_rereads_db_records_before_orphan_cleanup() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_image_assets(&ctx.data_dir, "latest", &rgba(2, 2, 255), 2, 2).expect("write assets");
    make_old_file(&ctx, "images/latest.png");
    make_old_file(&ctx, "thumbnails/latest.png");
    let mut entry = image_entry("latest", 10);
    entry.image_path = Some("images/latest.png".to_string());
    entry.thumbnail_path = Some("thumbnails/latest.png".to_string());
    insert_entry(&ctx, &entry);

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.orphan_files_removed, 0);
    assert!(ctx.data_dir.join("images/latest.png").exists());
    assert!(ctx.data_dir.join("thumbnails/latest.png").exists());
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
    touch_file(&ctx, "thumbnails/orphan.png");
    make_old_file(&ctx, "images/orphan.png");
    make_old_file(&ctx, "thumbnails/orphan.jpg");
    make_old_file(&ctx, "thumbnails/orphan.png");

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.orphan_files_removed, 3);
    assert!(!ctx.data_dir.join("images/orphan.png").exists());
    assert!(!ctx.data_dir.join("thumbnails/orphan.jpg").exists());
    assert!(!ctx.data_dir.join("thumbnails/orphan.png").exists());
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
    assert_eq!(outcome.final_thumb_rel, "thumbnails/valid.png");
    let mut entry = image_entry("valid", 10);
    entry.image_path = Some(outcome.rel_image);
    entry.thumbnail_path = Some(outcome.final_thumb_rel);
    insert_entry(&ctx, &entry);
    let jpeg_outcome =
        write_image_assets(&ctx.data_dir, "valid-jpeg", &rgba(601, 301, 128), 601, 301)
            .expect("write jpeg assets");
    assert_eq!(jpeg_outcome.final_thumb_rel, "thumbnails/valid-jpeg.jpg");
    let mut jpeg_entry = image_entry("valid-jpeg", 11);
    jpeg_entry.image_path = Some(jpeg_outcome.rel_image);
    jpeg_entry.thumbnail_path = Some(jpeg_outcome.final_thumb_rel);
    insert_entry(&ctx, &jpeg_entry);

    let report = run_image_asset_maintenance(&app, &ctx.db, &ctx.data_dir).expect("maintenance");

    assert_eq!(report.rebuilt_thumbnails, Vec::<String>::new());
    assert_eq!(report.orphan_files_removed, 0);
    assert!(ctx.db.get_entry_by_id("valid").expect("lookup").is_some());
    assert!(ctx
        .db
        .get_entry_by_id("valid-jpeg")
        .expect("lookup jpeg")
        .is_some());
}

#[test]
fn cleanup_relative_paths_rejects_unsafe_paths_and_removes_valid_assets() {
    let ctx = TestContext::new();
    let outside = ctx._tempdir.path().join("outside.png");
    std::fs::write(&outside, b"outside").expect("outside");
    touch_file(&ctx, "images/valid.png");
    touch_file(&ctx, "thumbnails/valid.png");
    touch_file(&ctx, "thumbnails/valid.jpg");
    touch_file(&ctx, "notes/keep.txt");

    cleanup_relative_paths(
        &ctx.data_dir,
        vec![
            "../outside.png".to_string(),
            outside.to_string_lossy().to_string(),
            "notes/keep.txt".to_string(),
            "images/valid.png".to_string(),
            "thumbnails/valid.png".to_string(),
            "thumbnails/valid.jpg".to_string(),
        ],
    );

    assert!(outside.exists());
    assert!(ctx.data_dir.join("notes/keep.txt").exists());
    assert!(!ctx.data_dir.join("images/valid.png").exists());
    assert!(!ctx.data_dir.join("thumbnails/valid.png").exists());
    assert!(!ctx.data_dir.join("thumbnails/valid.jpg").exists());
}
