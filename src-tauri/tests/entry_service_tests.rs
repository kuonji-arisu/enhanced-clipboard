use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::models::{
    AppSettings, ClipboardImagePreviewMode, ClipboardListItem, ClipboardPreview,
    ClipboardQueryStaleReason, SettingsField,
};
use enhanced_clipboard_lib::services::artifacts::image as image_artifact_handler;
use enhanced_clipboard_lib::services::artifacts::maintenance::ArtifactMaintenanceScheduler;
use enhanced_clipboard_lib::services::effects::{
    apply_pipeline_effects, apply_pipeline_effects_with_cleanup, InlineArtifactCleanup,
    PipelineEffects,
};
use enhanced_clipboard_lib::services::entry::{
    clear_all_entries, copy_to_clipboard_or_repair, handle_image_load_failed, remove_entry,
    report_image_load_failed, toggle_pin_entry, ImageLoadFailureOutcome,
};
use enhanced_clipboard_lib::services::prune;
use enhanced_clipboard_lib::services::view_events::EventEmitter;
use enhanced_clipboard_lib::watcher::ClipboardWatcher;
use serde::Serialize;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

mod common;

use common::{
    image_display_path, image_entry, image_original_path, insert_entry, test_i18n, text_entry,
    touch_file, TestApp, TestContext,
};

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

struct FailingEventApp;

impl EventEmitter for FailingEventApp {
    fn emit_event<S: Serialize + Clone>(&self, _event: &str, _payload: S) -> Result<(), String> {
        Err("emit failed".to_string())
    }
}

#[test]
fn remove_entry_and_clear_all_delete_associated_asset_files() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let first = image_entry("first", 10);
    let second = image_entry("second", 11);
    touch_file(&ctx, &image_original_path("first"));
    touch_file(&ctx, &image_display_path("first"));
    touch_file(&ctx, &image_original_path("second"));
    touch_file(&ctx, &image_display_path("second"));
    insert_entry(&ctx, &first);
    insert_entry(&ctx, &second);

    assert!(remove_entry(
        &app,
        &ctx.db,
        &ctx.data_dir,
        Some(&ctx.claims),
        "first",
        ClipboardQueryStaleReason::EntryRemoved
    )
    .expect("remove entry"));
    assert!(ctx
        .db
        .get_entry_by_id("first")
        .expect("first lookup")
        .is_none());
    wait_until(|| !ctx.data_dir.join(image_original_path("first")).exists());

    let cleared_ids =
        clear_all_entries(&app, &ctx.db, &ctx.data_dir, Some(&ctx.claims)).expect("clear all");
    assert_eq!(cleared_ids, vec!["second".to_string()]);
    assert!(ctx
        .db
        .get_entry_by_id("second")
        .expect("second lookup")
        .is_none());
    wait_until(|| !ctx.data_dir.join(image_display_path("second")).exists());
}

#[test]
fn image_load_failure_repairs_display_when_original_exists() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let image = image_entry("image", 10);
    touch_file(&ctx, &image_original_path("image"));
    touch_file(&ctx, &image_display_path("image"));
    insert_entry(&ctx, &image);
    insert_entry(&ctx, &text_entry("text", 11, "Alpha"));

    assert_eq!(
        handle_image_load_failed(&app, &ctx.db, &ctx.data_dir, "image").expect("image failure"),
        ImageLoadFailureOutcome::MarkedRepairing
    );
    assert!(ctx.data_dir.join(image_original_path("image")).exists());
    wait_until(|| !ctx.data_dir.join(image_display_path("image")).exists());
    assert!(ctx
        .db
        .get_entry_by_id("image")
        .expect("image lookup")
        .is_some());
    let updated = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(updated.len(), 1);
    assert!(updated[0].image_path.is_some());
    assert!(updated[0].thumbnail_path.is_none());
    assert!(matches!(
        updated[0].preview,
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Repairing,
            ..
        }
    ));
    assert_eq!(
        handle_image_load_failed(&app, &ctx.db, &ctx.data_dir, "image").expect("repeat failure"),
        ImageLoadFailureOutcome::MarkedRepairing
    );
    assert_eq!(
        handle_image_load_failed(&app, &ctx.db, &ctx.data_dir, "text").expect("text failure"),
        ImageLoadFailureOutcome::Unchanged
    );
}

