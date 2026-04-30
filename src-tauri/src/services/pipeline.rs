use std::path::Path;
use std::sync::{Arc, Mutex};

use log::{debug, error, warn};

use crate::db::{Database, JobFinalizeOutcome};
use crate::models::{
    ClipboardEntry, ClipboardJob, ClipboardJobKind, ClipboardQueryStaleReason, ImageIngestJobDraft,
};
use crate::services::artifacts::{image, store};
use crate::services::effects::{apply_pipeline_effects, EffectApplyReport, PipelineEffects};
use crate::services::jobs::{
    cleanup_plan_from_db, generated_cleanup_paths_for_job, staging_cleanup_path_for_job,
    ImageDedupState, MAX_IMAGE_INGEST_ATTEMPTS,
};
use crate::services::prune;
use crate::services::view_events::EventEmitter;

pub fn insert_ready_entry(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    entry: &ClipboardEntry,
    attrs: &[(&str, &[String])],
    expiry_seconds: i64,
    max_history: u32,
) -> Result<(), String> {
    prune::prepare_for_immediate_ready_insert(app, db, data_dir, expiry_seconds, max_history)?;
    if attrs.is_empty() {
        db.insert_entry(entry)?;
    } else {
        db.insert_entry_with_attrs(entry, attrs)?;
    }
    let mut effects = PipelineEffects {
        added: vec![entry.clone()],
        stale_reason: Some(ClipboardQueryStaleReason::EntryCreated),
        ..PipelineEffects::default()
    };
    let retention_error = match prune::apply_retention_after_ready_change(
        db,
        expiry_seconds,
        max_history,
        ClipboardQueryStaleReason::BeforeInsert,
    ) {
        Ok(retention_effects) => {
            effects.merge(retention_effects);
            None
        }
        Err(err) => {
            warn!(
                "Retention failed after ready insert for entry {}: {}",
                entry.id, err
            );
            Some(err)
        }
    };
    log_effect_warnings(
        "insert ready entry",
        apply_pipeline_effects(app, db, data_dir, effects),
    );
    if let Some(err) = retention_error {
        Err(err)
    } else {
        Ok(())
    }
}

pub fn insert_pending_image_ingest_job(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    entry: &ClipboardEntry,
    job: &ImageIngestJobDraft,
    max_active_jobs: i64,
    max_active_bytes: i64,
) -> Result<(), String> {
    db.insert_pending_image_entry_with_job(entry, job, max_active_jobs, max_active_bytes)?;

    let report = apply_pipeline_effects(
        app,
        db,
        data_dir,
        PipelineEffects {
            added: vec![entry.clone()],
            stale_reason: Some(ClipboardQueryStaleReason::EntryCreated),
            ..PipelineEffects::default()
        },
    );
    log_effect_warnings("insert pending entry", report);
    Ok(())
}

pub fn run_claimed_deferred_job(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    expiry_seconds: i64,
    max_history: u32,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
    job: ClipboardJob,
) -> Result<(), String> {
    match job.kind {
        ClipboardJobKind::ImageIngest => run_claimed_image_ingest_job(
            app,
            db,
            data_dir,
            expiry_seconds,
            max_history,
            image_dedup,
            job,
        ),
        kind => {
            warn!("Ignoring unsupported deferred job kind: {:?}", kind);
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_claimed_image_ingest_job(
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
    let rgba = match image::read_staging_rgba8(
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

    let finalize = db.finalize_running_image_ingest_job(&job.id, &artifacts);
    match finalize {
        Ok(JobFinalizeOutcome::Ready(entry)) => {
            let mut effects = PipelineEffects {
                cleanup_paths: staging_cleanup_path_for_job(&job),
                ..PipelineEffects::default()
            };
            match prune::apply_retention_after_ready_change(
                db,
                expiry_seconds,
                max_history,
                ClipboardQueryStaleReason::BeforeInsert,
            ) {
                Ok(retention_effects) => effects.merge(retention_effects),
                Err(err) => {
                    warn!(
                        "Retention failed after image ingest finalize for entry {}: {}",
                        entry.id, err
                    );
                    effects.updated.push(entry);
                    return finish_deferred_result(app, db, data_dir, effects, Some(err));
                }
            }
            if effects.removed_ids.iter().any(|id| id == &entry.id) {
                debug!(
                    "Image ingest entry {} was removed by retention after finalize",
                    entry.id
                );
            } else {
                effects.updated.push(entry);
            }
            finish_deferred_result(app, db, data_dir, effects, None)
        }
        Ok(JobFinalizeOutcome::Skipped) => {
            debug!(
                "Image ingest job {} was canceled or entry disappeared before finalize",
                job.id
            );
            finish_deferred_result(
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
        db.requeue_running_job(&job.id, &error)?;
        return finish_deferred_result(app, db, data_dir, PipelineEffects::default(), Some(error));
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
        finish_deferred_result(
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
        finish_deferred_result(
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

fn cleanup_uncommitted_retry_files(data_dir: &Path, cleanup_paths: Vec<String>) {
    if cleanup_paths.is_empty() {
        return;
    }
    store::cleanup_relative_paths(data_dir, cleanup_paths);
}

fn finish_deferred_result(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    effects: PipelineEffects,
    terminal_error: Option<String>,
) -> Result<(), String> {
    log_effect_warnings(
        "finalize deferred content",
        apply_pipeline_effects(app, db, data_dir, effects),
    );
    if let Some(err) = terminal_error {
        Err(err)
    } else {
        Ok(())
    }
}

fn log_effect_warnings(context: &str, report: EffectApplyReport) {
    for error in report.event_errors {
        warn!("Post-commit effect warning during {}: {}", context, error);
    }
}
