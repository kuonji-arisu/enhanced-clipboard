use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{info, warn};

use crate::db::Database;
use crate::models::{ClipboardQueryStaleReason, EntryStatus};
use crate::services::artifacts::{image, store};
use crate::services::effects::{apply_pipeline_effects, PipelineEffects};
use crate::services::image_ingest;
use crate::services::view_events::EventEmitter;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ArtifactMaintenanceSummary {
    pub rebuilt_previews: Vec<String>,
}

#[derive(Debug, Default)]
pub struct MaintenancePlan {
    pub effects: PipelineEffects,
    pub summary: ArtifactMaintenanceSummary,
}

#[derive(Debug, Clone, Copy)]
pub struct ArtifactMaintenanceOptions {
    pub max_repairs: usize,
}

impl Default for ArtifactMaintenanceOptions {
    fn default() -> Self {
        Self { max_repairs: 32 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ArtifactMaintenanceScheduler {
    state: Arc<ArtifactMaintenanceSchedulerState>,
}

#[derive(Debug, Default)]
struct ArtifactMaintenanceSchedulerState {
    running: AtomicBool,
    rerun_requested: AtomicBool,
}

impl ArtifactMaintenanceScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start<A>(&self, app: A, db: Arc<Database>, data_dir: PathBuf) -> bool
    where
        A: EventEmitter + Clone + Send + 'static,
    {
        if self.state.running.swap(true, Ordering::AcqRel) {
            self.state.rerun_requested.store(true, Ordering::Release);
            return false;
        }

        spawn_artifact_maintenance_worker(self.state.clone(), app, db, data_dir);
        true
    }
}

fn spawn_artifact_maintenance_worker<A>(
    state: Arc<ArtifactMaintenanceSchedulerState>,
    app: A,
    db: Arc<Database>,
    data_dir: PathBuf,
) where
    A: EventEmitter + Clone + Send + 'static,
{
    std::thread::spawn(move || {
        loop {
            if let Err(err) = run_artifact_maintenance_once(
                &app,
                &db,
                &data_dir,
                ArtifactMaintenanceOptions::default(),
            ) {
                warn!("Failed to run artifact maintenance: {}", err);
            }

            if !state.rerun_requested.swap(false, Ordering::AcqRel) {
                break;
            }
        }

        state.running.store(false, Ordering::Release);
        if state.rerun_requested.swap(false, Ordering::AcqRel)
            && !state.running.swap(true, Ordering::AcqRel)
        {
            spawn_artifact_maintenance_worker(state, app, db, data_dir);
        }
    });
}

pub fn run_artifact_maintenance_once<A>(
    app: &A,
    db: &Database,
    data_dir: &Path,
    options: ArtifactMaintenanceOptions,
) -> Result<ArtifactMaintenanceSummary, String>
where
    A: EventEmitter,
{
    let plan = run_artifact_maintenance_core(db, data_dir, options)?;
    let report = apply_pipeline_effects(app, db, data_dir, plan.effects);
    for error in report.event_errors {
        warn!("Post-commit artifact maintenance effect warning: {}", error);
    }
    if !plan.summary.rebuilt_previews.is_empty() {
        info!(
            "Completed artifact maintenance: rebuilt_previews={}",
            plan.summary.rebuilt_previews.len()
        );
    }

    Ok(plan.summary)
}

/// Performs maintenance decisions and DB repair writes, but leaves view events
/// and DB-backed file cleanup to the shared effects applier.
pub fn run_artifact_maintenance_core(
    db: &Database,
    data_dir: &Path,
    options: ArtifactMaintenanceOptions,
) -> Result<MaintenancePlan, String> {
    store::ensure_artifact_dirs(data_dir)?;
    let records = db.get_image_asset_records()?;
    let mut effects = PipelineEffects::default();
    let mut rebuilt_previews = Vec::new();
    let mut repairs = 0usize;

    for record in &records {
        if repairs >= options.max_repairs {
            break;
        }
        if record.status != EntryStatus::Ready {
            continue;
        }
        let preview_missing = record
            .preview_path
            .as_deref()
            .and_then(|path| store::validate_relative_path(data_dir, path))
            .is_none_or(|path| !path.exists());
        if !preview_missing {
            continue;
        }

        let Some(original_rel) = record.original_path.as_deref() else {
            remove_ready_image_record(db, &mut effects, &record.id)?;
            repairs += 1;
            continue;
        };

        match image::rebuild_preview_artifact(data_dir, &record.id, original_rel) {
            Ok(outcome) => {
                if let Some(old_path) = db.replace_artifact(&record.id, &outcome.artifact)? {
                    effects.cleanup_paths.push(old_path);
                }
                effects
                    .cleanup_paths
                    .extend(outcome.old_candidate_paths.into_iter().filter(|path| {
                        path != &outcome.artifact.rel_path
                            && Some(path.as_str()) != record.preview_path.as_deref()
                    }));
                if let Some(entry) = db.get_entry_by_id(&record.id)? {
                    effects.updated.push(entry);
                }
                rebuilt_previews.push(record.id.clone());
                repairs += 1;
            }
            Err(image::RebuildPreviewError::OriginalMissing)
            | Err(image::RebuildPreviewError::OriginalBroken(_)) => {
                remove_ready_image_record(db, &mut effects, &record.id)?;
                repairs += 1;
            }
            Err(image::RebuildPreviewError::PreviewWrite(err)) => {
                warn!(
                    "Failed to rebuild preview artifact for image entry {}: {}",
                    record.id, err
                );
            }
        }
    }

    Ok(MaintenancePlan {
        effects,
        summary: ArtifactMaintenanceSummary { rebuilt_previews },
    })
}

fn remove_ready_image_record(
    db: &Database,
    effects: &mut PipelineEffects,
    entry_id: &str,
) -> Result<(), String> {
    if let Some(cleanup) = db.delete_entry_with_job_cleanup(entry_id)? {
        let plan = image_ingest::cleanup_plan_from_entry_removal(cleanup);
        effects.removed_ids.extend(plan.removed_ids);
        effects.cleanup_paths.extend(plan.cleanup_paths);
        effects.stale_reason = Some(ClipboardQueryStaleReason::SettingsOrStartup);
    }
    Ok(())
}
