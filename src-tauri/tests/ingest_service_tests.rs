use std::borrow::Cow;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arboard::ImageData;

use enhanced_clipboard_lib::constants::{
    EVENT_ENTRIES_REMOVED, EVENT_QUERY_RESULTS_STALE, EVENT_STREAM_ITEM_ADDED,
    EVENT_STREAM_ITEM_UPDATED,
};
use enhanced_clipboard_lib::models::{
    ClipboardListItem, ClipboardPreview, ClipboardQueryStaleReason, EntryStatus,
};
use enhanced_clipboard_lib::services::entry::{clear_all_entries, remove_entry};
use enhanced_clipboard_lib::services::ingest::{
    accept_image_clipboard_change as accept_image_clipboard_change_inner,
    save_image_entry as save_image_entry_inner, ImageDedupState, ImageIngestDeps,
};
use enhanced_clipboard_lib::services::view_events::EventEmitter;
use enhanced_clipboard_lib::services::{
    jobs::{
        ContentJobWorker, DeferredClaimRegistry, DeferredContentJob, DeferredJobContext,
        DeferredJobQueue, DeferredJobResult, DeferredJobStartGate, ImageAssetJob, ImageDedupUpdate,
    },
    pipeline,
};
use enhanced_clipboard_lib::utils::image::hash_image_content;
use serde::Serialize;

mod common;

use common::{
    image_artifacts, image_display_path, image_original_path, insert_entry, pending_image_entry,
    text_entry, touch_file, TestApp, TestContext,
};

fn image_deps<'a, A, Q>(
    app_handle: &'a A,
    db: &'a Arc<enhanced_clipboard_lib::db::Database>,
    data_dir: &'a std::path::Path,
    worker: &'a Q,
    claims: &'a DeferredClaimRegistry,
) -> ImageIngestDeps<'a, A, Q>
where
    Q: DeferredJobQueue,
{
    ImageIngestDeps {
        app_handle,
        db,
        data_dir,
        worker,
        claims,
    }
}

#[allow(clippy::too_many_arguments)]
fn accept_image_clipboard_change<A, Q>(
    app_handle: &A,
    db: &Arc<enhanced_clipboard_lib::db::Database>,
    data_dir: &std::path::Path,
    worker: &Q,
    claims: &DeferredClaimRegistry,
    img: &ImageData,
    source_app: &str,
    image_dedup: &Arc<std::sync::Mutex<ImageDedupState>>,
    _expiry_seconds: i64,
    _max_history: u32,
) -> Result<Option<enhanced_clipboard_lib::services::ingest::AcceptedImageChange>, String>
where
    A: EventEmitter + Clone + Send + 'static,
    Q: DeferredJobQueue,
{
    accept_image_clipboard_change_inner(
        image_deps(app_handle, db, data_dir, worker, claims),
        img,
        source_app,
        image_dedup,
    )
}

