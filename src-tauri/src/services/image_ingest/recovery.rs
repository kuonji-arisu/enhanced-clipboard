use std::path::Path;

use crate::db::Database;
use crate::models::ClipboardQueryStaleReason;
use crate::services::effects::PipelineEffects;
use crate::services::artifacts::store;
use crate::services::image_ingest::staging;
use crate::services::image_ingest::CleanupPlan;
use crate::services::image_ingest::sweeper;
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
    // Startup recovery converges DB/file state but does not thread process-local
    // polling dedup state through this path.
    let cleanup =
        sweeper::converge_db_and_plan_cleanup(db, data_dir, store::ORPHAN_FILE_PROTECTION_WINDOW)?;
    let summary = startup_recovery_from_cleanup(requeued_running, &cleanup);
    let effects = PipelineEffects {
        removed_ids: cleanup.removed_ids,
        cleanup_paths: cleanup.cleanup_paths,
        stale_reason: (!summary.removed_ids.is_empty())
            .then_some(ClipboardQueryStaleReason::SettingsOrStartup),
        ..PipelineEffects::default()
    };
    Ok((summary, effects))
}

fn startup_recovery_from_cleanup(
    requeued_running: usize,
    cleanup: &CleanupPlan,
) -> StartupRecovery {
    StartupRecovery {
        requeued_running,
        removed_ids: cleanup.removed_ids.clone(),
        cleanup_paths: cleanup.cleanup_paths.len(),
    }
}
