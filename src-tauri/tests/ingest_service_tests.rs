use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use arboard::ImageData;
use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_STREAM_ITEM_ADDED, EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::db::image_ingest_jobs::ImageIngestJobDraft;
use enhanced_clipboard_lib::db::SettingsStore;
use enhanced_clipboard_lib::models::{
    ClipboardImagePreviewMode, ClipboardJobKind, ClipboardJobStatus, ClipboardListItem,
    ClipboardPreview, ClipboardQueryStaleReason, EntryStatus,
};
use enhanced_clipboard_lib::services::image_ingest::{
    self, staging, CaptureImageDeps, MAX_ACTIVE_IMAGE_INGEST_JOBS, MAX_ACTIVE_IMAGE_STAGING_BYTES,
    MAX_IMAGE_INGEST_ATTEMPTS,
};
use enhanced_clipboard_lib::services::ingest::{accept_image_clipboard_change, ImageIngestDeps};
use enhanced_clipboard_lib::services::jobs::{
    clear_polling_image_dedup_if_current, ContentJobWorker, ImageDedupState,
};
use enhanced_clipboard_lib::services::pipeline;
use enhanced_clipboard_lib::utils::image::hash_image_content;
use uuid::Uuid;

mod common;

use common::{
    image_display_path, image_original_path, insert_entry, pending_image_entry, text_entry,
    TestApp, TestContext,
};

const TEST_DB_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn open_raw_clipboard_conn(ctx: &TestContext) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(&format!("PRAGMA key = \"x'{TEST_DB_KEY}'\";"))
        .expect("unlock db");
    conn
}

fn image_data(width: usize, height: usize, bytes: Vec<u8>) -> ImageData<'static> {
    ImageData {
        width,
        height,
        bytes: Cow::Owned(bytes),
    }
}

fn solid_image(width: usize, height: usize, value: u8) -> ImageData<'static> {
    image_data(width, height, vec![value; width * height * 4])
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

fn make_old_file(ctx: &TestContext, rel_path: &str) {
    use filetime::{set_file_mtime, FileTime};

    let path = ctx.data_dir.join(rel_path);
    let old = FileTime::from_unix_time(1_600_000_000, 0);
    set_file_mtime(path, old).expect("set old mtime");
}

fn start_test_worker(
    app: Arc<TestApp>,
    db: Arc<enhanced_clipboard_lib::db::Database>,
    data_dir: std::path::PathBuf,
    image_dedup: Arc<Mutex<ImageDedupState>>,
) -> ContentJobWorker {
    ContentJobWorker::start(
        app,
        db,
        Arc::new(
            SettingsStore::new(data_dir.join("settings.db").to_string_lossy().as_ref())
                .expect("settings store"),
        ),
        data_dir,
        image_dedup,
    )
}

fn image_job_draft(ctx: &TestContext, entry_id: &str, img: &ImageData<'_>) -> ImageIngestJobDraft {
    let job_id = Uuid::new_v4().to_string();
    let input_ref = staging::input_rel_path(&job_id);
    let width = img.width as u32;
    let height = img.height as u32;
    let byte_size =
        staging::write_rgba8(&ctx.data_dir, &input_ref, img.bytes.as_ref(), width, height)
            .expect("write staging") as i64;
    let content_hash = hash_image_content(img);
    ImageIngestJobDraft {
        id: job_id,
        entry_id: entry_id.to_string(),
        input_ref,
        dedup_key: content_hash.clone(),
        created_at: 10,
        width: i64::from(width),
        height: i64::from(height),
        pixel_format: staging::PIXEL_FORMAT_RGBA8.to_string(),
        byte_size,
        content_hash,
    }
}

fn insert_pending_job(
    ctx: &TestContext,
    app: &TestApp,
    entry_id: &str,
    img: &ImageData<'_>,
) -> ImageIngestJobDraft {
    let entry = pending_image_entry(entry_id, 10);
    let job = image_job_draft(ctx, entry_id, img);
    ctx.db
        .insert_pending_image_entry_with_job(
            &entry,
            &job,
            MAX_ACTIVE_IMAGE_INGEST_JOBS,
            MAX_ACTIVE_IMAGE_STAGING_BYTES,
        )
        .expect("insert pending job");
    pipeline::emit_pending_entry_added(app, &ctx.db, &ctx.data_dir, &entry).expect("emit pending");
    job
}

