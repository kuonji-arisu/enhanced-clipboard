use enhanced_clipboard_lib::db::JobFinalizeOutcome;
use enhanced_clipboard_lib::models::{
    ClipboardContentType, ClipboardImagePreviewMode, ClipboardPreview, EntryStatus,
};
use enhanced_clipboard_lib::services::image_ingest::{self, staging};

mod common;

use common::{
    image_entry, image_original_path, image_preview_path, insert_entry,
    insert_pending_image_with_job, open_raw_clipboard_conn, pending_image_entry, text_entry,
    touch_file, TestApp, TestContext,
};

#[test]
fn pending_image_insert_creates_active_ingest_job_without_status_history() {
    let ctx = TestContext::new();
    insert_pending_image_with_job(&ctx, "pending", 10);

    let entry = ctx
        .db
        .get_entry_by_id("pending")
        .expect("entry lookup")
        .expect("pending entry");
    assert_eq!(entry.status, EntryStatus::Pending);

    let jobs = ctx.db.get_active_image_ingest_jobs().expect("active jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].entry_id, "pending");
    assert_eq!(jobs[0].kind.as_str(), "image_ingest");
    assert!(!jobs[0].input_ref.is_empty());
    assert!(jobs[0].image_ingest_payload().is_ok());
}

#[test]
fn worker_finalizes_active_job_and_removes_job_row() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_pending_image_with_job(&ctx, "pending", 10);

    assert!(
        image_ingest::run_next_job(&app, &ctx.db, &ctx.data_dir, 0, 500, &ctx.claims)
            .expect("run job")
    );

    let entry = ctx
        .db
        .get_entry_by_id("pending")
        .expect("entry lookup")
        .expect("ready entry");
    assert_eq!(entry.status, EntryStatus::Ready);
    assert!(ctx
        .db
        .get_active_image_ingest_jobs()
        .expect("active jobs")
        .is_empty());
    assert!(ctx.data_dir.join(image_original_path("pending")).exists());
    assert!(ctx.data_dir.join(image_preview_path("pending")).exists());
}

#[test]
fn finalize_safely_skips_when_pending_entry_disappeared() {
    let ctx = TestContext::new();
    insert_pending_image_with_job(&ctx, "pending", 10);
    ctx.db
        .delete_entry_with_job_cleanup("pending")
        .expect("delete pending")
        .expect("pending existed");

    let outcome = ctx
        .db
        .finalize_active_image_ingest_job("pending", &common::image_artifacts("pending"))
        .expect("finalize missing entry");

    assert!(matches!(outcome, JobFinalizeOutcome::Skipped));
    assert!(ctx
        .db
        .get_active_image_ingest_jobs()
        .expect("active jobs")
        .is_empty());
}

#[test]
fn startup_recovery_only_repairs_pending_job_consistency() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_entry(&ctx, &pending_image_entry("without-job", 10));
    insert_pending_image_with_job(&ctx, "missing-staging", 11);
    insert_entry(&ctx, &image_entry("ready-missing-original", 12));
    touch_file(&ctx, "staging/image_ingest/orphan.rgba");

    let missing_job = ctx
        .db
        .get_active_image_ingest_jobs()
        .expect("active jobs")
        .into_iter()
        .find(|job| job.entry_id == "missing-staging")
        .expect("missing-staging job");
    std::fs::remove_file(ctx.data_dir.join(missing_job.input_ref)).expect("remove staging");

    let summary = image_ingest::recover_startup(&app, &ctx.db, &ctx.data_dir, &ctx.claims)
        .expect("startup recovery");

    assert_eq!(
        summary.removed_ids,
        vec!["missing-staging".to_string(), "without-job".to_string()]
    );
    assert!(ctx
        .db
        .get_entry_by_id("ready-missing-original")
        .expect("ready lookup")
        .is_some());
    assert!(ctx
        .data_dir
        .join("staging/image_ingest/orphan.rgba")
        .exists());
}