#[test]
fn reported_image_load_failure_schedules_background_display_rebuild() {
    let common::TestContext {
        _tempdir,
        data_dir,
        db,
        settings: _settings,
        claims: _claims,
    } = TestContext::new();
    let db = Arc::new(db);
    let app = Arc::new(TestApp::new());
    let maintenance = ArtifactMaintenanceScheduler::new();
    let artifacts =
        image_artifact_handler::write_image_artifacts(&data_dir, "image", &[255; 16], 2, 2)
            .expect("write valid image artifacts")
            .artifacts;
    let entry = image_entry("image", 10);
    db.insert_entry(&entry).expect("insert image");
    db.insert_artifacts(&entry.id, &artifacts)
        .expect("insert artifacts");

    assert!(report_image_load_failed(
        app.clone(),
        db.clone(),
        data_dir.clone(),
        &maintenance,
        "image",
    )
    .expect("report image failure"));

    wait_until(|| {
        let updated = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
        let display_exists = db
            .get_artifacts_for_entry("image")
            .expect("artifacts")
            .iter()
            .any(|artifact| artifact.rel_path == "thumbnails/image.png")
            && data_dir.join("thumbnails/image.png").exists();
        updated.len() >= 2 && display_exists
    });

    let updated = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert!(matches!(
        updated[0].preview.clone(),
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Repairing,
            ..
        }
    ));
    assert!(matches!(
        updated.last().expect("ready update").preview.clone(),
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Ready,
            ..
        }
    ));
}

#[test]
fn image_load_failure_removes_entry_when_original_is_missing() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let image = image_entry("image", 10);
    touch_file(&ctx, &image_display_path("image"));
    insert_entry(&ctx, &image);

    assert_eq!(
        handle_image_load_failed(&app, &ctx.db, &ctx.data_dir, "image").expect("image failure"),
        ImageLoadFailureOutcome::Removed
    );
    assert!(ctx
        .db
        .get_entry_by_id("image")
        .expect("image lookup")
        .is_none());
}

#[test]
fn copy_ready_image_removes_entry_when_original_file_is_missing() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let watcher = ClipboardWatcher::new();
    let image = image_entry("image", 10);
    touch_file(&ctx, &image_original_path("image"));
    touch_file(&ctx, &image_display_path("image"));
    insert_entry(&ctx, &image);
    std::fs::remove_file(ctx.data_dir.join(image_original_path("image"))).expect("remove original");
    let i18n = test_i18n();
    let tr = i18n.read().expect("i18n");

    let err = copy_to_clipboard_or_repair(&app, &ctx.db, &watcher, &ctx.data_dir, "image", &tr)
        .expect_err("copy missing image original");

    assert_eq!(err, tr.t("errImageFileMissing"));
    assert!(ctx
        .db
        .get_entry_by_id("image")
        .expect("image lookup")
        .is_none());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["image".to_string()]]
    );
    wait_until(|| !ctx.data_dir.join(image_display_path("image")).exists());
}

