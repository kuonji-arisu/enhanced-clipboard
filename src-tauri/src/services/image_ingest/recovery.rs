use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::db::Database;
use crate::models::ClipboardQueryStaleReason;
use crate::services::effects::PipelineEffects;
use crate::services::image_ingest::cleanup::{
    cancel_entries, staging_cleanup_paths_for_records, staging_input_exists,
};
use crate::services::image_ingest::staging;
use crate::services::jobs::ImageDedupState;
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupRecovery {
    pub removed_ids: Vec<String>,
    pub cleanup_paths: usize,
    pub dangling_jobs_removed: usize,
}

pub fn recover_startup(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
) -> Result<StartupRecovery, String> {
    staging::ensure_dirs(data_dir)?;

    let dangling_jobs = db.delete_dangling_active_jobs()?;
    let mut remove_ids = Vec::new();
    for job in db.get_active_image_ingest_jobs()? {
        if !staging_input_exists(data_dir, &job) {
            remove_ids.push(job.entry_id);
        }
    }
    remove_ids.extend(db.get_pending_image_entries_without_active_job()?);
    remove_ids.sort();
    remove_ids.dedup();

    let mut cleanup = cancel_entries(db, &remove_ids)?;
    cleanup
        .cleanup_paths
        .extend(staging_cleanup_paths_for_records(&dangling_jobs));
    for job in &dangling_jobs {
        if !job.dedup_key.is_empty() {
            cleanup.dedup_keys.push(job.dedup_key.clone());
        }
    }
    cleanup.clear_polling_dedup(image_dedup);
    let summary = StartupRecovery {
        removed_ids: cleanup.removed_ids.clone(),
        cleanup_paths: cleanup.cleanup_paths.len(),
        dangling_jobs_removed: dangling_jobs.len(),
    };
    pipeline::apply_effects(
        app,
        db,
        data_dir,
        PipelineEffects {
            removed_ids: cleanup.removed_ids,
            cleanup_paths: cleanup.cleanup_paths,
            stale_reason: (!summary.removed_ids.is_empty())
                .then_some(ClipboardQueryStaleReason::SettingsOrStartup),
            ..PipelineEffects::default()
        },
        "image ingest startup recovery",
    );
    Ok(StartupRecovery {
        removed_ids: summary.removed_ids,
        cleanup_paths: summary.cleanup_paths,
        dangling_jobs_removed: summary.dangling_jobs_removed,
    })
}
