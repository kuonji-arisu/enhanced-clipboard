use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::models::{
    ArtifactRole, ClipboardArtifactDraft, ClipboardListItem, ClipboardPreview,
    ClipboardQueryStaleReason,
};
use enhanced_clipboard_lib::services::artifacts::image as image_artifact_handler;
use enhanced_clipboard_lib::services::artifacts::maintenance::{
    run_artifact_maintenance_core, run_artifact_maintenance_once, run_startup_lightweight_repair,
    schedule_periodic_artifact_maintenance, ArtifactMaintenanceOptions,
};
use enhanced_clipboard_lib::services::artifacts::store::cleanup_relative_paths;
use image::GenericImageView;
use std::thread;
use std::time::{Duration, Instant};

mod common;

use common::{
    image_entry, insert_entry, pending_image_entry, text_entry, touch_file, TestApp, TestContext,
};

fn display_artifact(rel_path: &str) -> ClipboardArtifactDraft {
    ClipboardArtifactDraft {
        role: ArtifactRole::Display,
        rel_path: rel_path.to_string(),
        mime_type: if rel_path.ends_with(".jpg") {
            "image/jpeg"
        } else {
            "image/png"
        }
        .to_string(),
        width: None,
        height: None,
        byte_size: None,
    }
}

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

fn wait_until(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(condition(), "condition was not met before timeout");
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

#[derive(Debug)]
struct TestImageArtifactWriteOutcome {
    original_rel: String,
    display_rel: String,
    downscaled: bool,
}

fn write_test_image_artifacts(
    ctx: &TestContext,
    id: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> TestImageArtifactWriteOutcome {
    let outcome =
        image_artifact_handler::write_image_artifacts(&ctx.data_dir, id, rgba, width, height)
            .expect("write image artifacts");
    let original_rel = outcome
        .artifacts
        .iter()
        .find(|artifact| artifact.role == ArtifactRole::Original)
        .map(|artifact| artifact.rel_path.clone())
        .expect("original artifact");
    let display_rel = outcome
        .artifacts
        .iter()
        .find(|artifact| artifact.role == ArtifactRole::Display)
        .map(|artifact| artifact.rel_path.clone())
        .expect("display artifact");
    TestImageArtifactWriteOutcome {
        original_rel,
        display_rel,
        downscaled: outcome.downscaled,
    }
}

fn run_default_artifact_maintenance(
    app: &TestApp,
    ctx: &TestContext,
) -> enhanced_clipboard_lib::services::artifacts::maintenance::ArtifactMaintenanceSummary {
    run_artifact_maintenance_once(
        app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions::default(),
    )
    .expect("maintenance")
}

#[test]
fn startup_repair_removes_only_broken_image_rows_without_orphan_scan() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let broken = image_entry("broken", 10);
    insert_entry(&ctx, &broken);
    touch_file(&ctx, "images/orphan.png");
    touch_file(&ctx, "thumbnails/orphan.jpg");

    let report =
        run_startup_lightweight_repair(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, vec!["broken".to_string()]);
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

    let second =
        run_startup_lightweight_repair(&app, &ctx.db, &ctx.data_dir).expect("second repair");
    assert_eq!(second.removed_ids, Vec::<String>::new());
}

#[test]
fn startup_repair_removes_stale_pending_entries_from_previous_process() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_entry(&ctx, &pending_image_entry("pending", 10));
    touch_file(&ctx, "images/pending.png");
    touch_file(&ctx, "thumbnails/pending.png");
    touch_file(&ctx, "thumbnails/pending.jpg");

    let report =
        run_startup_lightweight_repair(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, vec!["pending".to_string()]);
    assert!(ctx
        .db
        .get_entry_by_id("pending")
        .expect("pending lookup")
        .is_none());
    wait_until(|| {
        !ctx.data_dir.join("images/pending.png").exists()
            && !ctx.data_dir.join("thumbnails/pending.png").exists()
            && !ctx.data_dir.join("thumbnails/pending.jpg").exists()
    });
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["pending".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );
}