#[test]
fn failed_active_job_deletes_pending_entry_without_terminal_job_history() {
    let ctx = TestContext::new();
    let app = TestApp::new();
    insert_pending_image_with_job(&ctx, "pending", 10);
    let job = ctx
        .db
        .get_active_image_ingest_jobs()
        .expect("active jobs")
        .pop()
        .expect("active job");
    std::fs::write(ctx.data_dir.join(job.input_ref), b"wrong-size").expect("break staging");

    image_ingest::run_next_job(&app, &ctx.db, &ctx.data_dir, 0, 500, &ctx.claims)
        .expect("run broken job");

    assert!(ctx
        .db
        .get_entry_by_id("pending")
        .expect("entry lookup")
        .is_none());
    assert!(ctx
        .db
        .get_active_image_ingest_jobs()
        .expect("active jobs")
        .is_empty());
}

#[test]
fn clipboard_jobs_schema_contains_only_active_ingest_fields() {
    let ctx = TestContext::new();
    let conn = open_raw_clipboard_conn(&ctx);
    let mut stmt = conn
        .prepare("PRAGMA table_info(clipboard_jobs)")
        .expect("table info");
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("columns");

    assert_eq!(
        columns,
        vec![
            "entry_id".to_string(),
            "kind".to_string(),
            "created_at".to_string(),
            "input_ref".to_string(),
            "dedup_key".to_string(),
            "payload_json".to_string(),
        ]
    );
    for removed in [
        "id",
        "status",
        "attempts",
        "updated_at",
        "error",
        "width",
        "height",
        "pixel_format",
        "byte_size",
        "content_hash",
    ] {
        assert!(!columns.iter().any(|column| column == removed));
    }
}

#[test]
fn persisted_status_and_projection_repairing_stay_separate() {
    let ctx = TestContext::new();
    let entry = image_entry("repairing", 10);
    touch_file(&ctx, &image_original_path("repairing"));
    insert_entry(&ctx, &entry);

    let stored = ctx
        .db
        .get_entry_by_id("repairing")
        .expect("entry lookup")
        .expect("entry");
    assert_eq!(stored.status, EntryStatus::Ready);

    let item = enhanced_clipboard_lib::services::query::get_list_item_by_id(
        &ctx.db,
        &ctx.data_dir,
        "repairing",
        &Default::default(),
        0,
    )
    .expect("query item")
    .expect("item");
    assert!(matches!(
        item.preview,
        ClipboardPreview::Image {
            mode: ClipboardImagePreviewMode::Repairing
        }
    ));
}

#[test]
fn table_shape_already_allows_future_file_content_and_ingest_kind() {
    let ctx = TestContext::new();
    let mut file_entry = text_entry("file-entry", 10, "");
    file_entry.content_type = ClipboardContentType::File;
    ctx.db.insert_entry(&file_entry).expect("insert file entry");

    let conn = open_raw_clipboard_conn(&ctx);
    conn.execute(
        "INSERT INTO clipboard_jobs
         (entry_id, kind, created_at, input_ref, dedup_key, payload_json)
         VALUES (?1, 'file_ingest', 10, 'files/input.bin', 'file-dedup', '{}')",
        ["file-entry"],
    )
    .expect("insert future file ingest job");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM clipboard_jobs WHERE entry_id = 'file-entry' AND kind = 'file_ingest'",
            [],
            |row| row.get(0),
        )
        .expect("file job count");
    assert_eq!(count, 1);
}

#[test]
fn active_ingest_payload_validates_staging_metadata() {
    let ctx = TestContext::new();
    insert_pending_image_with_job(&ctx, "pending", 10);
    let job = ctx
        .db
        .claim_next_image_ingest_job()
        .expect("claim")
        .expect("job");
    let payload = job.image_ingest_payload().expect("payload");

    let bytes = staging::read_rgba8(
        &ctx.data_dir,
        &job.input_ref,
        i64::from(payload.width),
        i64::from(payload.height),
        Some(payload.pixel_format.as_str()),
        Some(payload.byte_size),
    )
    .expect("read staging");

    assert_eq!(bytes.len() as i64, payload.byte_size);
}
