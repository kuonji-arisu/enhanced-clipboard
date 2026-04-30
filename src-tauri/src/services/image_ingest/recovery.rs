use std::path::Path;

use crate::db::Database;
use crate::models::{
    ClipboardJob, ClipboardJobKind, ClipboardJobStatus, ClipboardQueryStaleReason,
};
use crate::services::effects::PipelineEffects;
use crate::services::image_ingest::cleanup::{cancel_entries, staging_input_exists};
use crate::services::image_ingest::staging;
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupRecovery {
    pub requeued_running: usize,
    pub removed_ids: Vec<String>,
}

pub fn recover_startup(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
) -> Result<StartupRecovery, String> {
    staging::ensure_dirs(data_dir)?;
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

fn is_recoverable_image_ingest_job(job: &ClipboardJob) -> bool {
    job.kind == ClipboardJobKind::ImageIngest
        && matches!(
            job.status,
            ClipboardJobStatus::Queued | ClipboardJobStatus::Running
        )
}
