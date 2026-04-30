use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use log::{info, warn};

use crate::db::Database;
use crate::models::{
    ClipboardJob, ClipboardJobKind, ClipboardJobStatus, ClipboardQueryStaleReason,
};
use crate::services::effects::PipelineEffects;
use crate::services::image_ingest::cleanup::{
    cancel_entries, cleanup_terminal_jobs, plan_staging_orphan_cleanup, staging_input_exists,
};
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
) -> Result<ImageIngestSweepSummary, String> {
    let (summary, effects) = plan_once(db, data_dir, protection_window)?;
    pipeline::apply_effects(app, db, data_dir, effects, "image ingest sweep");
    Ok(summary)
}

pub fn plan_once(
    db: &Database,
    data_dir: &Path,
    protection_window: Duration,
) -> Result<(ImageIngestSweepSummary, PipelineEffects), String> {
    let mut remove_ids = entries_to_remove_for_inconsistent_jobs(db, data_dir)?;
    remove_ids.extend(db.get_pending_image_entries_without_active_job()?);
    remove_ids.sort();
    remove_ids.dedup();

    let plan = cancel_entries(db, &remove_ids)?;
    let mut cleanup_paths = plan.cleanup_paths;
    cleanup_paths.extend(cleanup_terminal_jobs(db)?);
    cleanup_paths.extend(plan_staging_orphan_cleanup(
        db,
        data_dir,
        protection_window,
    )?);
    let cleanup_path_count = cleanup_paths.len();
    let removed_ids = plan.removed_ids;
    let effects = PipelineEffects {
        removed_ids: removed_ids.clone(),
        cleanup_paths,
        stale_reason: (!removed_ids.is_empty())
            .then_some(ClipboardQueryStaleReason::SettingsOrStartup),
        ..PipelineEffects::default()
    };
    Ok((
        ImageIngestSweepSummary {
            removed_ids,
            cleanup_paths: cleanup_path_count,
        },
        effects,
    ))
}

pub fn schedule_delayed<A>(app: A, db: Arc<Database>, data_dir: PathBuf, delay: Duration)
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
