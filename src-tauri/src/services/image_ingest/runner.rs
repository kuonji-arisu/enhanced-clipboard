use std::path::Path;
use std::sync::{Arc, Mutex};

use log::{debug, error, warn};

use crate::db::{Database, JobFinalizeOutcome};
use crate::models::{ClipboardJob, ClipboardJobKind, ClipboardQueryStaleReason};
use crate::services::artifacts::image;
use crate::services::effects::PipelineEffects;
use crate::services::image_ingest::cleanup::{
    cleanup_plan_from_db, cleanup_uncommitted_retry_files, generated_cleanup_paths_for_job,
    staging_cleanup_path_for_job,
};
use crate::services::image_ingest::{staging, MAX_IMAGE_INGEST_ATTEMPTS};
use crate::services::jobs::ImageDedupState;
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

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

fn finish_job_result(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    effects: PipelineEffects,
    handled_error: Option<String>,
) -> Result<(), String> {
    pipeline::apply_effects(app, db, data_dir, effects, "image ingest job");
    if let Some(err) = handled_error {
        warn!("Handled image ingest job failure: {}", err);
    }
    Ok(())
}