#[allow(clippy::too_many_arguments)]
fn save_image_entry<A, Q>(
    app_handle: &A,
    db: &Arc<enhanced_clipboard_lib::db::Database>,
    data_dir: &std::path::Path,
    worker: &Q,
    claims: &DeferredClaimRegistry,
    img: &ImageData,
    source_app: String,
    _expiry_seconds: i64,
    _max_history: u32,
    dedup_update: Option<ImageDedupUpdate>,
) -> Result<(), String>
where
    A: EventEmitter + Clone + Send + 'static,
    Q: DeferredJobQueue,
{
    save_image_entry_inner(
        image_deps(app_handle, db, data_dir, worker, claims),
        img,
        source_app,
        dedup_update,
    )
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

fn start_test_worker<A>(
    app: Arc<A>,
    db: Arc<enhanced_clipboard_lib::db::Database>,
    data_dir: std::path::PathBuf,
    expiry_seconds: i64,
    max_history: u32,
    claims: Arc<DeferredClaimRegistry>,
) -> ContentJobWorker
where
    A: EventEmitter + Send + Sync + 'static,
{
    let (worker, results) = ContentJobWorker::start();
    thread::spawn(move || {
        while let Ok(result) = results.recv() {
            pipeline::finalize_deferred_result(
                &app,
                &db,
                &data_dir,
                expiry_seconds,
                max_history,
                &claims,
                result,
            )
            .expect("finalize deferred result");
        }
    });
    worker
}

fn blocking_image_job(
    data_dir: &std::path::Path,
    entry_id: &str,
    gate: DeferredJobStartGate,
) -> DeferredContentJob {
    DeferredContentJob::Image(ImageAssetJob {
        context: DeferredJobContext {
            entry_id: entry_id.to_string(),
            dedup_update: None,
        },
        data_dir: data_dir.to_path_buf(),
        rgba: vec![255; 4],
        width: 1,
        height: 1,
        start_gate: Some(gate),
    })
}

fn enqueue_blocking_job_until_ok(
    worker: &ContentJobWorker,
    data_dir: &std::path::Path,
    entry_id: &str,
    gate: DeferredJobStartGate,
) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if worker
            .enqueue(blocking_image_job(data_dir, entry_id, gate.clone()))
            .is_ok()
        {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("failed to fill bounded worker queue");
}

struct RejectingQueue;

impl DeferredJobQueue for RejectingQueue {
    fn enqueue(&self, _job: DeferredContentJob) -> Result<(), String> {
        Err("queue closed".to_string())
    }
}

struct ObservingQueue {
    app: Arc<TestApp>,
}

impl DeferredJobQueue for ObservingQueue {
    fn enqueue(&self, _job: DeferredContentJob) -> Result<(), String> {
        let added = self
            .app
            .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED);
        assert!(added.is_empty(), "pending added event must follow enqueue");
        Ok(())
    }
}

struct AcceptingQueue;

impl DeferredJobQueue for AcceptingQueue {
    fn enqueue(&self, _job: DeferredContentJob) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct CapturingQueue {
    job: std::sync::Mutex<Option<DeferredContentJob>>,
}

impl CapturingQueue {
    fn take_context(&self) -> DeferredJobContext {
        self.job
            .lock()
            .expect("captured job")
            .take()
            .expect("job")
            .context()
            .clone()
    }
}

impl DeferredJobQueue for CapturingQueue {
    fn enqueue(&self, job: DeferredContentJob) -> Result<(), String> {
        *self.job.lock().expect("captured job") = Some(job);
        Ok(())
    }
}

struct FailingEventApp;

impl EventEmitter for FailingEventApp {
    fn emit_event<S: Serialize + Clone>(&self, _event: &str, _payload: S) -> Result<(), String> {
        Err("emit failed".to_string())
    }
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
fn save_image_entry_emits_pending_then_finalizes_ready_item() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let claims = Arc::new(DeferredClaimRegistry::new());
    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        500,
        claims.clone(),
    );
    let img = solid_image(2, 2, 255);

    save_image_entry(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos".to_string(),
        0,
        500,
        None,
    )
    .expect("save image");

    let added = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].thumbnail_path, None);
    assert!(matches!(
        added[0].preview,
        ClipboardPreview::Image {
            mode: enhanced_clipboard_lib::models::ClipboardImagePreviewMode::Pending
        }
    ));
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

    let _entry = db
        .get_entry_by_id(&id)
        .expect("lookup")
        .expect("finalized entry");
    let expected_image = format!("images/{id}.png");
    let expected_thumb = format!("thumbnails/{id}.png");
    let artifacts = db.get_artifacts_for_entry(&id).expect("artifacts");
    assert!(artifacts
        .iter()
        .any(|artifact| artifact.rel_path == expected_image));
    assert!(artifacts
        .iter()
        .any(|artifact| artifact.rel_path == expected_thumb));
    assert!(ctx.data_dir.join(expected_image).exists());
    assert!(ctx.data_dir.join(expected_thumb).exists());
}