#[test]
fn delete_report_and_clear_use_effects_even_when_events_fail() {
    let ctx = TestContext::new();
    let first = image_entry("delete-me", 10);
    let broken = image_entry("broken", 11);
    let clear = image_entry("clear-me", 12);
    for id in ["delete-me", "broken", "clear-me"] {
        touch_file(&ctx, &image_original_path(id));
        touch_file(&ctx, &image_display_path(id));
    }
    insert_entry(&ctx, &first);
    insert_entry(&ctx, &broken);
    insert_entry(&ctx, &clear);

    assert!(remove_entry(
        &FailingEventApp,
        &ctx.db,
        &ctx.data_dir,
        Some(&ctx.claims),
        "delete-me",
        ClipboardQueryStaleReason::EntryRemoved
    )
    .expect("delete with event failure"));
    assert_eq!(
        handle_image_load_failed(&FailingEventApp, &ctx.db, &ctx.data_dir, "broken")
            .expect("report with event failure"),
        ImageLoadFailureOutcome::MarkedRepairing
    );
    let cleared = clear_all_entries(&FailingEventApp, &ctx.db, &ctx.data_dir, Some(&ctx.claims))
        .expect("clear with event failure");

    assert_eq!(cleared, vec!["broken".to_string(), "clear-me".to_string()]);
    for id in ["delete-me", "clear-me"] {
        assert!(ctx.db.get_entry_by_id(id).expect("lookup").is_none());
    }
    assert!(ctx.db.get_entry_by_id("broken").expect("lookup").is_none());
    wait_until(|| {
        ["delete-me", "broken", "clear-me"].iter().all(|id| {
            !ctx.data_dir.join(image_original_path(id)).exists()
                && !ctx.data_dir.join(image_display_path(id)).exists()
        })
    });
}

#[test]
fn toggle_pin_event_failure_does_not_roll_back_database_update() {
    let ctx = TestContext::new();
    let i18n = test_i18n();
    insert_entry(
        &ctx,
        &text_entry("entry", chrono::Utc::now().timestamp(), "Alpha"),
    );

    toggle_pin_entry(
        &FailingEventApp,
        &ctx.db,
        &ctx.settings,
        &ctx.data_dir,
        "entry",
        &i18n.read().expect("i18n"),
    )
    .expect("toggle pin with event failure");

    assert!(
        ctx.db
            .get_entry_by_id("entry")
            .expect("lookup")
            .expect("entry")
            .is_pinned
    );
}

#[test]
fn prune_deletes_database_rows_first_then_cleans_assets() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let old = image_entry("old-image", 10);
    let fresh = image_entry("fresh-image", 20);
    touch_file(&ctx, &image_original_path("old-image"));
    touch_file(&ctx, &image_display_path("old-image"));
    touch_file(&ctx, &image_original_path("fresh-image"));
    touch_file(&ctx, &image_display_path("fresh-image"));
    insert_entry(&ctx, &old);
    insert_entry(&ctx, &fresh);

    prune::prune(
        &app,
        &ctx.db,
        &ctx.data_dir,
        0,
        1,
        ClipboardQueryStaleReason::BeforeInsert,
    )
    .expect("prune");

    assert!(ctx
        .db
        .get_entry_by_id("old-image")
        .expect("old lookup")
        .is_none());
    wait_until(|| {
        !ctx.data_dir.join(image_original_path("old-image")).exists()
            && !ctx.data_dir.join(image_display_path("old-image")).exists()
    });
    assert!(ctx
        .data_dir
        .join(image_original_path("fresh-image"))
        .exists());
}

#[test]
fn effects_cleanup_artifacts_even_when_event_emit_fails() {
    let ctx = TestContext::new();
    touch_file(&ctx, "images/leaked.png");

    let report = apply_pipeline_effects_with_cleanup(
        &FailingEventApp,
        &ctx.db,
        &ctx.data_dir,
        PipelineEffects {
            removed_ids: vec!["gone".to_string()],
            cleanup_paths: vec!["images/leaked.png".to_string()],
            stale_reason: Some(ClipboardQueryStaleReason::EntryRemoved),
            ..PipelineEffects::default()
        },
        &InlineArtifactCleanup,
    );

    assert!(report.has_event_errors());
    assert_eq!(report.cleanup_paths_scheduled, 1);
    assert!(!ctx.data_dir.join("images/leaked.png").exists());
}

#[test]
fn effects_cleanup_after_updated_emit_failure() {
    let ctx = TestContext::new();
    let entry = text_entry("entry", 10, "Alpha");
    insert_entry(&ctx, &entry);
    touch_file(&ctx, "images/update-cleanup.png");

    let report = apply_pipeline_effects_with_cleanup(
        &FailingEventApp,
        &ctx.db,
        &ctx.data_dir,
        PipelineEffects {
            updated: vec![entry],
            cleanup_paths: vec!["images/update-cleanup.png".to_string()],
            ..PipelineEffects::default()
        },
        &InlineArtifactCleanup,
    );

    assert!(report.has_event_errors());
    assert_eq!(report.cleanup_paths_scheduled, 1);
    assert!(!ctx.data_dir.join("images/update-cleanup.png").exists());
}