fn mark_image_job_terminal(
    ctx: &TestContext,
    entry_id: &str,
    job_id: &str,
    status: ClipboardJobStatus,
) {
    let conn = open_raw_clipboard_conn(ctx);
    conn.execute(
        "UPDATE clipboard_entries SET status = 'ready' WHERE id = ?1",
        [entry_id],
    )
    .expect("mark entry ready");
    conn.execute(
        "UPDATE clipboard_jobs SET status = ?1 WHERE id = ?2",
        [status.as_str(), job_id],
    )
    .expect("mark job terminal");
}

fn run_next_job(
    ctx: &TestContext,
    app: &TestApp,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
    max_history: u32,
) -> Option<()> {
    match image_ingest::run_next_job(app, &ctx.db, &ctx.data_dir, 0, max_history, image_dedup) {
        Ok(true) | Err(_) => Some(()),
        Ok(false) => None,
    }
}

#[test]
fn sweeper_keeps_fresh_unreferenced_staging_inputs_inside_protection_window() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let orphan = staging::input_rel_path(&Uuid::new_v4().to_string());
    staging::ensure_dirs(&ctx.data_dir).expect("staging dirs");
    std::fs::write(ctx.data_dir.join(&orphan), b"fresh orphan").expect("fresh orphan");

    let summary = image_ingest::sweeper::run_once(
        &app,
        &ctx.db,
        &ctx.data_dir,
        enhanced_clipboard_lib::services::artifacts::store::ORPHAN_FILE_PROTECTION_WINDOW,
    )
    .expect("sweep");

    assert_eq!(summary.cleanup_paths, 0);
    assert!(ctx.data_dir.join(orphan).exists());
}

#[test]
fn sweeper_removes_old_unreferenced_staging_inputs() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let orphan = staging::input_rel_path(&Uuid::new_v4().to_string());
    staging::ensure_dirs(&ctx.data_dir).expect("staging dirs");
    std::fs::write(ctx.data_dir.join(&orphan), b"old orphan").expect("old orphan");
    make_old_file(&ctx, &orphan);

    let summary = image_ingest::sweeper::run_once(&app, &ctx.db, &ctx.data_dir, Duration::ZERO)
        .expect("sweep");

    assert_eq!(summary.cleanup_paths, 1);
    wait_until(|| !ctx.data_dir.join(orphan.clone()).exists());
}

#[test]
fn sweeper_keeps_active_job_referenced_staging_inputs() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(2, 2, 64);
    let job = insert_pending_job(&ctx, &app, "pending", &img);
    make_old_file(&ctx, &job.input_ref);

    let summary = image_ingest::sweeper::run_once(&app, &ctx.db, &ctx.data_dir, Duration::ZERO)
        .expect("sweep");

    assert!(summary.removed_ids.is_empty());
    assert_eq!(summary.cleanup_paths, 0);
    assert!(ctx.data_dir.join(job.input_ref).exists());
    assert!(ctx.db.get_entry_by_id("pending").expect("entry").is_some());
}

#[test]
fn delayed_startup_sweep_runs_image_ingest_sweeper() {
    let common::TestContext {
        _tempdir,
        data_dir,
        db,
        settings: _settings,
        claims: _claims,
    } = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(db);
    let orphan = staging::input_rel_path(&Uuid::new_v4().to_string());
    staging::ensure_dirs(&data_dir).expect("staging dirs");
    std::fs::write(data_dir.join(&orphan), b"old orphan").expect("old orphan");
    {
        use filetime::{set_file_mtime, FileTime};
        set_file_mtime(
            data_dir.join(&orphan),
            FileTime::from_unix_time(1_600_000_000, 0),
        )
        .expect("old mtime");
    }

    image_ingest::sweeper::schedule_delayed(app, db, data_dir.clone(), Duration::ZERO);

    wait_until(|| !data_dir.join(&orphan).exists());
}