#[test]
fn startup_repair_keeps_ready_image_when_only_display_is_missing() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_test_image_artifacts(&ctx, "small", &rgba(2, 2, 255), 2, 2);
    std::fs::remove_file(ctx.data_dir.join("thumbnails/small.png")).expect("remove display");

    let entry = image_entry("small", 10);
    insert_entry(&ctx, &entry);

    let report =
        run_startup_lightweight_repair(&app, &ctx.db, &ctx.data_dir).expect("startup repair");

    assert_eq!(report.removed_ids, Vec::<String>::new());
    assert!(ctx.db.get_entry_by_id("small").expect("lookup").is_some());
    assert!(ctx.data_dir.join("images/small.png").exists());
    assert!(!ctx.data_dir.join("thumbnails/small.png").exists());
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        Vec::<ClipboardQueryStaleReason>::new()
    );

    let maintenance = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions::default(),
    )
    .expect("maintenance");
    assert_eq!(maintenance.rebuilt_displays, vec!["small".to_string()]);
    assert!(ctx.data_dir.join("thumbnails/small.png").exists());
}

#[test]
fn background_maintenance_rebuilds_missing_display_without_deleting_entry() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_test_image_artifacts(&ctx, "small", &rgba(2, 2, 255), 2, 2);
    std::fs::remove_file(ctx.data_dir.join("thumbnails/small.png")).expect("remove display");

    let entry = image_entry("small", 10);
    insert_entry(&ctx, &entry);

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions::default(),
    )
    .expect("maintenance");

    assert_eq!(report.rebuilt_displays, vec!["small".to_string()]);
    let repaired = ctx.db.get_artifacts_for_entry("small").expect("artifacts");
    assert!(repaired
        .iter()
        .any(|artifact| artifact.rel_path == "thumbnails/small.png"));
    assert!(ctx.data_dir.join("thumbnails/small.png").exists());
    let updated = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, "small");
    assert!(updated[0].thumbnail_path.is_some());
    assert!(matches!(updated[0].preview, ClipboardPreview::Image { .. }));
}

#[test]
fn background_maintenance_replaces_stale_display_file_after_db_row_is_removed() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_test_image_artifacts(&ctx, "stale-display", &rgba(2, 2, 255), 2, 2);
    let entry = image_entry("stale-display", 10);
    insert_entry(&ctx, &entry);

    ctx.db
        .delete_artifact("stale-display", ArtifactRole::Display)
        .expect("remove display row");
    assert!(ctx.data_dir.join("thumbnails/stale-display.png").exists());

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions::default(),
    )
    .expect("maintenance");

    assert_eq!(report.rebuilt_displays, vec!["stale-display".to_string()]);
    let repaired = ctx
        .db
        .get_artifacts_for_entry("stale-display")
        .expect("artifacts");
    assert!(repaired
        .iter()
        .any(|artifact| artifact.rel_path == "thumbnails/stale-display.png"));
    assert!(ctx.data_dir.join("thumbnails/stale-display.png").exists());
}

#[test]
fn maintenance_core_returns_effects_without_emitting_events() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_test_image_artifacts(&ctx, "planned", &rgba(2, 2, 255), 2, 2);
    std::fs::remove_file(ctx.data_dir.join("thumbnails/planned.png")).expect("remove display");
    insert_entry(&ctx, &image_entry("planned", 10));

    let plan = run_artifact_maintenance_core(
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions { max_repairs: 8 },
    )
    .expect("plan maintenance");

    assert_eq!(plan.summary.rebuilt_displays, vec!["planned".to_string()]);
    assert_eq!(plan.effects.updated.len(), 1);
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
}

#[test]
fn maintenance_does_not_finalize_pending_ingest_entries() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_entry(&ctx, &pending_image_entry("pending", 10));

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions::default(),
    )
    .expect("maintenance");

    assert!(report.rebuilt_displays.is_empty());
    assert_eq!(
        ctx.db
            .get_entry_by_id("pending")
            .expect("lookup")
            .expect("pending exists")
            .status,
        enhanced_clipboard_lib::models::EntryStatus::Pending
    );
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
}