#[test]
fn effects_removed_entries_suppress_updated_payloads_for_same_id() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let entry = text_entry("entry", 10, "Alpha");

    let report = apply_pipeline_effects(
        &app,
        &ctx.db,
        &ctx.data_dir,
        PipelineEffects {
            updated: vec![entry],
            removed_ids: vec!["entry".to_string()],
            ..PipelineEffects::default()
        },
    );

    assert!(!report.has_event_errors());
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["entry".to_string()]]
    );
}

#[test]
fn effects_reread_database_and_skip_missing_updated_entries() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let entry = text_entry("entry", 10, "Alpha");
    insert_entry(&ctx, &entry);
    ctx.db.delete_entry("entry").expect("delete entry");

    let report = apply_pipeline_effects(
        &app,
        &ctx.db,
        &ctx.data_dir,
        PipelineEffects {
            updated: vec![entry],
            ..PipelineEffects::default()
        },
    );

    assert!(!report.has_event_errors());
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
}

#[test]
fn effects_added_emit_failure_reports_warning_without_rollback_semantics() {
    let ctx = TestContext::new();
    let entry = text_entry("entry", 10, "Alpha");
    insert_entry(&ctx, &entry);

    let report = apply_pipeline_effects(
        &FailingEventApp,
        &ctx.db,
        &ctx.data_dir,
        PipelineEffects {
            added: vec![entry],
            ..PipelineEffects::default()
        },
    );

    assert!(report.has_event_errors());
    assert!(ctx.db.get_entry_by_id("entry").expect("lookup").is_some());
}

#[test]
fn toggle_pin_emits_stream_update_and_pin_changed_when_entry_remains_visible() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let i18n = test_i18n();

    insert_entry(
        &ctx,
        &text_entry("entry", chrono::Utc::now().timestamp(), "Alpha"),
    );

    toggle_pin_entry(
        &app,
        &ctx.db,
        &ctx.settings,
        &ctx.data_dir,
        "entry",
        &i18n.read().expect("i18n"),
    )
    .expect("toggle pin");

    assert!(
        ctx.db
            .get_entry_by_id("entry")
            .expect("entry lookup")
            .unwrap()
            .is_pinned
    );
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        Vec::<Vec<String>>::new()
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::PinChanged]
    );
    let updated_payloads = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED);
    assert_eq!(updated_payloads.len(), 1);
    assert_eq!(updated_payloads[0].id, "entry");
    assert!(updated_payloads[0].is_pinned);
}

#[test]
fn toggle_pin_unpin_retention_emits_removed_entries_instead_of_stream_update() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let i18n = test_i18n();

    ctx.settings
        .save_app_settings_fields(
            &AppSettings {
                hotkey: "CmdOrCtrl+Shift+V".to_string(),
                autostart: false,
                max_history: 500,
                theme_mode: "light".to_string(),
                expiry_seconds: 1,
                capture_images: true,
                log_level: "error".to_string(),
            },
            &[
                SettingsField::Hotkey,
                SettingsField::Autostart,
                SettingsField::MaxHistory,
                SettingsField::ThemeMode,
                SettingsField::ExpirySeconds,
                SettingsField::CaptureImages,
                SettingsField::LogLevel,
            ],
        )
        .expect("seed settings");

    let mut entry = text_entry("pinned-old", 10, "Old pinned");
    entry.is_pinned = true;
    insert_entry(&ctx, &entry);

    toggle_pin_entry(
        &app,
        &ctx.db,
        &ctx.settings,
        &ctx.data_dir,
        "pinned-old",
        &i18n.read().expect("i18n"),
    )
    .expect("toggle pin");

    assert!(ctx
        .db
        .get_entry_by_id("pinned-old")
        .expect("entry lookup")
        .is_none());
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["pinned-old".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::UnpinRetention]
    );
}
