use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use log::{debug, error, warn};
use uuid::Uuid;

use crate::db::image_ingest_jobs::ImageIngestJobDraft;
use crate::db::{Database, EntryJobCleanup, JobFinalizeOutcome};
use crate::models::{
    ClipboardEntry, ClipboardJob, ClipboardJobKind, ClipboardJobStatus, ClipboardQueryStaleReason,
    EntryStatus,
};
use crate::services::artifacts::{image, store};
use crate::services::effects::PipelineEffects;
use crate::services::jobs::{
    clear_polling_image_dedup_if_current, ContentJobWorker, ImageDedupState,
};
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

pub mod staging;

pub const MAX_ACTIVE_IMAGE_INGEST_JOBS: i64 = 3;
pub const MAX_ACTIVE_IMAGE_STAGING_BYTES: i64 = 300 * 1024 * 1024;
pub const MAX_IMAGE_INGEST_ATTEMPTS: i64 = 2;

pub struct CaptureImageDeps<'a, A> {
    pub app_handle: &'a A,
    pub db: &'a Arc<Database>,
    pub data_dir: &'a Path,
    pub worker: &'a ContentJobWorker,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupPlan {
    pub removed_ids: Vec<String>,
    pub cleanup_paths: Vec<String>,
    pub dedup_keys: Vec<String>,
}

impl CleanupPlan {
    pub fn is_empty(&self) -> bool {
        self.removed_ids.is_empty() && self.cleanup_paths.is_empty() && self.dedup_keys.is_empty()
    }

