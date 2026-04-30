use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use log::{debug, info, warn};

use crate::constants::MAX_PINNED_ENTRIES;
use crate::db::{Database, PinToggleResult, SettingsStore};
use crate::i18n::I18n;
use crate::models::{ArtifactRole, ClipboardQueryStaleReason, EntryStatus};
use crate::services::artifacts::maintenance::ArtifactMaintenanceScheduler;
use crate::services::artifacts::store;
use crate::services::effects::{
    apply_pipeline_effects, apply_pipeline_effects_with_cleanup, EffectApplyReport,
    InlineArtifactCleanup, PipelineEffects,
};
use crate::services::image_ingest;
use crate::services::jobs::ImageDedupState;
use crate::services::prune;
use crate::services::view_events::EventEmitter;
use crate::utils::clipboard::{write_file_to_clipboard, write_text_to_clipboard};
use crate::watcher::ClipboardWatcher;

/// Write the selected entry back to the system clipboard.
pub fn copy_to_clipboard_or_repair(
    app: &impl EventEmitter,
    db: &Database,
    watcher: &ClipboardWatcher,
    data_dir: &Path,
    id: &str,
    tr: &I18n,
) -> Result<(), String> {
    let entry = db
        .get_entry_by_id(id)?
        .ok_or_else(|| tr.t("errEntryNotFound"))?;

    match entry.content_type.as_str() {
        "text" => {
            watcher.begin_text_suppression(entry.content.clone());
            if let Err(err) = write_text_to_clipboard(&entry.content) {
                watcher.rollback_text_suppression(&entry.content);
                return Err(err);
            }
            debug!("Copied text entry back to clipboard: id={}", id);
        }
        "image" => {
            if entry.status != EntryStatus::Ready {
                return Err(tr.t("errImagePathMissing"));
            }
            let artifacts = db.get_artifacts_for_entry(id)?;
            let Some(img_rel) = artifacts
                .iter()
                .find(|artifact| artifact.role == ArtifactRole::Original)
                .map(|artifact| artifact.rel_path.as_str())
            else {
                remove_entry(
                    app,
                    db,
                    data_dir,
                    None,
                    id,
                    ClipboardQueryStaleReason::EntryRemoved,
                )?;
                return Err(tr.t("errImageFileMissing"));
            };
            let Some(img_path) = store::validate_relative_path(data_dir, img_rel) else {
                remove_entry(
                    app,
                    db,
                    data_dir,
                    None,
                    id,
                    ClipboardQueryStaleReason::EntryRemoved,
                )?;
                return Err(tr.t("errImageFileMissing"));
            };
            if !img_path.exists() {
                remove_entry(
                    app,
                    db,
                    data_dir,
                    None,
                    id,
                    ClipboardQueryStaleReason::EntryRemoved,
                )?;
                return Err(tr.t("errImageFileMissing"));
            }
            write_file_to_clipboard(&img_path)?;
            debug!("Copied image entry back to clipboard: id={}", id);
        }
        _ => return Err(tr.t("errUnknownType")),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageLoadFailureOutcome {
    Unchanged,
    Removed,
    MarkedRepairing,
}

impl ImageLoadFailureOutcome {
    pub fn changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    fn should_schedule_maintenance(self) -> bool {
        matches!(self, Self::MarkedRepairing)
    }
}

pub fn report_image_load_failed<A>(
    app: A,
    db: Arc<Database>,
    data_dir: PathBuf,
    maintenance: &ArtifactMaintenanceScheduler,
    id: &str,
) -> Result<bool, String>
where
    A: EventEmitter + Clone + Send + 'static,
{
    let outcome = handle_image_load_failed(&app, &db, &data_dir, id)?;
    if outcome.should_schedule_maintenance() {
        maintenance.start(app, db, data_dir);
    }
    Ok(outcome.changed())
}

pub fn handle_image_load_failed(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    id: &str,
) -> Result<ImageLoadFailureOutcome, String> {
    let Some(entry) = db.get_entry_by_id(id)? else {
        return Ok(ImageLoadFailureOutcome::Unchanged);
    };
    if entry.content_type != "image" {
        warn!("Ignoring image-load failure for non-image entry: {}", id);
        return Ok(ImageLoadFailureOutcome::Unchanged);
    }
    if entry.status == EntryStatus::Pending {
        debug!(
            "Ignoring image-load failure for pending image entry: {}",
            id
        );
        return Ok(ImageLoadFailureOutcome::Unchanged);
    }

    let artifacts = db.get_artifacts_for_entry(id)?;
    let original_path = artifacts
        .iter()
        .find(|artifact| artifact.role == ArtifactRole::Original)
        .map(|artifact| artifact.rel_path.as_str());
    let original_exists = original_path
        .and_then(|rel_path| store::validate_relative_path(data_dir, rel_path))
        .is_some_and(|path| path.exists());

    if !original_exists {
        let removed = remove_entry(
            app,
            db,
            data_dir,
            None,
            id,
            ClipboardQueryStaleReason::EntryRemoved,
        )?;
        if removed {
            info!(
                "Removed image entry after source artifact was missing on load failure: id={}",
                id
            );
        }
        return Ok(if removed {
            ImageLoadFailureOutcome::Removed
        } else {
            ImageLoadFailureOutcome::Unchanged
        });
    }

    let display_cleanup = db.delete_artifact(id, ArtifactRole::Display)?;
    let Some(current_entry) = db.get_entry_by_id(id)? else {
        return Ok(ImageLoadFailureOutcome::Unchanged);
    };
    log_effect_warnings(
        "mark image display repairing",
        apply_pipeline_effects_with_cleanup(
            app,
            db,
            data_dir,
            PipelineEffects {
                updated: vec![current_entry],
                cleanup_paths: display_cleanup.into_iter().collect(),
                ..PipelineEffects::default()
            },
            &InlineArtifactCleanup,
        ),
    );
    info!(
        "Marked image display artifact for repair after frontend load failure: id={}",
        id
    );
    Ok(ImageLoadFailureOutcome::MarkedRepairing)
}

/// Delete the target entry and any associated artifact files.
pub fn remove_entry(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    image_dedup: Option<&Arc<Mutex<ImageDedupState>>>,
    id: &str,
    stale_reason: ClipboardQueryStaleReason,
) -> Result<bool, String> {
    if let Some(plan) = image_ingest::cancel_entry(db, id)? {
        if let Some(image_dedup) = image_dedup {
            plan.clear_polling_dedup(image_dedup);
        }
        log_effect_warnings(
            "delete entry",
            apply_pipeline_effects(
                app,
                db,
                data_dir,
                PipelineEffects {
                    removed_ids: plan.removed_ids,
                    cleanup_paths: plan.cleanup_paths,
                    stale_reason: Some(stale_reason),
                    ..PipelineEffects::default()
                },
            ),
        );
        info!("Deleted entry: id={}", id);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Toggle pinned state for an entry, enforcing the max-pinned limit.
pub fn toggle_pin_entry(
    app: &impl EventEmitter,
    db: &Database,
    settings: &SettingsStore,
    data_dir: &Path,
    id: &str,
    tr: &I18n,
) -> Result<(), String> {
    let new_state = match db.toggle_pinned_with_limit(id, MAX_PINNED_ENTRIES)? {
        PinToggleResult::Updated(new_state) => new_state,
        PinToggleResult::NotFound => return Err(tr.t("errEntryNotFound")),
        PinToggleResult::LimitExceeded => {
            return Err(tr.t_fmt(
                "pinLimitMessage",
                &[("count", MAX_PINNED_ENTRIES.to_string())],
            ));
        }
    };
    info!("Updated pin state: id={}, pinned={}", id, new_state);

    let mut effects = PipelineEffects {
        stale_reason: Some(ClipboardQueryStaleReason::PinChanged),
        ..PipelineEffects::default()
    };
    if !new_state {
        let settings = settings.load_runtime_app_settings()?;
        effects.merge(prune::apply_retention_after_ready_change(
            db,
            settings.expiry_seconds,
            settings.max_history,
            ClipboardQueryStaleReason::UnpinRetention,
        )?);
    }

    match db.get_entry_by_id(id)? {
        Some(updated_entry) => effects.updated.push(updated_entry),
        None if new_state => warn!(
            "Entry disappeared before clipboard_stream_item_updated emit: {}",
            id
        ),
        None => {}
    }
    log_effect_warnings(
        "toggle pin",
        apply_pipeline_effects(app, db, data_dir, effects),
    );
    Ok(())
}

/// Remove every entry and all committed artifact files.
pub fn clear_all_entries(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    image_dedup: Option<&Arc<Mutex<ImageDedupState>>>,
) -> Result<Vec<String>, String> {
    let plan = image_ingest::cancel_all(db)?;
    let ids = plan.removed_ids.clone();
    if !ids.is_empty() {
        if let Some(image_dedup) = image_dedup {
            plan.clear_polling_dedup(image_dedup);
        }
        log_effect_warnings(
            "clear all entries",
            apply_pipeline_effects(
                app,
                db,
                data_dir,
                PipelineEffects {
                    removed_ids: ids.clone(),
                    cleanup_paths: plan.cleanup_paths,
                    stale_reason: Some(ClipboardQueryStaleReason::ClearAll),
                    ..PipelineEffects::default()
                },
            ),
        );
        info!("Cleared all entries: count={}", ids.len());
    }
    Ok(ids)
}

fn log_effect_warnings(context: &str, report: EffectApplyReport) {
    for error in report.event_errors {
        warn!("Post-commit effect warning during {}: {}", context, error);
    }
}