#[test]
fn periodic_maintenance_trigger_reuses_the_once_runner_boundary() {
    schedule_periodic_artifact_maintenance();
}

#[test]
fn display_asset_policy_uses_png_for_small_transparent_images() {
    let ctx = TestContext::new();
    let outcome =
        write_test_image_artifacts(&ctx, "small-alpha", &rgba_with_alpha(2, 2, 42, 128), 2, 2);

    assert_eq!(outcome.original_rel, "images/small-alpha.png");
    assert_eq!(outcome.display_rel, "thumbnails/small-alpha.png");
    assert_ne!(outcome.original_rel, outcome.display_rel);
    assert!(!outcome.downscaled);
    assert_display_has_alpha(&ctx, &outcome.display_rel);
}

#[test]
fn display_asset_policy_uses_png_for_small_opaque_images() {
    let ctx = TestContext::new();
    let outcome = write_test_image_artifacts(&ctx, "small-opaque", &rgba(2, 2, 255), 2, 2);

    assert_eq!(outcome.display_rel, "thumbnails/small-opaque.png");
    assert!(!outcome.downscaled);
}

#[test]
fn display_asset_policy_uses_png_and_downscales_large_transparent_images() {
    let ctx = TestContext::new();
    let outcome = write_test_image_artifacts(
        &ctx,
        "large-alpha",
        &rgba_with_alpha(601, 301, 42, 128),
        601,
        301,
    );

    assert_eq!(outcome.display_rel, "thumbnails/large-alpha.png");
    assert!(outcome.downscaled);
    assert_display_size_at_most(&ctx, &outcome.display_rel, 600, 300);
    assert_display_has_alpha(&ctx, &outcome.display_rel);
}

#[test]
fn display_asset_policy_uses_jpeg_and_downscales_large_opaque_images() {
    let ctx = TestContext::new();
    let outcome = write_test_image_artifacts(&ctx, "large-opaque", &rgba(601, 301, 255), 601, 301);

    assert_eq!(outcome.display_rel, "thumbnails/large-opaque.jpg");
    assert!(outcome.downscaled);
    assert_display_size_at_most(&ctx, &outcome.display_rel, 600, 300);
}

#[test]
fn display_artifact_metadata_records_display_dimensions() {
    let ctx = TestContext::new();
    let outcome = image_artifact_handler::write_image_artifacts(
        &ctx.data_dir,
        "metadata",
        &rgba(1200, 600, 255),
        1200,
        600,
    )
    .expect("write artifacts");

    let display = outcome
        .artifacts
        .iter()
        .find(|artifact| artifact.role == ArtifactRole::Display)
        .expect("display artifact");
    assert_eq!(display.width, Some(600));
    assert_eq!(display.height, Some(300));
}

#[test]
fn background_maintenance_removes_entries_with_broken_original_assets() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/broken.png");
    let entry = image_entry("broken", 10);
    insert_entry(&ctx, &entry);

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert!(report.rebuilt_displays.is_empty());
    assert!(ctx.db.get_entry_by_id("broken").expect("lookup").is_none());
    wait_until(|| !ctx.data_dir.join("images/broken.png").exists());
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
fn background_maintenance_removes_entry_when_original_is_missing_even_if_display_exists() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_test_image_artifacts(&ctx, "missing-original", &rgba(2, 2, 255), 2, 2);
    let entry = image_entry("missing-original", 10);
    insert_entry(&ctx, &entry);
    std::fs::remove_file(ctx.data_dir.join("images/missing-original.png"))
        .expect("remove original");

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert!(report.rebuilt_displays.is_empty());
    assert!(ctx
        .db
        .get_entry_by_id("missing-original")
        .expect("lookup")
        .is_none());
    wait_until(|| {
        !ctx.data_dir
            .join("thumbnails/missing-original.png")
            .exists()
    });
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["missing-original".to_string()]]
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
    write_test_image_artifacts(&ctx, "retry", &rgba(2, 2, 255), 2, 2);
    std::fs::remove_file(ctx.data_dir.join("thumbnails/retry.png")).expect("remove display");
    std::fs::create_dir(ctx.data_dir.join("thumbnails/retry.png")).expect("block display path");
    let entry = image_entry("retry", 10);
    insert_entry(&ctx, &entry);

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert!(report.rebuilt_displays.is_empty());
    assert!(ctx.db.get_entry_by_id("retry").expect("lookup").is_some());
    assert!(app
        .captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED)
        .is_empty());
}

