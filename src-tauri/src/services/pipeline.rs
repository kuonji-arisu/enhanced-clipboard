use std::path::Path;

use log::{debug, error, warn};

use crate::db::Database;
use crate::models::{ClipboardEntry, ClipboardQueryStaleReason};
use crate::services::effects::{apply_pipeline_effects, EffectApplyReport, PipelineEffects};
use crate::services::jobs::{
    DeferredClaimRegistry, DeferredContentJob, DeferredJobQueue, DeferredJobResult,
    DeferredJobStartGate,
};
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

pub fn insert_pending_entry(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    worker: &impl DeferredJobQueue,
    claims: &DeferredClaimRegistry,
    entry: &ClipboardEntry,
    job: DeferredContentJob,
) -> Result<(), String> {
    let context = job.context().clone();
    let uncommitted_cleanup_paths = job.candidate_cleanup_paths();
    if let Err(err) = db.insert_entry(entry) {
        context.release_dedup_claim();
        return Err(err);
    }

    let gate = DeferredJobStartGate::new();
    let gated_job = job.with_start_gate(gate.clone());
    if let Err(err) = worker.enqueue(gated_job) {
        let mut cleanup_paths = uncommitted_cleanup_paths;
        match db.delete_pending_entry_with_assets(&entry.id) {
            Ok(db_paths) => {
                context.release_dedup_claim();
                cleanup_paths.extend(db_paths.unwrap_or_default());
            }
            Err(delete_err) => {
                log_effect_warnings(
                    "cleanup failed pending enqueue artifacts",
                    apply_pipeline_effects(
                        app,
                        db,
                        data_dir,
                        PipelineEffects {
                            cleanup_paths,
                            ..PipelineEffects::default()
                        },
                    ),
                );
                return Err(format!(
                    "{err}; failed to delete unqueued pending entry {}: {delete_err}",
                    entry.id
                ));
            }
        }
        log_effect_warnings(
            "rollback failed pending enqueue",
            apply_pipeline_effects(
                app,
                db,
                data_dir,
                PipelineEffects {
                    cleanup_paths,
                    ..PipelineEffects::default()
                },
            ),
        );
        return Err(err);
    }
    claims.register(&context);

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
    gate.release();
    Ok(())
}

pub fn finalize_deferred_result(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    expiry_seconds: i64,
    max_history: u32,
    claims: &DeferredClaimRegistry,
    result: DeferredJobResult,
) -> Result<(), String> {
    let (effects, terminal_error) = match result {
        DeferredJobResult::Ready { context, artifacts } => {
            let entry_id = context.entry_id.clone();
            match db.finalize_pending_entry(&entry_id, &artifacts) {
                Ok(Some(entry)) => {
                    let mut effects = match prune::apply_retention_after_ready_change(
                        db,
                        expiry_seconds,
                        max_history,
                        ClipboardQueryStaleReason::BeforeInsert,
                    ) {
                        Ok(effects) => effects,
                        Err(err) => {
                            warn!(
                                "Retention failed after deferred finalize for entry {}: {}",
                                entry_id, err
                            );
                            claims.forget(&entry_id);
                            let mut effects = PipelineEffects::default();
                            effects.updated.push(entry);
                            return finish_deferred_result(app, db, data_dir, effects, Some(err));
                        }
                    };
                    if effects.removed_ids.iter().any(|id| id == &entry_id) {
                        debug!(
                            "Deferred entry {} was removed by retention after finalize",
                            entry_id
                        );
                        claims.release(&entry_id);
                    } else {
                        effects.updated.push(entry);
                        claims.forget(&entry_id);
                    }
                    (effects, None)
                }
                Ok(None) => {
                    debug!(
                        "Deferred entry {} disappeared before finalize; cleaning generated artifacts",
                        entry_id
                    );
                    claims.release(&entry_id);
                    (
                        PipelineEffects {
                            cleanup_paths: artifacts
                                .into_iter()
                                .map(|artifact| artifact.rel_path)
                                .collect(),
                            ..PipelineEffects::default()
                        },
                        None,
                    )
                }
                Err(err) => {
                    warn!(
                        "Failed to finalize pending entry {}; terminalizing deferred job: {}",
                        entry_id, err
                    );
                    let generated_paths = artifacts
                        .into_iter()
                        .map(|artifact| artifact.rel_path)
                        .collect();
                    let terminalized =
                        terminalize_failed_pending_entry(db, &entry_id, generated_paths);
                    let (effects, terminal_error) = match terminalized {
                        Ok(effects) => {
                            claims.release(&entry_id);
                            (effects, Some(err))
                        }
                        Err(outcome) => (
                            outcome.effects,
                            Some(format!(
                                "{err}; failed to terminalize pending entry {entry_id}: {}",
                                outcome.error
                            )),
                        ),
                    };
                    (effects, terminal_error)
                }
            }
        }
        DeferredJobResult::Failed {
            context,
            cleanup_paths,
            error,
        } => {
            let entry_id = context.entry_id.clone();
            error!(
                "Deferred content job failed for entry {}: {}",
                entry_id, error
            );
            match terminalize_failed_pending_entry(db, &entry_id, cleanup_paths) {
                Ok(effects) => {
                    claims.release(&entry_id);
                    (effects, None)
                }
                Err(outcome) => (
                    outcome.effects,
                    Some(format!(
                        "Failed to terminalize failed deferred entry {entry_id}: {}",
                        outcome.error
                    )),
                ),
            }
        }
    };

    finish_deferred_result(app, db, data_dir, effects, terminal_error)
}

fn terminalize_failed_pending_entry(
    db: &Database,
    entry_id: &str,
    mut cleanup_paths: Vec<String>,
) -> Result<PipelineEffects, Box<FailedPendingTerminalization>> {
    let mut removed_ids = Vec::new();
    let mut stale_reason = None;
    match db.delete_pending_entry_with_assets(entry_id) {
        Ok(Some(db_paths)) => {
            removed_ids.push(entry_id.to_string());
            stale_reason = Some(ClipboardQueryStaleReason::EntriesRemoved);
            cleanup_paths.extend(db_paths);
        }
        Ok(None) => {
            debug!(
                "Deferred entry {} was no longer pending during terminal failure cleanup",
                entry_id
            );
        }
        Err(delete_err) => {
            warn!(
                "Failed to delete pending entry {} during deferred terminalization: {}",
                entry_id, delete_err
            );
            return Err(Box::new(FailedPendingTerminalization {
                effects: PipelineEffects {
                    cleanup_paths,
                    ..PipelineEffects::default()
                },
                error: delete_err,
            }));
        }
    }

    Ok(PipelineEffects {
        removed_ids,
        cleanup_paths,
        stale_reason,
        ..PipelineEffects::default()
    })
}

struct FailedPendingTerminalization {
    effects: PipelineEffects,
    error: String,
}

fn finish_deferred_result(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
    effects: PipelineEffects,
    terminal_error: Option<String>,
) -> Result<(), String> {
    log_effect_warnings(
        "finalize deferred content",
        apply_pipeline_effects(app, db, data_dir, effects),
    );
    if let Some(err) = terminal_error {
        Err(err)
    } else {
        Ok(())
    }
}

fn log_effect_warnings(context: &str, report: EffectApplyReport) {
    for error in report.event_errors {
        warn!("Post-commit effect warning during {}: {}", context, error);
    }
}