#[test]
fn ready_insert_emits_added_even_when_post_insert_retention_fails() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let entry = text_entry("ready-retention-error", 10, "Alpha");

    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(
        "PRAGMA key = \"x'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\";
         CREATE TRIGGER fail_retention_delete_after_ready_insert
         BEFORE DELETE ON clipboard_entries
         BEGIN
             SELECT RAISE(FAIL, 'forced retention delete failure');
         END;",
    )
    .expect("install trigger");
    drop(conn);

    let result = pipeline::insert_ready_entry(&app, &ctx.db, &ctx.data_dir, &entry, &[], 0, 0);

    assert!(result.is_err());
    assert!(ctx
        .db
        .get_entry_by_id("ready-retention-error")
        .expect("lookup")
        .is_some());
    assert_eq!(
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)
            .len(),
        1
    );
}

#[test]
fn original_write_failure_rolls_back_pending_without_pruning_existing_history() {
    let ctx = TestContext::new();
    let old = text_entry("old", 10, "old");
    insert_entry(&ctx, &old);
    std::fs::remove_dir(ctx.data_dir.join("images")).expect("remove images dir");
    std::fs::write(ctx.data_dir.join("images"), b"not a dir").expect("block images dir");

    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let claims = Arc::new(DeferredClaimRegistry::new());
    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        1,
        claims.clone(),
    );
    let img = solid_image(2, 2, 64);

    save_image_entry(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos".to_string(),
        0,
        1,
        None,
    )
    .expect("queue image");
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();

    wait_until(|| {
        db.get_entry_by_id(&id).expect("pending lookup").is_none()
            && !app
                .captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED)
                .is_empty()
    });

    assert!(db.get_entry_by_id("old").expect("old lookup").is_some());
}

#[test]
fn enqueue_failure_removes_pending_entry_and_releases_dedup_for_recapture() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let img = solid_image(2, 2, 64);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));
    let claims = Arc::new(DeferredClaimRegistry::new());

    let first = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &RejectingQueue,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("accept")
    .expect("change");

    assert!(first.persist_result.is_err());
    assert!(db.get_image_asset_records().expect("records").is_empty());
    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)
        .is_empty());
    assert!(app
        .captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED)
        .is_empty());

    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        500,
        claims.clone(),
    );
    let second = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("second accept");
    assert!(second.is_some());
}

#[test]
fn bounded_worker_queue_full_rolls_back_pending_and_releases_dedup() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let (worker, _results) = ContentJobWorker::start();
    let gate = DeferredJobStartGate::new();

    enqueue_blocking_job_until_ok(&worker, &ctx.data_dir, "blocked-1", gate.clone());
    enqueue_blocking_job_until_ok(&worker, &ctx.data_dir, "blocked-2", gate.clone());
    enqueue_blocking_job_until_ok(&worker, &ctx.data_dir, "blocked-3", gate.clone());

    let img = solid_image(2, 2, 64);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));
    let claims = DeferredClaimRegistry::new();
    let change = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("accept")
    .expect("change");

    assert!(change.persist_result.is_err());
    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
    assert!(db.get_image_asset_records().expect("records").is_empty());
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)
        .is_empty());
    gate.release();
}

#[test]
fn pending_image_enqueue_succeeds_before_added_event_is_emitted() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let img = solid_image(2, 2, 64);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));
    let claims = DeferredClaimRegistry::new();

    let change = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &AcceptingQueue,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("accept")
    .expect("change");

    assert!(change.persist_result.is_ok());
    assert_eq!(
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)
            .len(),
        1
    );
}