#[test]
fn image_hash_detects_different_content_with_same_dimensions_and_size() {
    let first = vec![0u8; 4 * 4 * 4];
    let mut second = first.clone();
    let changed_pixel = (2 * 4 + 1) * 4;
    second[changed_pixel] = 200;

    let first = image_data(4, 4, first);
    let second = image_data(4, 4, second);

    assert_ne!(hash_image_content(&first), hash_image_content(&second));
}

#[test]
fn pending_entry_job_and_raw_rgba_staging_are_persisted_together() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(2, 3, 64);

    let job = insert_pending_job(&ctx, &app, "pending", &img);

    let entry = ctx
        .db
        .get_entry_by_id("pending")
        .expect("lookup")
        .expect("entry");
    assert_eq!(entry.status, EntryStatus::Pending);
    let persisted = ctx
        .db
        .get_job_by_id(&job.id)
        .expect("job lookup")
        .expect("job");
    assert_eq!(persisted.kind, ClipboardJobKind::ImageIngest);
    assert_eq!(persisted.status, ClipboardJobStatus::Queued);
    assert_eq!(persisted.input_ref, job.input_ref);
    assert_eq!(
        persisted.pixel_format.as_deref(),
        Some(staging::PIXEL_FORMAT_RGBA8)
    );
    assert_eq!(persisted.byte_size, Some(2 * 3 * 4));
    assert!(ctx.data_dir.join(&persisted.input_ref).exists());

    let added = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].thumbnail_path, None);
    assert!(matches!(
        added[0].preview,
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Pending
        }
    ));
}

#[test]
fn active_image_ingest_backlog_is_bounded_inside_insert_transaction() {
    let ctx = TestContext::new();
    let app = TestApp::new();

    for idx in 0..MAX_ACTIVE_IMAGE_INGEST_JOBS {
        let img = solid_image(1, 1, idx as u8 + 1);
        insert_pending_job(&ctx, &app, &format!("pending-{idx}"), &img);
    }

    let img = solid_image(1, 1, 99);
    let overflow = pending_image_entry("overflow", 20);
    let overflow_job = image_job_draft(&ctx, "overflow", &img);
    let result = ctx.db.insert_pending_image_entry_with_job(
        &overflow,
        &overflow_job,
        MAX_ACTIVE_IMAGE_INGEST_JOBS,
        MAX_ACTIVE_IMAGE_STAGING_BYTES,
    );
    assert!(result.is_err());
    assert!(ctx
        .db
        .get_entry_by_id("overflow")
        .expect("lookup")
        .is_none());
}

#[test]
fn worker_claims_queued_job_as_running() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(1, 1, 1);
    let job = insert_pending_job(&ctx, &app, "pending", &img);

    let claimed = ctx
        .db
        .claim_next_image_ingest_job()
        .expect("claim")
        .expect("job");

    assert_eq!(claimed.id, job.id);
    assert_eq!(claimed.status, ClipboardJobStatus::Running);
    assert_eq!(claimed.attempts, 1);
    assert_eq!(
        ctx.db
            .get_job_by_id(&job.id)
            .expect("job lookup")
            .expect("job")
            .status,
        ClipboardJobStatus::Running
    );
}

#[test]
fn worker_success_commits_artifacts_entry_ready_job_succeeded_and_cleans_staging() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 255);
    let job = insert_pending_job(&ctx, &app, "pending", &img);

    run_next_job(&ctx, &app, &dedup, 500).expect("ran job");

    let entry = ctx
        .db
        .get_entry_by_id("pending")
        .expect("lookup")
        .expect("entry");
    assert_eq!(entry.status, EntryStatus::Ready);
    assert_eq!(
        ctx.db
            .get_job_by_id(&job.id)
            .expect("job lookup")
            .expect("job")
            .status,
        ClipboardJobStatus::Succeeded
    );
    assert!(ctx.data_dir.join(image_original_path("pending")).exists());
    assert!(ctx.data_dir.join(image_display_path("pending")).exists());
    wait_until(|| !ctx.data_dir.join(&job.input_ref).exists());
    assert_eq!(
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
            .len(),
        1
    );
}