#[test]
fn background_maintenance_rereads_db_records_before_orphan_cleanup() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_test_image_artifacts(&ctx, "latest", &rgba(2, 2, 255), 2, 2);
    make_old_file(&ctx, "images/latest.png");
    make_old_file(&ctx, "thumbnails/latest.png");
    let entry = image_entry("latest", 10);
    insert_entry(&ctx, &entry);

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert_eq!(report.orphan_files_removed, 0);
    assert!(ctx.data_dir.join("images/latest.png").exists());
    assert!(ctx.data_dir.join("thumbnails/latest.png").exists());
}

#[test]
fn background_maintenance_keeps_recent_orphan_files() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/recent-orphan.png");

    let report = run_default_artifact_maintenance(&app, &ctx);

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

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert_eq!(report.orphan_files_removed, 3);
    wait_until(|| {
        !ctx.data_dir.join("images/orphan.png").exists()
            && !ctx.data_dir.join("thumbnails/orphan.jpg").exists()
            && !ctx.data_dir.join("thumbnails/orphan.png").exists()
    });
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::SettingsOrStartup]
    );

    let second = run_default_artifact_maintenance(&app, &ctx);
    assert_eq!(second.rebuilt_displays, Vec::<String>::new());
    assert_eq!(second.orphan_files_removed, 0);
}

#[test]
fn background_maintenance_recursively_removes_old_nested_orphans() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "files/nested/orphan.bin");
    make_old_file(&ctx, "files/nested/orphan.bin");

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert_eq!(report.orphan_files_removed, 1);
    wait_until(|| !ctx.data_dir.join("files/nested/orphan.bin").exists());
}

#[test]
fn background_maintenance_keeps_any_db_referenced_artifact_path() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_entry(&ctx, &text_entry("text-with-artifact", 10, "Alpha"));
    touch_file(&ctx, "files/nested/referenced.bin");
    make_old_file(&ctx, "files/nested/referenced.bin");
    ctx.db
        .insert_artifacts(
            "text-with-artifact",
            &[ClipboardArtifactDraft {
                role: ArtifactRole::Original,
                rel_path: "files/nested/referenced.bin".to_string(),
                mime_type: "application/octet-stream".to_string(),
                width: None,
                height: None,
                byte_size: Some(5),
            }],
        )
        .expect("insert referenced artifact");

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert_eq!(report.orphan_files_removed, 0);
    assert!(ctx.data_dir.join("files/nested/referenced.bin").exists());
}

#[test]
fn maintenance_keeps_valid_assets() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let outcome = write_test_image_artifacts(&ctx, "valid", &rgba(2, 2, 128), 2, 2);
    assert_eq!(outcome.original_rel, "images/valid.png");
    assert_eq!(outcome.display_rel, "thumbnails/valid.png");
    let entry = image_entry("valid", 10);
    insert_entry(&ctx, &entry);
    let jpeg_outcome =
        write_test_image_artifacts(&ctx, "valid-jpeg", &rgba(601, 301, 128), 601, 301);
    assert_eq!(jpeg_outcome.display_rel, "thumbnails/valid-jpeg.jpg");
    let jpeg_entry = image_entry("valid-jpeg", 11);
    insert_entry(&ctx, &jpeg_entry);
    ctx.db
        .replace_artifact("valid-jpeg", &display_artifact(&jpeg_outcome.display_rel))
        .expect("replace jpeg display artifact");

    let report = run_default_artifact_maintenance(&app, &ctx);

    assert_eq!(report.rebuilt_displays, Vec::<String>::new());
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