#[test]
fn delete_pending_before_worker_completes_releases_active_dedup_claim() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let queue = CapturingQueue::default();
    let claims = DeferredClaimRegistry::new();
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));
    let img = solid_image(2, 2, 64);
    let hash = hash_image_content(&img);

    let first = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &queue,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("first accept")
    .expect("first change");
    assert!(first.persist_result.is_ok());
    let old_context = queue.take_context();
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();

    assert!(remove_entry(
        &app,
        &db,
        &ctx.data_dir,
        &claims,
        &id,
        ClipboardQueryStaleReason::EntryRemoved,
    )
    .expect("delete pending"));
    assert_eq!(dedup.lock().expect("dedup").last_hash, None);

    let second = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &AcceptingQueue,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("second accept");
    assert!(second.is_some());

    std::fs::write(ctx.data_dir.join(image_original_path(&id)), b"artifact").expect("original");
    std::fs::write(ctx.data_dir.join(image_display_path(&id)), b"artifact").expect("display");
    pipeline::finalize_deferred_result(
        &app,
        &db,
        &ctx.data_dir,
        0,
        500,
        &claims,
        DeferredJobResult::Ready {
            context: old_context,
            artifacts: image_artifacts(&id),
        },
    )
    .expect("old result cleanup");

    assert!(db.get_entry_by_id(&id).expect("old lookup").is_none());
    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some(hash.as_str())
    );
}

#[test]
fn clear_all_releases_pending_dedup_claims() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let queue = CapturingQueue::default();
    let claims = DeferredClaimRegistry::new();
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));
    let img = solid_image(2, 2, 64);

    let first = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &queue,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("first accept")
    .expect("first change");
    assert!(first.persist_result.is_ok());

    let cleared = clear_all_entries(&app, &db, &ctx.data_dir, &claims).expect("clear all pending");
    assert_eq!(cleared.len(), 1);
    assert_eq!(dedup.lock().expect("dedup").last_hash, None);

    let second = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &AcceptingQueue,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("second accept");
    assert!(second.is_some());
}

#[test]
fn added_event_failure_does_not_abort_deferred_lifecycle_or_release_dedup() {
    let ctx = TestContext::new();
    let app = Arc::new(FailingEventApp);
    let db = Arc::new(ctx.db);
    let claims = Arc::new(DeferredClaimRegistry::new());
    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        500,
        claims.clone(),
    );
    let img = solid_image(2, 2, 64);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));

    let change = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("accept")
    .expect("change");
    assert!(change.persist_result.is_ok());

    wait_until(|| {
        db.get_image_asset_records()
            .expect("records")
            .iter()
            .any(|record| record.status == EntryStatus::Ready)
    });
    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some(hash_image_content(&img).as_str())
    );
}

#[test]
fn image_asset_failure_clears_dedup_hash_so_recapture_is_possible() {
    let ctx = TestContext::new();
    std::fs::remove_dir(ctx.data_dir.join("images")).expect("remove images dir");
    std::fs::write(ctx.data_dir.join("images"), b"not a dir").expect("block images dir");

    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let claims = Arc::new(DeferredClaimRegistry::new());
    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        500,
        claims.clone(),
    );
    let img = solid_image(2, 2, 64);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));

    let change = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("accept")
    .expect("change");
    assert!(change.persist_result.is_ok());
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();

    wait_until(|| db.get_entry_by_id(&id).expect("pending lookup").is_none());

    let state = dedup.lock().expect("dedup");
    assert_eq!(state.last_hash, None);
}

#[test]
fn display_write_failure_rolls_back_pending_and_generated_original() {
    let ctx = TestContext::new();
    std::fs::remove_dir(ctx.data_dir.join("thumbnails")).expect("remove thumbnails dir");
    std::fs::write(ctx.data_dir.join("thumbnails"), b"not a dir").expect("block thumbnails dir");

    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let claims = Arc::new(DeferredClaimRegistry::new());
    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        500,
        claims.clone(),
    );
    let img = solid_image(601, 301, 180);

    save_image_entry(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos".to_string(),
        0,
        500,
        None,
    )
    .expect("queue image");
    let id = app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)[0]
        .id
        .clone();

    wait_until(|| db.get_entry_by_id(&id).expect("pending lookup").is_none());

    assert!(!ctx.data_dir.join(format!("images/{id}.png")).exists());
}

