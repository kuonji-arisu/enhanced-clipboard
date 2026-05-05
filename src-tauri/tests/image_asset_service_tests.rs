use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::models::{
    ArtifactRole, ClipboardImagePreviewMode, ClipboardListItem, ClipboardPreview,
    ClipboardQueryStaleReason, EntryStatus,
};
use enhanced_clipboard_lib::services::artifacts::image as image_artifacts;
use enhanced_clipboard_lib::services::artifacts::maintenance::{
    run_artifact_maintenance_once, ArtifactMaintenanceOptions,
};

mod common;

use common::{
    image_entry, image_original_path, image_preview_path, insert_entry,
    insert_pending_image_with_job, touch_file, TestApp, TestContext,
};

fn write_valid_ready_image(ctx: &TestContext, id: &str) {
    let artifacts = image_artifacts::write_image_artifacts(
        &ctx.data_dir,
        id,
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
        2,
        2,
    )
    .expect("write image artifacts")
    .artifacts;
    let entry = image_entry(id, 10);
    ctx.db.insert_entry(&entry).expect("insert image entry");
    ctx.db
        .insert_artifacts(id, &artifacts)
        .expect("insert image artifacts");
}

#[test]
fn maintenance_skips_pending_images_and_active_jobs() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_pending_image_with_job(&ctx, "pending", 10);

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions { max_repairs: 10 },
    )
    .expect("maintenance");

    assert!(report.rebuilt_previews.is_empty());
    let entry = ctx
        .db
        .get_entry_by_id("pending")
        .expect("pending lookup")
        .expect("pending entry");
    assert_eq!(entry.status, EntryStatus::Pending);
    assert_eq!(
        ctx.db
            .get_active_image_ingest_jobs()
            .expect("active jobs")
            .len(),
        1
    );
}

#[test]
fn maintenance_removes_ready_image_when_original_is_missing() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_entry(&ctx, &image_entry("missing-original", 10));
    touch_file(&ctx, &image_preview_path("missing-original"));

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions { max_repairs: 10 },
    )
    .expect("maintenance");

    assert!(report.rebuilt_previews.is_empty());
    assert!(ctx
        .db
        .get_entry_by_id("missing-original")
        .expect("entry lookup")
        .is_none());
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
fn maintenance_rebuilds_missing_ready_image_preview() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_valid_ready_image(&ctx, "missing-preview");
    std::fs::remove_file(ctx.data_dir.join(image_preview_path("missing-preview")))
        .expect("remove preview");

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions { max_repairs: 10 },
    )
    .expect("maintenance");

    assert_eq!(report.rebuilt_previews, vec!["missing-preview".to_string()]);
    assert!(ctx
        .data_dir
        .join(image_preview_path("missing-preview"))
        .exists());
    let updated = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(updated.len(), 1);
    assert!(updated[0].preview_path.is_some());
    assert!(matches!(
        updated[0].preview,
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Ready
        }
    ));
}

#[test]
fn maintenance_rebuilds_broken_ready_image_preview() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    write_valid_ready_image(&ctx, "broken-preview");
    std::fs::write(
        ctx.data_dir.join(image_preview_path("broken-preview")),
        b"not an image",
    )
    .expect("break preview");

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions { max_repairs: 10 },
    )
    .expect("maintenance");

    assert_eq!(report.rebuilt_previews, vec!["broken-preview".to_string()]);
    assert!(::image::open(ctx.data_dir.join(image_preview_path("broken-preview"))).is_ok());
}

#[test]
fn maintenance_does_not_scan_unreferenced_managed_files() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    touch_file(&ctx, "images/orphan.png");
    touch_file(&ctx, "staging/image_ingest/orphan.rgba");

    let report = run_artifact_maintenance_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        ArtifactMaintenanceOptions { max_repairs: 10 },
    )
    .expect("maintenance");

    assert!(report.rebuilt_previews.is_empty());
    assert!(ctx.data_dir.join("images/orphan.png").exists());
    assert!(ctx
        .data_dir
        .join("staging/image_ingest/orphan.rgba")
        .exists());
}

#[test]
fn image_artifact_roles_are_generic_original_and_preview() {
    let ctx = TestContext::new();
    let outcome =
        image_artifacts::write_image_artifacts(&ctx.data_dir, "roles", &[0, 0, 0, 255], 1, 1)
            .expect("write image")
            .artifacts;

    assert!(outcome
        .iter()
        .any(|artifact| artifact.role == ArtifactRole::Original));
    assert!(outcome
        .iter()
        .any(|artifact| artifact.role == ArtifactRole::Preview));
    assert!(!outcome
        .iter()
        .any(|artifact| artifact.rel_path == image_original_path("roles")
            && artifact.role == ArtifactRole::Preview));
}
