use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use log::{info, warn};

use crate::db::Database;
use crate::models::{
    ClipboardJob, ClipboardJobKind, ClipboardJobStatus, ClipboardQueryStaleReason,
};
use crate::services::image_ingest::CleanupPlan;
use crate::services::effects::PipelineEffects;
use crate::services::image_ingest::cleanup::{
    cancel_entries, cleanup_terminal_jobs, plan_staging_orphan_cleanup, staging_input_exists,
};
use crate::services::jobs::ImageDedupState;
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageIngestSweepSummary {
    pub removed_ids: Vec<String>,
    pub cleanup_paths: usize,
}

pub fn run_once(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    protection_window: Duration,
    image_dedup: Option<&Arc<Mutex<ImageDedupState>>>,
) -> Result<ImageIngestSweepSummary, String> {
    let cleanup = converge_db_and_plan_cleanup(db, data_dir, protection_window)?;
    if let Some(image_dedup) = image_dedup {
        cleanup.clear_polling_dedup(image_dedup);
    }
    let summary = ImageIngestSweepSummary {
        removed_ids: cleanup.removed_ids.clone(),
        cleanup_paths: cleanup.cleanup_paths.len(),
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
        "image ingest sweep",
    );
    Ok(summary)
}

/// Performs image-ingest convergence DB mutations and returns post-DB cleanup.
///
/// This is not a dry run. It may delete inconsistent pending image entries and
/// terminal image-ingest job rows before returning `CleanupPlan`.
/// Callers must apply removed/file cleanup through the shared effects path.
/// Callers with `ImageDedupState` must clear the returned dedup keys.
pub fn converge_db_and_plan_cleanup(
    db: &Database,
    data_dir: &Path,
    protection_window: Duration,
) -> Result<CleanupPlan, String> {
    let mut remove_ids = entries_to_remove_for_inconsistent_jobs(db, data_dir)?;
    remove_ids.extend(db.get_pending_image_entries_without_active_job()?);
    remove_ids.sort();
    remove_ids.dedup();

    let mut cleanup = cancel_entries(db, &remove_ids)?;
    cleanup.cleanup_paths.extend(cleanup_terminal_jobs(db)?);
    cleanup.cleanup_paths.extend(plan_staging_orphan_cleanup(
        db,
        data_dir,
        protection_window,
    )?);
    let mut seen_paths = HashSet::new();
    cleanup
        .cleanup_paths
        .retain(|path| seen_paths.insert(path.clone()));
    let mut seen_dedup = HashSet::new();
    cleanup
        .dedup_keys
        .retain(|key| seen_dedup.insert(key.clone()));
    Ok(cleanup)
}

pub fn schedule_delayed<A>(
    app: A,
    db: Arc<Database>,
    data_dir: PathBuf,
    image_dedup: Option<Arc<Mutex<ImageDedupState>>>,
    delay: Duration,
)
where
    A: EventEmitter + Clone + Send + 'static,
{
    std::thread::spawn(move || {
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        match run_once(
            &app,
            &db,
            &data_dir,
            crate::services::artifacts::store::ORPHAN_FILE_PROTECTION_WINDOW,
            image_dedup.as_ref(),
        ) {
            Ok(summary) => {
                if !summary.removed_ids.is_empty() || summary.cleanup_paths > 0 {
                    info!(
                        "Completed delayed image ingest sweep: removed_entries={}, cleanup_paths={}",
                        summary.removed_ids.len(),
                        summary.cleanup_paths
                    );
                }
            }
            Err(err) => warn!("Failed to run delayed image ingest sweep: {}", err),
        }
    });
}

fn entries_to_remove_for_inconsistent_jobs(
    db: &Database,
    data_dir: &Path,
) -> Result<Vec<String>, String> {
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
    Ok(remove_ids)
}

fn is_recoverable_image_ingest_job(job: &ClipboardJob) -> bool {
    job.kind == ClipboardJobKind::ImageIngest
        && matches!(
            job.status,
            ClipboardJobStatus::Queued | ClipboardJobStatus::Running
        )
}
