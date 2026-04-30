use std::path::Path;

use crate::db::Database;
use crate::services::artifacts::store;
use crate::services::image_ingest::staging;
use crate::services::image_ingest::sweeper::{self, ImageIngestSweepSummary};
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupRecovery {
    pub requeued_running: usize,
    pub removed_ids: Vec<String>,
    pub cleanup_paths: usize,
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
) -> Result<(StartupRecovery, crate::services::effects::PipelineEffects), String> {
    let requeued_running = db.requeue_running_image_ingest_jobs()?;
    let (summary, effects) =
        sweeper::plan_once(db, data_dir, store::ORPHAN_FILE_PROTECTION_WINDOW)?;
    Ok((
        startup_recovery_from_sweep(requeued_running, summary),
        effects,
    ))
}

fn startup_recovery_from_sweep(
    requeued_running: usize,
    summary: ImageIngestSweepSummary,
) -> StartupRecovery {
    StartupRecovery {
        requeued_running,
        removed_ids: summary.removed_ids,
        cleanup_paths: summary.cleanup_paths,
    }
}