#[test]
fn finalize_returns_none_after_pending_entry_was_deleted() {
    let ctx = TestContext::new();
    let pending = pending_image_entry("pending-image", 10);
    insert_entry(&ctx, &pending);

    ctx.db
        .delete_entry("pending-image")
        .expect("delete pending");

    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some("hash".to_string()),
    }));
    touch_file(&ctx, &image_original_path("pending-image"));
    touch_file(&ctx, &image_display_path("pending-image"));
    let claims = DeferredClaimRegistry::new();
    let context = DeferredJobContext {
        entry_id: "pending-image".to_string(),
        dedup_update: Some(ImageDedupUpdate {
            state: dedup.clone(),
            content_hash: "hash".to_string(),
        }),
    };
    claims.register(&context);

    pipeline::finalize_deferred_result(
        &TestApp::new(),
        &ctx.db,
        &ctx.data_dir,
        0,
        500,
        &claims,
        DeferredJobResult::Ready {
            context,
            artifacts: image_artifacts("pending-image"),
        },
    )
    .expect("terminalize deleted pending");

    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
    wait_until(|| {
        !ctx.data_dir
            .join(image_original_path("pending-image"))
            .exists()
            && !ctx
                .data_dir
                .join(image_display_path("pending-image"))
                .exists()
    });
}

#[test]
fn finalize_db_error_terminalizes_pending_dedup_and_generated_artifacts() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let pending = pending_image_entry("pending-db-error", 10);
    insert_entry(&ctx, &pending);
    touch_file(&ctx, &image_original_path("pending-db-error"));
    touch_file(&ctx, &image_display_path("pending-db-error"));
    let img = solid_image(2, 2, 64);
    let content_hash = hash_image_content(&img);
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some(content_hash.clone()),
    }));
    let claims = DeferredClaimRegistry::new();
    let context = DeferredJobContext {
        entry_id: "pending-db-error".to_string(),
        dedup_update: Some(ImageDedupUpdate {
            state: dedup.clone(),
            content_hash: content_hash.clone(),
        }),
    };
    claims.register(&context);

    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(
        "PRAGMA key = \"x'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\";
         CREATE TRIGGER fail_artifact_insert
         BEFORE INSERT ON clipboard_entry_artifacts
         BEGIN
             SELECT RAISE(FAIL, 'forced artifact failure');
         END;",
    )
    .expect("install trigger");
    drop(conn);

    let result = pipeline::finalize_deferred_result(
        &app,
        &ctx.db,
        &ctx.data_dir,
        0,
        500,
        &claims,
        DeferredJobResult::Ready {
            context,
            artifacts: image_artifacts("pending-db-error"),
        },
    );

    assert!(result.is_err());
    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
    assert!(ctx
        .db
        .get_entry_by_id("pending-db-error")
        .expect("lookup")
        .is_none());
    wait_until(|| {
        !ctx.data_dir
            .join(image_original_path("pending-db-error"))
            .exists()
            && !ctx
                .data_dir
                .join(image_display_path("pending-db-error"))
                .exists()
    });
    assert_eq!(
        app.captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED),
        vec![vec!["pending-db-error".to_string()]]
    );
    assert_eq!(
        app.captured_event::<ClipboardQueryStaleReason>(EVENT_QUERY_RESULTS_STALE),
        vec![ClipboardQueryStaleReason::EntriesRemoved]
    );
    assert!(app
        .captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
        .is_empty());

    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(
        "PRAGMA key = \"x'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\";
         DROP TRIGGER fail_artifact_insert;",
    )
    .expect("drop trigger");
    drop(conn);

    let db = Arc::new(ctx.db);
    let second = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &ObservingQueue { app: app.clone() },
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("recapture after terminal failure");
    assert!(second.is_some());
}