#[test]
fn startup_recovery_requeues_running_and_keeps_recoverable_pending() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(1, 1, 9);
    let job = insert_pending_job(&ctx, &app, "pending", &img);
    let claimed = ctx
        .db
        .claim_next_image_ingest_job()
        .expect("claim")
        .expect("job");
    assert_eq!(claimed.status, ClipboardJobStatus::Running);

    let (summary, effects) =
        image_ingest::plan_startup_recovery(&ctx.db, &ctx.data_dir).expect("startup recovery");

    assert_eq!(summary.requeued_running, 1);
    assert!(summary.removed_ids.is_empty());
    assert!(effects.removed_ids.is_empty());
    assert_eq!(
        ctx.db
            .get_job_by_id(&job.id)
            .expect("job")
            .expect("job")
            .status,
        ClipboardJobStatus::Queued
    );
    assert!(ctx.db.get_entry_by_id("pending").expect("entry").is_some());
}

#[test]
fn startup_recovery_removes_missing_staging_and_pending_without_active_job() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(1, 1, 9);
    let job = insert_pending_job(&ctx, &app, "missing-staging", &img);
    std::fs::remove_file(ctx.data_dir.join(&job.input_ref)).expect("remove staging");
    insert_entry(&ctx, &pending_image_entry("orphan-pending", 11));

    let (summary, effects) =
        image_ingest::plan_startup_recovery(&ctx.db, &ctx.data_dir).expect("startup recovery");

    assert_eq!(
        summary.removed_ids,
        vec!["missing-staging".to_string(), "orphan-pending".to_string()]
    );
    assert_eq!(effects.removed_ids, summary.removed_ids);
    assert!(ctx
        .db
        .get_entry_by_id("missing-staging")
        .expect("lookup")
        .is_none());
    assert!(ctx
        .db
        .get_entry_by_id("orphan-pending")
        .expect("lookup")
        .is_none());
}

#[test]
fn startup_recovery_and_sweeper_share_pending_job_staging_consistency_rules() {
    fn setup(ctx: &TestContext, app: &TestApp) {
        let img = solid_image(2, 2, 64);
        let missing = insert_pending_job(ctx, app, "missing-staging", &img);
        std::fs::remove_file(ctx.data_dir.join(&missing.input_ref)).expect("remove staging");
        insert_entry(ctx, &pending_image_entry("orphan-pending", 11));
    }

    let startup_ctx = TestContext::new();
    let startup_app = TestApp::new();
    setup(&startup_ctx, &startup_app);
    let (startup_summary, _) =
        image_ingest::plan_startup_recovery(&startup_ctx.db, &startup_ctx.data_dir)
            .expect("startup recovery");

    let sweep_ctx = TestContext::new();
    let sweep_app = TestApp::new();
    setup(&sweep_ctx, &sweep_app);
    let (sweep_summary, _) =
        image_ingest::sweeper::plan_once(&sweep_ctx.db, &sweep_ctx.data_dir, Duration::ZERO)
            .expect("sweep");

    assert_eq!(startup_summary.removed_ids, sweep_summary.removed_ids);
    assert_eq!(
        sweep_summary.removed_ids,
        vec!["missing-staging".to_string(), "orphan-pending".to_string()]
    );
}

#[test]
fn delete_pending_cancels_durable_job_cleans_staging_and_prevents_resurrection() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let hash = hash_image_content(&img);
    dedup.lock().expect("dedup").last_hash = Some(hash.clone());
    let job = insert_pending_job(&ctx, &app, "pending", &img);
    let running = ctx
        .db
        .claim_next_image_ingest_job()
        .expect("claim")
        .expect("job");

    enhanced_clipboard_lib::services::entry::remove_entry(
        &app,
        &ctx.db,
        &ctx.data_dir,
        Some(&dedup),
        "pending",
        ClipboardQueryStaleReason::EntryRemoved,
    )
    .expect("delete pending");

    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
    assert!(ctx.db.get_entry_by_id("pending").expect("lookup").is_none());
    wait_until(|| !ctx.data_dir.join(&job.input_ref).exists());

    let _ = image_ingest::run_claimed_job(&app, &ctx.db, &ctx.data_dir, 0, 500, &dedup, running);
    assert!(ctx.db.get_entry_by_id("pending").expect("lookup").is_none());
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
}