    pub fn clear_polling_dedup(&self, image_dedup: &Arc<Mutex<ImageDedupState>>) {
        for dedup_key in &self.dedup_keys {
            clear_polling_image_dedup_if_current(image_dedup, dedup_key);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupRecovery {
    pub requeued_running: usize,
    pub removed_ids: Vec<String>,
}

pub fn ensure_staging_dirs(data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir.join("staging").join("image_ingest"))
        .map_err(|e| e.to_string())
}

pub fn capture_image<A>(
    deps: CaptureImageDeps<'_, A>,
    img: &arboard::ImageData,
    source_app: String,
    image_dedup: Arc<Mutex<ImageDedupState>>,
    content_hash: String,
) -> Result<(), String>
where
    A: EventEmitter + Clone + Send + 'static,
{
    let id = Uuid::new_v4().to_string();
    let job_id = Uuid::new_v4().to_string();
    let width = img.width as u32;
    let height = img.height as u32;
    let byte_size = match staging::expected_rgba8_byte_size(width, height) {
        Ok(byte_size) => byte_size,
        Err(err) => {
            clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
            return Err(err);
        }
    };

    let backlog = match deps.db.image_ingest_backlog() {
        Ok(backlog) => backlog,
        Err(err) => {
            clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
            return Err(err);
        }
    };
    if backlog.count >= MAX_ACTIVE_IMAGE_INGEST_JOBS
        || backlog.byte_size.saturating_add(byte_size) > MAX_ACTIVE_IMAGE_STAGING_BYTES
    {
        clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
        return Err("Active image ingest backlog is full".to_string());
    }

    let entry = ClipboardEntry {
        id: id.clone(),
        content_type: "image".to_string(),
        status: EntryStatus::Pending,
        content: String::new(),
        canonical_search_text: String::new(),
        tags: Vec::new(),
        created_at: Utc::now().timestamp(),
        is_pinned: false,
        source_app,
    };
    debug!(
        "Queued image entry: id={}, width={}, height={}",
        entry.id, width, height
    );

    let input_ref = staging::input_rel_path(&job_id);
    if let Err(err) =
        staging::write_rgba8(deps.data_dir, &input_ref, img.bytes.as_ref(), width, height)
    {
        clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
        return Err(err);
    }

    let job = ImageIngestJobDraft {
        id: job_id,
        entry_id: id,
        input_ref: input_ref.clone(),
        dedup_key: content_hash.clone(),
        created_at: entry.created_at,
        width: i64::from(width),
        height: i64::from(height),
        pixel_format: staging::PIXEL_FORMAT_RGBA8.to_string(),
        byte_size,
        content_hash: content_hash.clone(),
    };

    if let Err(err) = deps.db.insert_pending_image_entry_with_job(
        &entry,
        &job,
        MAX_ACTIVE_IMAGE_INGEST_JOBS,
        MAX_ACTIVE_IMAGE_STAGING_BYTES,
    ) {
        store::cleanup_relative_paths(deps.data_dir, vec![input_ref]);
        clear_polling_image_dedup_if_current(&image_dedup, &content_hash);
        return Err(err);
    }

    pipeline::emit_pending_entry_added(deps.app_handle, deps.db, deps.data_dir, &entry)?;
    if let Err(err) = deps.worker.wake() {
        warn!(
            "Image ingest job was queued but worker wake failed: {}",
            err
        );
    }

    Ok(())
}

pub fn run_next_job(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    expiry_seconds: i64,
    max_history: u32,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
) -> Result<bool, String> {
    let Some(job) = db.claim_next_image_ingest_job()? else {
        return Ok(false);
    };
    if job.kind != ClipboardJobKind::ImageIngest {
        warn!("Ignoring unsupported deferred job kind: {:?}", job.kind);
        return Ok(true);
    }
    run_claimed_job(
        app,
        db,
        data_dir,
        expiry_seconds,
        max_history,
        image_dedup,
        job,
    )?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub fn run_claimed_job(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    expiry_seconds: i64,
    max_history: u32,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
    job: ClipboardJob,
) -> Result<(), String> {
    let Some(width) = job.width else {
        return terminalize_running_job(
            app,
            db,
            data_dir,
            image_dedup,
            &job,
            Vec::new(),
            "Image ingest job is missing width metadata".to_string(),
        );
    };
    let Some(height) = job.height else {
        return terminalize_running_job(
            app,
            db,
            data_dir,
            image_dedup,
            &job,
            Vec::new(),
            "Image ingest job is missing height metadata".to_string(),
        );
    };
    let rgba = match staging::read_rgba8(
        data_dir,
        &job.input_ref,
        width,
        height,
        job.pixel_format.as_deref(),
        job.byte_size,
    ) {
        Ok(rgba) => rgba,
        Err(err) => {
            warn!(
                "Image ingest staging input is unrecoverable for job {} entry {}: {}",
                job.id, job.entry_id, err
            );
            return terminalize_running_job(app, db, data_dir, image_dedup, &job, Vec::new(), err);
        }
    };

    let artifacts = match image::write_image_artifacts(
        data_dir,
        &job.entry_id,
        &rgba,
        width as u32,
        height as u32,
    ) {
        Ok(outcome) => outcome.artifacts,
        Err(err) => {
            error!(
                "Image ingest artifact generation failed for job {} entry {}: {}",
                job.id, job.entry_id, err
            );
            return handle_retryable_running_job_failure(
                app,
                db,
                data_dir,
                image_dedup,
                &job,
                generated_cleanup_paths_for_job(&job),
                err,
            );
        }
    };

    match db.finalize_running_image_ingest_job(&job.id, &artifacts) {
        Ok(JobFinalizeOutcome::Ready(entry)) => pipeline::finish_ready_entry_update(
            app,
            db,
            data_dir,
            pipeline::ReadyEntryUpdate {
                entry,
                cleanup_paths: staging_cleanup_path_for_job(&job),
                expiry_seconds,
                max_history,
                context: "finalize image ingest job",
            },
        ),
        Ok(JobFinalizeOutcome::Skipped) => {
            debug!(
                "Image ingest job {} was canceled or entry disappeared before finalize",
                job.id
            );
            finish_job_result(
                app,
                db,
                data_dir,
                PipelineEffects {
                    cleanup_paths: generated_cleanup_paths_for_job(&job)
                        .into_iter()
                        .chain(staging_cleanup_path_for_job(&job))
                        .collect(),
                    ..PipelineEffects::default()
                },
                None,
            )
        }
        Err(err) => {
            warn!(
                "Failed to commit image ingest job {} entry {}; applying retry policy: {}",
                job.id, job.entry_id, err
            );
            handle_retryable_running_job_failure(
                app,
                db,
                data_dir,
                image_dedup,
                &job,
                generated_cleanup_paths_for_job(&job),
                err,
            )
        }
    }
}

pub fn recover_startup(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
) -> Result<StartupRecovery, String> {
    ensure_staging_dirs(data_dir)?;
    let (summary, effects) = plan_startup_recovery(db, data_dir)?;
    pipeline::apply_effects(app, db, data_dir, effects, "startup image ingest recovery");
    Ok(summary)
}

pub fn plan_startup_recovery(
    db: &Database,
    data_dir: &Path,
) -> Result<(StartupRecovery, PipelineEffects), String> {
    let requeued_running = db.requeue_running_image_ingest_jobs()?;
    let active_jobs = db.get_active_image_ingest_jobs()?;
    let mut remove_ids = Vec::new();

    for job in &active_jobs {
        if !is_recoverable_image_ingest_job(job) {
            continue;
        }
        if !staging_input_exists(data_dir, job) {
            remove_ids.push(job.entry_id.clone());
        }
    }

    remove_ids.extend(db.get_pending_image_entries_without_active_job()?);
    remove_ids.sort();
    remove_ids.dedup();

    let plan = cancel_entries(db, &remove_ids)?;
    let _ = db.cleanup_terminal_image_ingest_jobs()?;
    let effects = PipelineEffects {
        removed_ids: plan.removed_ids.clone(),
        cleanup_paths: plan.cleanup_paths,
        stale_reason: (!plan.removed_ids.is_empty())
            .then_some(ClipboardQueryStaleReason::SettingsOrStartup),
        ..PipelineEffects::default()
    };
    Ok((
        StartupRecovery {
            requeued_running,
            removed_ids: plan.removed_ids,
        },
        effects,
    ))
}

pub fn cancel_entry(db: &Database, id: &str) -> Result<Option<CleanupPlan>, String> {
    db.delete_entry_with_job_cleanup(id)
        .map(|cleanup| cleanup.map(cleanup_plan_from_db))
}

pub fn cancel_entries(db: &Database, ids: &[String]) -> Result<CleanupPlan, String> {
    db.delete_entries_with_job_cleanup(ids)
        .map(cleanup_plan_from_db)
}

pub fn cancel_all(db: &Database) -> Result<CleanupPlan, String> {
    db.clear_all_with_job_cleanup().map(cleanup_plan_from_db)
}

fn handle_retryable_running_job_failure(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
    job: &ClipboardJob,
    cleanup_paths: Vec<String>,
    error: String,
) -> Result<(), String> {
    if job.attempts < MAX_IMAGE_INGEST_ATTEMPTS {
        cleanup_uncommitted_retry_files(data_dir, cleanup_paths);
        db.requeue_running_image_ingest_job(&job.id, &error)?;
        return finish_job_result(app, db, data_dir, PipelineEffects::default(), Some(error));
    }

    terminalize_running_job(app, db, data_dir, image_dedup, job, cleanup_paths, error)
}

fn terminalize_running_job(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
    job: &ClipboardJob,
    mut cleanup_paths: Vec<String>,
    error: String,
) -> Result<(), String> {
    let cleanup = db.fail_running_job_and_delete_pending_entry(&job.id, &error)?;
    if let Some(cleanup) = cleanup {
        let mut plan = cleanup_plan_from_db(cleanup);
        cleanup_paths.append(&mut plan.cleanup_paths);
        plan.clear_polling_dedup(image_dedup);
        finish_job_result(
            app,
            db,
            data_dir,
            PipelineEffects {
                removed_ids: plan.removed_ids,
                cleanup_paths,
                stale_reason: Some(ClipboardQueryStaleReason::EntriesRemoved),
                ..PipelineEffects::default()
            },
            Some(error),
        )
    } else {
        cleanup_paths.extend(generated_cleanup_paths_for_job(job));
        finish_job_result(
            app,
            db,
            data_dir,
            PipelineEffects {
                cleanup_paths,
                ..PipelineEffects::default()
            },
            None,
        )
    }
}

fn cleanup_plan_from_db(mut cleanup: EntryJobCleanup) -> CleanupPlan {
    let mut cleanup_paths = Vec::new();
    cleanup_paths.append(&mut cleanup.artifact_paths);

    let mut seen_paths = HashSet::new();
    let mut dedup_keys = Vec::new();
    for job in cleanup.active_jobs {
        if !job.input_ref.is_empty() && seen_paths.insert(job.input_ref.clone()) {
            cleanup_paths.push(job.input_ref);
        }
        if !job.dedup_key.is_empty() {
            dedup_keys.push(job.dedup_key.clone());
        }
        for path in image::generated_candidate_paths(&job.entry_id) {
            if seen_paths.insert(path.clone()) {
                cleanup_paths.push(path);
            }
        }
    }

    CleanupPlan {
        removed_ids: cleanup.removed_ids,
        cleanup_paths,
        dedup_keys,
    }
}

fn generated_cleanup_paths_for_job(job: &ClipboardJob) -> Vec<String> {
    match job.kind {
        ClipboardJobKind::ImageIngest => image::generated_candidate_paths(&job.entry_id),
        _ => Vec::new(),
    }
}

fn staging_cleanup_path_for_job(job: &ClipboardJob) -> Vec<String> {
    if job.input_ref.is_empty() {
        Vec::new()
    } else {
        vec![job.input_ref.clone()]
    }
}

fn staging_input_exists(data_dir: &Path, job: &ClipboardJob) -> bool {
    store::validate_cleanup_relative_path(data_dir, &job.input_ref)
        .is_some_and(|path| path.exists())
}

fn is_recoverable_image_ingest_job(job: &ClipboardJob) -> bool {
    job.kind == ClipboardJobKind::ImageIngest
        && matches!(
            job.status,
            ClipboardJobStatus::Queued | ClipboardJobStatus::Running
        )
}

fn cleanup_uncommitted_retry_files(data_dir: &Path, cleanup_paths: Vec<String>) {
    if cleanup_paths.is_empty() {
        return;
    }
    store::cleanup_relative_paths(data_dir, cleanup_paths);
}

fn finish_job_result(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    effects: PipelineEffects,
    terminal_error: Option<String>,
) -> Result<(), String> {
    pipeline::apply_effects(app, db, data_dir, effects, "image ingest job");
    if let Some(err) = terminal_error {
        Err(err)
    } else {
        Ok(())
    }
}
