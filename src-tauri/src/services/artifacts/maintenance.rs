use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{info, warn};

use crate::db::{Database, ImageAssetRecord};
use crate::models::{ClipboardQueryStaleReason, EntryStatus};
use crate::services::artifacts::{image, store};
use crate::services::effects::{apply_pipeline_effects, PipelineEffects};
use crate::services::image_ingest;
use crate::services::view_events::EventEmitter;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StartupImageAssetRepair {
    pub removed_ids: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ArtifactMaintenanceSummary {
    pub rebuilt_displays: Vec<String>,
    pub orphan_files_removed: usize,
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

pub fn run_startup_lightweight_repair(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
) -> Result<StartupImageAssetRepair, String> {
    let (repair, effects) = plan_startup_lightweight_repair(db, data_dir)?;
    let report = apply_pipeline_effects(app, db, data_dir, effects);
    for error in report.event_errors {
        warn!("Post-commit startup repair effect warning: {}", error);
    }
    Ok(repair)
}

pub fn plan_startup_lightweight_repair(
    db: &Database,
    data_dir: &Path,
) -> Result<(StartupImageAssetRepair, PipelineEffects), String> {
    store::ensure_artifact_dirs(data_dir)?;
    let records = db.get_image_asset_records()?;
    let mut remove_ids = Vec::new();
    let mut cleanup_paths = Vec::new();

    for record in &records {
        match startup_repair_action_for_record(data_dir, record) {
            StartupRepairAction::Keep => {}
            StartupRepairAction::KeepDisplayMissingForBackgroundMaintenance => {}
            StartupRepairAction::RemoveMissingOriginal => {
                cleanup_paths.extend(uncommitted_image_candidate_paths(&record.id));
                remove_ids.push(record.id.clone());
            }
        }
    }

    let (removed_ids, db_cleanup_paths) = db.delete_entries_with_assets(&remove_ids)?;
    cleanup_paths.extend(db_cleanup_paths);
    if !removed_ids.is_empty() {
        info!(
            "Repaired image artifacts on startup: removed_entries={}",
            removed_ids.len()
        );
    }

    let effects = PipelineEffects {
        removed_ids: removed_ids.clone(),
        cleanup_paths,
        stale_reason: (!removed_ids.is_empty())
            .then_some(ClipboardQueryStaleReason::SettingsOrStartup),
        ..PipelineEffects::default()
    };
    Ok((StartupImageAssetRepair { removed_ids }, effects))
}

enum StartupRepairAction {
    Keep,
    RemoveMissingOriginal,
    KeepDisplayMissingForBackgroundMaintenance,
}

fn startup_repair_action_for_record(
    data_dir: &Path,
    record: &ImageAssetRecord,
) -> StartupRepairAction {
    if record.status == EntryStatus::Pending {
        // Durable job recovery owns pending image consistency. This artifact
        // repair pass only validates ready image artifacts.
        return StartupRepairAction::Keep;
    }
    let Some(original) = record.original_path.as_deref() else {
        return StartupRepairAction::RemoveMissingOriginal;
    };
    if !store::validate_relative_path(data_dir, original).is_some_and(|path| path.exists()) {
        return StartupRepairAction::RemoveMissingOriginal;
    }
    let display_missing = record
        .display_path
        .as_deref()
        .and_then(|path| store::validate_relative_path(data_dir, path))
        .is_none_or(|path| !path.exists());
    if display_missing {
        StartupRepairAction::KeepDisplayMissingForBackgroundMaintenance
    } else {
        StartupRepairAction::Keep
    }
}

fn uncommitted_image_candidate_paths(id: &str) -> Vec<String> {
    let mut paths = vec![image::original_rel_path(id)];
    paths.extend(image::display_candidate_paths(id));
    paths
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
    if !plan.summary.rebuilt_displays.is_empty() || plan.summary.orphan_files_removed > 0 {
        info!(
            "Completed artifact maintenance: rebuilt_displays={}, orphan_files_removed={}",
            plan.summary.rebuilt_displays.len(),
            plan.summary.orphan_files_removed
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
    let mut rebuilt_displays = Vec::new();
    let mut repairs = 0usize;

    for record in &records {
        if repairs >= options.max_repairs {
            break;
        }
        if record.status != EntryStatus::Ready {
            continue;
        }
        let Some(original_rel) = record.original_path.as_deref() else {
            remove_ready_image_record(db, &mut effects, &record.id)?;
            repairs += 1;
            continue;
        };

        let Some(original_abs) = store::validate_relative_path(data_dir, original_rel) else {
            remove_ready_image_record(db, &mut effects, &record.id)?;
            repairs += 1;
            continue;
        };
        if !original_abs.exists() {
            remove_ready_image_record(db, &mut effects, &record.id)?;
            repairs += 1;
            continue;
        }

        let display_missing = record
            .display_path
            .as_deref()
            .and_then(|path| store::validate_relative_path(data_dir, path))
            .is_none_or(|path| !path.exists());
        if !display_missing {
            continue;
        }

        match image::rebuild_display_artifact(data_dir, &record.id, original_rel) {
            Ok(outcome) => {
                if let Some(old_path) = db.replace_artifact(&record.id, &outcome.artifact)? {
                    effects.cleanup_paths.push(old_path);
                }
                effects
                    .cleanup_paths
                    .extend(outcome.old_candidate_paths.into_iter().filter(|path| {
                        path != &outcome.artifact.rel_path
                            && Some(path.as_str()) != record.display_path.as_deref()
                    }));
                if let Some(entry) = db.get_entry_by_id(&record.id)? {
                    effects.updated.push(entry);
                }
                rebuilt_displays.push(record.id.clone());
                repairs += 1;
            }
            Err(image::RebuildDisplayError::OriginalMissing)
            | Err(image::RebuildDisplayError::OriginalBroken(_)) => {
                remove_ready_image_record(db, &mut effects, &record.id)?;
                repairs += 1;
            }
            Err(image::RebuildDisplayError::DisplayWrite(err)) => {
                warn!(
                    "Failed to rebuild display artifact for image entry {}: {}",
                    record.id, err
                );
            }
        }
    }

    let referenced = db.get_all_artifact_paths()?;
    let orphan_paths = store::scan_orphan_artifact_paths(
        data_dir,
        &referenced,
        store::ORPHAN_FILE_PROTECTION_WINDOW,
    )?;
    let orphan_files_removed = orphan_paths.len();
    if orphan_files_removed > 0 {
        effects.cleanup_paths.extend(orphan_paths);
        effects.stale_reason = Some(ClipboardQueryStaleReason::SettingsOrStartup);
    }
    let terminal_staging_paths = image_ingest::cleanup_terminal_jobs(db)?;
    let staging_orphan_paths = image_ingest::plan_staging_orphan_cleanup(
        db,
        data_dir,
        store::ORPHAN_FILE_PROTECTION_WINDOW,
    )?;
    let staging_orphans_removed = terminal_staging_paths.len() + staging_orphan_paths.len();
    if staging_orphans_removed > 0 {
        effects.cleanup_paths.extend(terminal_staging_paths);
        effects.cleanup_paths.extend(staging_orphan_paths);
    }

    Ok(MaintenancePlan {
        effects,
        summary: ArtifactMaintenanceSummary {
            rebuilt_displays,
            orphan_files_removed: orphan_files_removed + staging_orphans_removed,
        },
    })
}

fn remove_ready_image_record(
    db: &Database,
    effects: &mut PipelineEffects,
    entry_id: &str,
) -> Result<(), String> {
    let entry_ids = [entry_id.to_string()];
    let (ids, paths) = db.delete_entries_with_assets(&entry_ids)?;
    effects.removed_ids.extend(ids);
    effects.cleanup_paths.extend(paths);
    effects.stale_reason = Some(ClipboardQueryStaleReason::SettingsOrStartup);
    Ok(())
}

pub fn schedule_periodic_artifact_maintenance() {
    // Future trigger hook: call `run_artifact_maintenance_once` from a timer.
}