#[test]
fn canceled_running_job_does_not_clear_newer_polling_dedup() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let hash = hash_image_content(&img);
    dedup.lock().expect("dedup").last_hash = Some(hash.clone());
    insert_pending_job(&ctx, &app, "old-pending", &img);
    let running = ctx
        .db
        .claim_next_image_ingest_job()
        .expect("claim")
        .expect("job");

    enhanced_clipboard_lib::services::entry::remove_entry(
        &app,
        &ctx.db,
        &ctx.data_dir,
        Some(&dedup),
        "old-pending",
        ClipboardQueryStaleReason::EntryRemoved,
    )
    .expect("delete pending");
    dedup.lock().expect("dedup").last_hash = Some(hash.clone());

    image_ingest::run_claimed_job(&app, &ctx.db, &ctx.data_dir, 0, 500, &dedup, running)
        .expect("canceled job cleanup");

    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some(hash.as_str())
    );
}

#[test]
fn same_image_can_be_recaptured_after_pending_delete() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let data_dir = ctx.data_dir.clone();
    let db = Arc::new(ctx.db);
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let worker = start_test_worker(app.clone(), db.clone(), data_dir.clone(), dedup.clone());
    let img = solid_image(2, 2, 64);

    let first = accept_image_clipboard_change(
        ImageIngestDeps {
            app_handle: &app,
            db: &db,
            data_dir: &data_dir,
            worker: &worker,
        },
        &img,
        "Photos",
        &dedup,
    )
    .expect("accept")
    .expect("change");
    assert!(first.persist_result.is_ok());
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();
    enhanced_clipboard_lib::services::entry::remove_entry(
        app.as_ref(),
        &db,
        &data_dir,
        Some(&dedup),
        &id,
        ClipboardQueryStaleReason::EntryRemoved,
    )
    .expect("delete pending");

    let second = accept_image_clipboard_change(
        ImageIngestDeps {
            app_handle: &app,
            db: &db,
            data_dir: &data_dir,
            worker: &worker,
        },
        &img,
        "Photos",
        &dedup,
    )
    .expect("second accept");
    assert!(second.is_some());
}

#[test]
fn polling_dedup_compare_and_clear_does_not_clear_newer_hash() {
    let dedup = Arc::new(Mutex::new(ImageDedupState {
        last_hash: Some("new".to_string()),
    }));

    assert!(!clear_polling_image_dedup_if_current(&dedup, "old"));
    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some("new")
    );
    assert!(clear_polling_image_dedup_if_current(&dedup, "new"));
    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
}

#[test]
fn db_commit_failure_after_generated_files_retries_then_removes_pending() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let job = insert_pending_job(&ctx, &app, "db-error", &img);

    let conn = open_raw_clipboard_conn(&ctx);
    conn.execute_batch(
        "CREATE TRIGGER fail_artifact_insert
         BEFORE INSERT ON clipboard_entry_artifacts
         BEGIN
             SELECT RAISE(FAIL, 'forced artifact failure');
         END;",
    )
    .expect("install trigger");
    drop(conn);

    assert_eq!(
        image_ingest::run_next_job(&app, &ctx.db, &ctx.data_dir, 0, 500, &dedup),
        Ok(true)
    );
    assert!(!ctx.data_dir.join(image_original_path("db-error")).exists());
    assert!(!ctx.data_dir.join(image_display_path("db-error")).exists());
    let persisted = ctx
        .db
        .get_job_by_id(&job.id)
        .expect("job lookup")
        .expect("job");
    assert_eq!(persisted.status, ClipboardJobStatus::Queued);
    assert_eq!(persisted.attempts, 1);

    assert_eq!(
        image_ingest::run_next_job(&app, &ctx.db, &ctx.data_dir, 0, 500, &dedup),
        Ok(true)
    );

    assert!(ctx
        .db
        .get_entry_by_id("db-error")
        .expect("lookup")
        .is_none());
    wait_until(|| {
        !ctx.data_dir.join(image_original_path("db-error")).exists()
            && !ctx.data_dir.join(image_display_path("db-error")).exists()
    });
}