#[test]
fn failed_job_delete_pending_failure_surfaces_error_and_keeps_dedup_claim() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let pending = pending_image_entry("pending-delete-error", 10);
    insert_entry(&ctx, &pending);
    touch_file(&ctx, &image_original_path("pending-delete-error"));
    touch_file(&ctx, &image_display_path("pending-delete-error"));
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some("hash".to_string()),
    }));
    let claims = DeferredClaimRegistry::new();
    let context = DeferredJobContext {
        entry_id: "pending-delete-error".to_string(),
        dedup_update: Some(ImageDedupUpdate {
            state: dedup.clone(),
            content_hash: "hash".to_string(),
        }),
    };
    claims.register(&context);

    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(
        "PRAGMA key = \"x'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\";
         CREATE TRIGGER fail_pending_delete
         BEFORE DELETE ON clipboard_entries
         BEGIN
             SELECT RAISE(FAIL, 'forced pending delete failure');
         END;",
    )
    .expect("install trigger");
    drop(conn);

    let result = pipeline::finalize_deferred_result(
        &app,
        &ctx.db,
        &ctx.data_dir,
        0,
        500,
        &claims,
        DeferredJobResult::Failed {
            context,
            cleanup_paths: vec![
                image_original_path("pending-delete-error"),
                image_display_path("pending-delete-error"),
            ],
            error: "forced job failure".to_string(),
        },
    );

    assert!(result.is_err());
    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some("hash")
    );
    assert!(ctx
        .db
        .get_entry_by_id("pending-delete-error")
        .expect("lookup")
        .is_some());
    wait_until(|| {
        !ctx.data_dir
            .join(image_original_path("pending-delete-error"))
            .exists()
            && !ctx
                .data_dir
                .join(image_display_path("pending-delete-error"))
                .exists()
    });
    assert!(app
        .captured_event::<Vec<String>>(EVENT_ENTRIES_REMOVED)
        .is_empty());
}

#[test]
fn retention_removal_after_finalize_releases_dedup_and_skips_updated_event() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let pending = pending_image_entry("old-image", 10);
    insert_entry(&ctx, &pending);
    insert_entry(&ctx, &text_entry("new-text", 20, "newer"));
    touch_file(&ctx, &image_original_path("old-image"));
    touch_file(&ctx, &image_display_path("old-image"));
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some("old-hash".to_string()),
    }));
    let claims = DeferredClaimRegistry::new();
    let context = DeferredJobContext {
        entry_id: "old-image".to_string(),
        dedup_update: Some(ImageDedupUpdate {
            state: dedup.clone(),
            content_hash: "old-hash".to_string(),
        }),
    };
    claims.register(&context);

    pipeline::finalize_deferred_result(
        &app,
        &ctx.db,
        &ctx.data_dir,
        0,
        1,
        &claims,
        DeferredJobResult::Ready {
            context,
            artifacts: image_artifacts("old-image"),
        },
    )
    .expect("finalize old image");

    assert_eq!(dedup.lock().expect("dedup").last_hash, None);
    assert!(ctx
        .db
        .get_entry_by_id("old-image")
        .expect("old lookup")
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
fn retention_error_after_finalize_forgets_active_claim_without_clearing_dedup() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    let pending = pending_image_entry("retention-error", 10);
    insert_entry(&ctx, &pending);
    touch_file(&ctx, &image_original_path("retention-error"));
    touch_file(&ctx, &image_display_path("retention-error"));
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some("hash".to_string()),
    }));
    let claims = DeferredClaimRegistry::new();
    let context = DeferredJobContext {
        entry_id: "retention-error".to_string(),
        dedup_update: Some(ImageDedupUpdate {
            state: dedup.clone(),
            content_hash: "hash".to_string(),
        }),
    };
    claims.register(&context);

    let conn = rusqlite::Connection::open(ctx.data_dir.join("clipboard.db")).expect("open db");
    conn.execute_batch(
        "PRAGMA key = \"x'0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'\";
         CREATE TRIGGER fail_retention_delete_after_finalize
         BEFORE DELETE ON clipboard_entries
         BEGIN
             SELECT RAISE(FAIL, 'forced retention delete failure');
         END;",
    )
    .expect("install trigger");
    drop(conn);

    let result = pipeline::finalize_deferred_result(
        &app,
        &ctx.db,
        &ctx.data_dir,
        0,
        0,
        &claims,
        DeferredJobResult::Ready {
            context,
            artifacts: image_artifacts("retention-error"),
        },
    );

    assert!(result.is_err());
    assert!(ctx
        .db
        .get_entry_by_id("retention-error")
        .expect("lookup")
        .is_some());
    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some("hash")
    );
    assert!(!claims.is_active("retention-error"));
    assert_eq!(
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_UPDATED)
            .len(),
        1
    );
}

