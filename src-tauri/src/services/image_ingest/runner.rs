use std::path::Path;
use std::sync::{Arc, Mutex};

use log::{debug, error, warn};

use crate::db::{Database, JobFinalizeOutcome};
use crate::models::{ClipboardJob, ClipboardJobKind, ClipboardQueryStaleReason};
use crate::services::artifacts::image;
use crate::services::effects::PipelineEffects;
use crate::services::image_ingest::cleanup::{
    cleanup_plan_from_entry_removal, generated_cleanup_paths_for_job, staging_cleanup_path_for_job,
};
use crate::services::image_ingest::staging;
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
    let payload = match job.image_ingest_payload() {
        Ok(payload) => payload,
        Err(err) => {
            return finish_failed_active_job(app, db, data_dir, image_dedup, &job, Vec::new(), err);
        }
    };
    let rgba = match staging::read_rgba8(
        data_dir,
        &job.input_ref,
        i64::from(payload.width),
        i64::from(payload.height),
        Some(payload.pixel_format.as_str()),
        Some(payload.byte_size),
    ) {
        Ok(rgba) => rgba,
        Err(err) => {
            warn!(
                "Image ingest staging input is unrecoverable for entry {}: {}",
                job.entry_id, err
            );
            return finish_failed_active_job(app, db, data_dir, image_dedup, &job, Vec::new(), err);
        }
    };

    let artifacts = match image::write_image_artifacts(
        data_dir,
        &job.entry_id,
        &rgba,
        payload.width,
        payload.height,
    ) {
        Ok(outcome) => outcome.artifacts,
        Err(err) => {
            error!(
                "Image ingest artifact generation failed for entry {}: {}",
                job.entry_id, err
            );
            return finish_failed_active_job(
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

    match db.finalize_active_image_ingest_job(&job.entry_id, &artifacts) {
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
                "Image ingest job for entry {} disappeared before finalize",
                job.entry_id
            );
            finish_job_result(
                app,
                db,
                data_dir,
                PipelineEffects {
                    cleanup_paths: staging_cleanup_path_for_job(&job),
                    ..PipelineEffects::default()
                },
                None,
            )
        }
        Err(err) => {
            warn!(
                "Failed to commit image ingest job for entry {}; deleting pending entry: {}",
                job.entry_id, err
            );
            finish_failed_active_job(
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

fn finish_failed_active_job(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
    job: &ClipboardJob,
    mut generated_cleanup_paths: Vec<String>,
    error: String,
) -> Result<(), String> {
    let cleanup = db.fail_active_image_ingest_job_and_delete_pending_entry(&job.entry_id)?;
    if let Some(cleanup) = cleanup {
        let mut plan = cleanup_plan_from_entry_removal(cleanup);
        generated_cleanup_paths.append(&mut plan.cleanup_paths);
        plan.clear_polling_dedup(image_dedup);
        finish_job_result(
            app,
            db,
            data_dir,
            PipelineEffects {
                removed_ids: plan.removed_ids,
                cleanup_paths: generated_cleanup_paths,
                stale_reason: Some(ClipboardQueryStaleReason::EntriesRemoved),
                ..PipelineEffects::default()
            },
            Some(error),
        )
    } else {
        finish_job_result(
            app,
            db,
            data_dir,
            PipelineEffects {
                cleanup_paths: staging_cleanup_path_for_job(job),
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