#[test]
fn attempts_exhausted_removes_pending_entry_without_persisted_failed_entry() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let job = insert_pending_job(&ctx, &app, "write-error", &img);
    std::fs::remove_dir(ctx.data_dir.join("images")).expect("remove images dir");
    std::fs::write(ctx.data_dir.join("images"), b"not a dir").expect("block images dir");

    for _ in 0..MAX_IMAGE_INGEST_ATTEMPTS {
        assert_eq!(
            image_ingest::run_next_job(&app, &ctx.db, &ctx.data_dir, 0, 500, &dedup),
            Ok(true)
        );
    }

    assert!(ctx
        .db
        .get_entry_by_id("write-error")
        .expect("lookup")
        .is_none());
    assert!(ctx.db.get_job_by_id(&job.id).expect("job lookup").is_none());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["write-error".to_string()]]
    );
}

#[test]
fn retention_removes_just_ready_image_with_removed_event_only() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    insert_entry(&ctx, &text_entry("new-text", 20, "newer"));
    let img = solid_image(2, 2, 64);
    insert_pending_job(&ctx, &app, "old-image", &img);

    run_next_job(&ctx, &app, &dedup, 1).expect("ran job");

    assert!(ctx
        .db
        .get_entry_by_id("old-image")
        .expect("lookup")
        .is_none());
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["old-image".to_string()]]
    );
}

#[test]
fn clear_all_cancels_pending_jobs_and_cleans_staging() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let hash = hash_image_content(&img);
    dedup.lock().expect("dedup").last_hash = Some(hash);
    let job = insert_pending_job(&ctx, &app, "pending", &img);

    let cleared = enhanced_clipboard_lib::services::entry::clear_all_entries(
        &app,
        &ctx.db,
        &ctx.data_dir,
        Some(&dedup),
    )
    .expect("clear all");

    assert_eq!(cleared, vec!["pending".to_string()]);
    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
    wait_until(|| !ctx.data_dir.join(&job.input_ref).exists());
}

#[test]
fn delete_ready_image_cleans_terminal_image_ingest_staging_without_clearing_dedup() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let hash = hash_image_content(&img);
    dedup.lock().expect("dedup").last_hash = Some(hash.clone());
    let job = insert_pending_job(&ctx, &app, "ready", &img);
    mark_image_job_terminal(&ctx, "ready", &job.id, ClipboardJobStatus::Succeeded);

    enhanced_clipboard_lib::services::entry::remove_entry(
        &app,
        &ctx.db,
        &ctx.data_dir,
        Some(&dedup),
        "ready",
        ClipboardQueryStaleReason::EntryRemoved,
    )
    .expect("delete ready");

    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some(hash.as_str())
    );
    wait_until(|| !ctx.data_dir.join(&job.input_ref).exists());
}

#[test]
fn clear_all_cleans_terminal_image_ingest_staging() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let job = insert_pending_job(&ctx, &app, "ready", &img);
    mark_image_job_terminal(&ctx, "ready", &job.id, ClipboardJobStatus::Succeeded);

    let cleared = enhanced_clipboard_lib::services::entry::clear_all_entries(
        &app,
        &ctx.db,
        &ctx.data_dir,
        Some(&dedup),
    )
    .expect("clear all");

    assert_eq!(cleared, vec!["ready".to_string()]);
    wait_until(|| !ctx.data_dir.join(&job.input_ref).exists());
}

