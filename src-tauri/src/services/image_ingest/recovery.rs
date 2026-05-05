use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::db::Database;
use crate::services::artifacts::store;
use crate::services::image_ingest::staging;
use crate::services::jobs::ImageDedupState;
use crate::services::image_ingest::sweeper;
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
    image_dedup: &Arc<Mutex<ImageDedupState>>,
) -> Result<StartupRecovery, String> {
    staging::ensure_dirs(data_dir)?;
    let requeued_running = db.requeue_running_image_ingest_jobs()?;
    let sweep_summary = sweeper::run_full_convergence(
        app,
        db,
        data_dir,
        store::ORPHAN_FILE_PROTECTION_WINDOW,
        image_dedup,
    )?;
    Ok(StartupRecovery {
        requeued_running,
        removed_ids: sweep_summary.removed_ids,
        cleanup_paths: sweep_summary.cleanup_paths,
    })
}
