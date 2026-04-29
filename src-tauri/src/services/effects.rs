use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::thread;

use log::{info, warn};

use crate::db::Database;
use crate::models::{ClipboardEntry, ClipboardQueryStaleReason};
use crate::services::artifacts::store;
use crate::services::entry_tags::attach_tags;
use crate::services::view_events::{self, EventEmitter};

#[derive(Debug, Clone, Default)]
pub struct PipelineEffects {
    pub added: Vec<ClipboardEntry>,
    pub updated: Vec<ClipboardEntry>,
    pub removed_ids: Vec<String>,
    pub cleanup_paths: Vec<String>,
    pub stale_reason: Option<ClipboardQueryStaleReason>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectApplyReport {
    pub event_errors: Vec<String>,
    pub cleanup_paths_scheduled: usize,
}

impl EffectApplyReport {
    pub fn has_event_errors(&self) -> bool {
        !self.event_errors.is_empty()
    }

    pub fn first_error(&self) -> Option<&str> {
        self.event_errors.first().map(String::as_str)
    }
}

impl PipelineEffects {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.updated.is_empty()
            && self.removed_ids.is_empty()
            && self.cleanup_paths.is_empty()
            && self.stale_reason.is_none()
    }

    pub fn merge(&mut self, mut other: PipelineEffects) {
        self.added.append(&mut other.added);
        self.updated.append(&mut other.updated);
        self.removed_ids.append(&mut other.removed_ids);
        self.cleanup_paths.append(&mut other.cleanup_paths);
        if other.stale_reason.is_some() {
            self.stale_reason = other.stale_reason;
        }
    }
}

/// Applies view-facing post-commit effects. DB state is authoritative for
/// added/updated payloads, and artifact cleanup is scheduled after event emits
/// even when an emit returns an error.
pub fn apply_pipeline_effects(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    effects: PipelineEffects,
) -> EffectApplyReport {
    apply_pipeline_effects_with_cleanup(app, db, data_dir, effects, &BackgroundArtifactCleanup)
}

pub trait ArtifactCleanupExecutor {
    fn cleanup(&self, data_dir: &Path, paths: Vec<String>) -> usize;
}

pub struct BackgroundArtifactCleanup;

impl ArtifactCleanupExecutor for BackgroundArtifactCleanup {
    fn cleanup(&self, data_dir: &Path, paths: Vec<String>) -> usize {
        let count = paths.len();
        if count == 0 {
            return 0;
        }
        let data_dir: PathBuf = data_dir.to_path_buf();
        thread::spawn(move || {
            store::cleanup_relative_paths(&data_dir, paths);
        });
        count
    }
}

pub struct InlineArtifactCleanup;

impl ArtifactCleanupExecutor for InlineArtifactCleanup {
    fn cleanup(&self, data_dir: &Path, paths: Vec<String>) -> usize {
        let count = paths.len();
        store::cleanup_relative_paths(data_dir, paths);
        count
    }
}

pub fn apply_pipeline_effects_with_cleanup(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    mut effects: PipelineEffects,
    cleanup: &impl ArtifactCleanupExecutor,
) -> EffectApplyReport {
    let cleanup_paths_scheduled = effects.cleanup_paths.len();
    let mut report = EffectApplyReport {
        cleanup_paths_scheduled,
        ..EffectApplyReport::default()
    };

    if effects.is_empty() {
        return report;
    }

    let removed = effects
        .removed_ids
        .iter()
        .cloned()
        .collect::<HashSet<String>>();
    effects.added.retain(|entry| !removed.contains(&entry.id));
    effects.updated.retain(|entry| !removed.contains(&entry.id));

    for entry in effects.added {
        let Some((entry, artifacts)) = load_final_projection_inputs(db, &entry.id, &mut report)
        else {
            continue;
        };
        if let Err(err) = view_events::emit_stream_item_added(app, data_dir, &entry, &artifacts) {
            report.event_errors.push(err);
        }
    }

    for entry in effects.updated {
        let Some((entry, artifacts)) = load_final_projection_inputs(db, &entry.id, &mut report)
        else {
            continue;
        };
        if let Err(err) = view_events::emit_stream_item_updated(app, data_dir, &entry, &artifacts) {
            report.event_errors.push(err);
        }
    }

    if !effects.removed_ids.is_empty() {
        info!(
            "Removed entries: count={}, assets={}",
            effects.removed_ids.len(),
            effects.cleanup_paths.len()
        );
        if let Err(err) = view_events::emit_entries_removed(app, effects.removed_ids) {
            report.event_errors.push(err);
        }
    }

    if let Some(reason) = effects.stale_reason {
        if let Err(err) = view_events::emit_query_results_stale(app, reason) {
            report.event_errors.push(err);
        }
    }

    cleanup.cleanup(data_dir, effects.cleanup_paths);
    report
}

fn load_final_projection_inputs(
    db: &Database,
    entry_id: &str,
    report: &mut EffectApplyReport,
) -> Option<(ClipboardEntry, Vec<crate::models::ClipboardArtifact>)> {
    let mut entry = match db.get_entry_by_id(entry_id) {
        Ok(Some(entry)) => entry,
        Ok(None) => return None,
        Err(err) => {
            warn!(
                "Failed to reload entry before stream effect emit for entry {}: {}",
                entry_id, err
            );
            report.event_errors.push(err);
            return None;
        }
    };
    if let Err(err) = attach_tags(db, std::slice::from_mut(&mut entry)) {
        warn!("Failed to attach tags before stream effect emit: {}", err);
    }
    let artifacts = match db.get_artifacts_for_entry(&entry.id) {
        Ok(artifacts) => artifacts,
        Err(err) => {
            warn!(
                "Failed to load artifacts before stream effect emit for entry {}: {}",
                entry.id, err
            );
            report.event_errors.push(err);
            Vec::new()
        }
    };
    Some((entry, artifacts))
}