#[test]
fn terminal_jobs_can_be_cleaned_safely_and_future_kinds_do_not_affect_retention() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let img = solid_image(2, 2, 64);
    let job = insert_pending_job(&ctx, &app, "pending", &img);
    run_next_job(&ctx, &app, &dedup, 500).expect("run job");

    let terminal_cleanup = ctx
        .db
        .cleanup_terminal_image_ingest_jobs()
        .expect("cleanup jobs");
    assert_eq!(terminal_cleanup.len(), 1);
    assert!(ctx.db.get_job_by_id(&job.id).expect("job lookup").is_none());
    assert!(ctx.db.get_entry_by_id("pending").expect("entry").is_some());

    insert_entry(&ctx, &text_entry("future-entry", 30, "future"));
    let future_job_id = Uuid::new_v4().to_string();
    let conn = open_raw_clipboard_conn(&ctx);
    conn.execute(
        "INSERT INTO clipboard_jobs
         (id, entry_id, kind, status, input_ref, dedup_key, attempts, created_at, updated_at)
         VALUES (?1, 'future-entry', 'file_preview', 'succeeded', 'files/future.bin', 'future', 1, 30, 30)",
        [&future_job_id],
    )
    .expect("insert future job");
    drop(conn);
    let terminal_cleanup = ctx
        .db
        .cleanup_terminal_image_ingest_jobs()
        .expect("cleanup jobs");
    assert_eq!(terminal_cleanup.len(), 0);
    assert!(ctx
        .db
        .get_job_by_id(&future_job_id)
        .expect("future job lookup")
        .is_some());

    let future_kind = ClipboardJobKind::FilePreview;
    assert_eq!(future_kind.as_str(), "file_preview");
}

#[test]
fn worker_claim_ignores_unimplemented_future_job_kinds() {
    let ctx = TestContext::new();
    insert_entry(&ctx, &text_entry("future-entry", 30, "future"));
    let conn = open_raw_clipboard_conn(&ctx);
    conn.execute(
        "INSERT INTO clipboard_jobs
         (id, entry_id, kind, status, input_ref, dedup_key, attempts, created_at, updated_at)
         VALUES (?1, 'future-entry', 'file_preview', 'queued', 'files/future.bin', 'future', 0, 30, 30)",
        [Uuid::new_v4().to_string()],
    )
    .expect("insert future job");
    drop(conn);

    assert!(ctx
        .db
        .claim_next_image_ingest_job()
        .expect("claim")
        .is_none());
}

#[test]
fn image_cleanup_does_not_plan_future_job_input_paths() {
    let ctx = TestContext::new();
    insert_entry(&ctx, &text_entry("future-entry", 30, "future"));
    std::fs::create_dir_all(ctx.data_dir.join("files")).expect("files dir");
    std::fs::write(ctx.data_dir.join("files/future.bin"), b"future input").expect("future input");

    let conn = open_raw_clipboard_conn(&ctx);
    conn.execute(
        "INSERT INTO clipboard_jobs
         (id, entry_id, kind, status, input_ref, dedup_key, attempts, created_at, updated_at)
         VALUES (?1, 'future-entry', 'file_preview', 'queued', 'files/future.bin', 'future', 0, 30, 30)",
        [Uuid::new_v4().to_string()],
    )
    .expect("insert future job");
    drop(conn);

    let plan = image_ingest::cancel_all(&ctx.db).expect("cancel all");

    assert_eq!(plan.removed_ids, vec!["future-entry".to_string()]);
    assert!(!plan
        .cleanup_paths
        .iter()
        .any(|path| path == "files/future.bin"));
    assert!(ctx.data_dir.join("files/future.bin").exists());
}

#[test]
fn startup_recovery_cleans_terminal_job_staging_before_deleting_job() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(2, 2, 64);
    let job = insert_pending_job(&ctx, &app, "ready-image", &img);

    let conn = open_raw_clipboard_conn(&ctx);
    conn.execute(
        "UPDATE clipboard_entries SET status = 'ready' WHERE id = 'ready-image'",
        [],
    )
    .expect("mark entry ready");
    conn.execute(
        "UPDATE clipboard_jobs SET status = 'succeeded' WHERE id = ?1",
        [&job.id],
    )
    .expect("mark job succeeded");
    drop(conn);

    let (summary, effects) =
        image_ingest::plan_startup_recovery(&ctx.db, &ctx.data_dir).expect("startup recovery");

    assert!(summary.removed_ids.is_empty());
    assert!(effects
        .cleanup_paths
        .iter()
        .any(|path| path == &job.input_ref));
    assert!(ctx.db.get_job_by_id(&job.id).expect("job lookup").is_none());
    assert!(ctx
        .db
        .get_entry_by_id("ready-image")
        .expect("entry lookup")
        .is_some());
}