#[test]
fn old_job_dedup_release_does_not_clear_newer_image_hash() {
    let ctx = TestContext::new();
    let pending = pending_image_entry("old-image", 10);
    insert_entry(&ctx, &pending);
    ctx.db
        .delete_entry("old-image")
        .expect("delete old pending");
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some("old-hash".to_string()),
    }));
    dedup.lock().expect("dedup").last_hash = Some("new-hash".to_string());
    let claims = DeferredClaimRegistry::new();

    pipeline::finalize_deferred_result(
        &TestApp::new(),
        &ctx.db,
        &ctx.data_dir,
        0,
        500,
        &claims,
        DeferredJobResult::Ready {
            context: DeferredJobContext {
                entry_id: "old-image".to_string(),
                dedup_update: Some(ImageDedupUpdate {
                    state: dedup.clone(),
                    content_hash: "old-hash".to_string(),
                }),
            },
            artifacts: image_artifacts("old-image"),
        },
    )
    .expect("terminalize old job");

    assert_eq!(
        dedup.lock().expect("dedup").last_hash.as_deref(),
        Some("new-hash")
    );
}

#[test]
fn content_hash_is_recorded_immediately_for_accepted_images() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let claims = Arc::new(DeferredClaimRegistry::new());
    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        500,
        claims.clone(),
    );
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState { last_hash: None }));
    let img = solid_image(2, 2, 2);

    let change = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("accept")
    .expect("change");

    assert!(change.persist_result.is_ok());
    let state = dedup.lock().expect("dedup");
    let expected_hash = hash_image_content(&img);
    assert_eq!(state.last_hash.as_deref(), Some(expected_hash.as_str()));
}

#[test]
fn consecutive_accepts_of_same_image_do_not_duplicate() {
    let ctx = TestContext::new();
    let app = Arc::new(TestApp::new());
    let db = Arc::new(ctx.db);
    let claims = Arc::new(DeferredClaimRegistry::new());
    let worker = start_test_worker(
        app.clone(),
        db.clone(),
        ctx.data_dir.clone(),
        0,
        500,
        claims.clone(),
    );
    let dedup = Arc::new(std::sync::Mutex::new(ImageDedupState {
        last_hash: Some(hash_image_content(&solid_image(2, 2, 1))),
    }));
    let img = solid_image(2, 2, 2);

    let first = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("first accept");
    let second = accept_image_clipboard_change(
        &app,
        &db,
        &ctx.data_dir,
        &worker,
        &claims,
        &img,
        "Photos",
        &dedup,
        0,
        500,
    )
    .expect("second accept");

    assert!(first.is_some());
    assert!(second.is_none());
    assert_eq!(
        app.captured_event::<ClipboardListItem>(EVENT_STREAM_ITEM_ADDED)
            .len(),
        1
    );
}
