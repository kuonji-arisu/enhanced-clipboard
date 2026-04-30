use std::path::Path;

use log::warn;

use crate::db::Database;
use crate::models::{ClipboardEntry, ClipboardQueryStaleReason};
use crate::services::effects::{apply_pipeline_effects, EffectApplyReport, PipelineEffects};
use crate::services::prune;
use crate::services::view_events::EventEmitter;

pub fn insert_ready_entry(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    entry: &ClipboardEntry,
    attrs: &[(&str, &[String])],
    expiry_seconds: i64,
    max_history: u32,
) -> Result<(), String> {
    prune::prepare_for_immediate_ready_insert(app, db, data_dir, expiry_seconds, max_history)?;
    if attrs.is_empty() {
        db.insert_entry(entry)?;
    } else {
        db.insert_entry_with_attrs(entry, attrs)?;
    }
    let mut effects = PipelineEffects {
        added: vec![entry.clone()],
        stale_reason: Some(ClipboardQueryStaleReason::EntryCreated),
        ..PipelineEffects::default()
    };
    let retention_error = match prune::apply_retention_after_ready_change(
        db,
        expiry_seconds,
        max_history,
        ClipboardQueryStaleReason::BeforeInsert,
    ) {
        Ok(retention_effects) => {
            effects.merge(retention_effects);
            None
        }
        Err(err) => {
            warn!(
                "Retention failed after ready insert for entry {}: {}",
                entry.id, err
            );
            Some(err)
        }
    };
    log_effect_warnings(
        "insert ready entry",
        apply_pipeline_effects(app, db, data_dir, effects),
    );
    if let Some(err) = retention_error {
        Err(err)
    } else {
        Ok(())
    }
}

pub fn emit_pending_entry_added(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    entry: &ClipboardEntry,
) -> Result<(), String> {
    let report = apply_pipeline_effects(
        app,
        db,
        data_dir,
        PipelineEffects {
            added: vec![entry.clone()],
            stale_reason: Some(ClipboardQueryStaleReason::EntryCreated),
            ..PipelineEffects::default()
        },
    );
    log_effect_warnings("insert pending entry", report);
    Ok(())
}

pub struct ReadyEntryUpdate<'a> {
    pub entry: ClipboardEntry,
    pub cleanup_paths: Vec<String>,
    pub expiry_seconds: i64,
    pub max_history: u32,
    pub context: &'a str,
}

pub fn finish_ready_entry_update(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    update: ReadyEntryUpdate<'_>,
) -> Result<(), String> {
    let ReadyEntryUpdate {
        entry,
        cleanup_paths,
        expiry_seconds,
        max_history,
        context,
    } = update;
    let mut effects = PipelineEffects {
        cleanup_paths,
        ..PipelineEffects::default()
    };
    let retention_error = match prune::apply_retention_after_ready_change(
        db,
        expiry_seconds,
        max_history,
        ClipboardQueryStaleReason::BeforeInsert,
    ) {
        Ok(retention_effects) => {
            effects.merge(retention_effects);
            None
        }
        Err(err) => {
            warn!(
                "Retention failed after ready entry update for entry {}: {}",
                entry.id, err
            );
            Some(err)
        }
    };
    if !effects.removed_ids.iter().any(|id| id == &entry.id) {
        effects.updated.push(entry);
    }
    apply_effects(app, db, data_dir, effects, context);
    if let Some(err) = retention_error {
        Err(err)
    } else {
        Ok(())
    }
}

pub fn apply_effects(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    effects: PipelineEffects,
    context: &str,
) {
    log_effect_warnings(context, apply_pipeline_effects(app, db, data_dir, effects));
}

fn log_effect_warnings(context: &str, report: EffectApplyReport) {
    for error in report.event_errors {
        warn!("Post-commit effect warning during {}: {}", context, error);
    }
}