#[test]
fn background_maintenance_removes_terminal_and_old_unreferenced_staging_inputs() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(2, 2, 64);
    let referenced = insert_pending_job(&ctx, &app, "pending", &img);
    let terminal_img = solid_image(2, 2, 65);
    let terminal = insert_pending_job(&ctx, &app, "terminal", &terminal_img);
    let old_orphan = staging::input_rel_path(&Uuid::new_v4().to_string());
    let recent_orphan = staging::input_rel_path(&Uuid::new_v4().to_string());
    std::fs::write(ctx.data_dir.join(&old_orphan), b"old orphan").expect("old orphan");
    std::fs::write(ctx.data_dir.join(&recent_orphan), b"recent orphan").expect("recent orphan");
    make_old_file(&ctx, &old_orphan);
    let conn = open_raw_clipboard_conn(&ctx);
    conn.execute(
        "UPDATE clipboard_entries SET status = 'ready' WHERE id = 'terminal'",
        [],
    )
    .expect("mark terminal entry ready");
    conn.execute(
        "UPDATE clipboard_jobs SET status = 'succeeded' WHERE id = ?1",
        [&terminal.id],
    )
    .expect("mark terminal job succeeded");
    drop(conn);

    let report =
        enhanced_clipboard_lib::services::artifacts::maintenance::run_artifact_maintenance_once(
            &app,
            &ctx.db,
            &ctx.data_dir,
            enhanced_clipboard_lib::services::artifacts::maintenance::ArtifactMaintenanceOptions {
                max_repairs: 0,
            },
        )
        .expect("maintenance");

    assert_eq!(report.orphan_files_removed, 2);
    wait_until(|| !ctx.data_dir.join(&old_orphan).exists());
    wait_until(|| !ctx.data_dir.join(&terminal.input_ref).exists());
    assert!(ctx
        .db
        .get_job_by_id(&terminal.id)
        .expect("terminal lookup")
        .is_none());
    assert!(ctx.data_dir.join(&recent_orphan).exists());
    assert!(ctx.data_dir.join(&referenced.input_ref).exists());
}

#[test]
fn capture_image_emits_pending_then_worker_finalizes_ready_item() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let data_dir = ctx.data_dir.clone();
    let db = Arc::new(ctx.db);
    let dedup = Arc::new(Mutex::new(ImageDedupState::default()));
    let worker = start_test_worker(app.clone(), db.clone(), data_dir.clone(), dedup.clone());
    let img = solid_image(2, 2, 255);
    let hash = hash_image_content(&img);

    image_ingest::capture_image(
        CaptureImageDeps {
            app_handle: &app,
            db: &db,
            data_dir: &data_dir,
            worker: &worker,
        },
        &img,
        "Photos".to_string(),
        dedup.clone(),
        hash,
    )
    .expect("save image");

    let added = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED);
    assert_eq!(added.len(), 1);
    let id = added[0].id.clone();
    wait_until(|| {
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
            .iter()
            .any(|item| item.id == id)
    });
    let event_names = app.event_names();
    let added_index = event_names
        .iter()
        .position(|name| name == EVENT_STREAM_ITEM_ADDED)
        .expect("added event");
    let updated_index = event_names
        .iter()
        .position(|name| name == EVENT_STREAM_ITEM_UPDATED)
        .expect("updated event");
    assert!(added_index < updated_index);
}

#[test]
fn cleanup_failure_is_logged_only_and_does_not_roll_back_delete() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let img = solid_image(1, 1, 1);
    insert_pending_job(&ctx, &app, "pending", &img);
    std::fs::remove_dir_all(ctx.data_dir.join("staging")).expect("remove staging root");

    let (summary, effects) =
        image_ingest::plan_startup_recovery(&ctx.db, &ctx.data_dir).expect("startup recovery");

    assert_eq!(summary.removed_ids, vec!["pending".to_string()]);
    assert_eq!(
        effects.stale_reason,
        Some(ClipboardQueryStaleReason::SettingsOrStartup)
    );
    assert!(ctx.db.get_entry_by_id("pending").expect("lookup").is_none());
}
