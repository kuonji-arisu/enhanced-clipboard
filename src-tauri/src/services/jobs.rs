use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use log::{debug, warn};

use crate::db::{Database, EntryJobCleanup};
use crate::models::{ClipboardJob, ClipboardJobKind, ClipboardJobStatus};
use crate::services::artifacts::{image, store};
use crate::services::effects::{apply_pipeline_effects, PipelineEffects};
use crate::services::pipeline;
use crate::services::view_events::EventEmitter;

pub const MAX_ACTIVE_IMAGE_INGEST_JOBS: i64 = 3;
pub const MAX_ACTIVE_IMAGE_STAGING_BYTES: i64 = 300 * 1024 * 1024;
pub const MAX_IMAGE_INGEST_ATTEMPTS: i64 = 2;

#[derive(Debug, Clone, Default)]
pub struct ImageDedupState {
    pub last_hash: Option<String>,
}

pub fn clear_polling_image_dedup_if_current(
    state: &Arc<Mutex<ImageDedupState>>,
    dedup_key: &str,
) -> bool {
    match state.lock() {
        Ok(mut state) if state.last_hash.as_deref() == Some(dedup_key) => {
            state.last_hash = None;
            true
        }
        Ok(_) => false,
        Err(err) => {
            warn!("Failed to clear polling image dedup state: {err}");
            false
        }
    }
}

#[derive(Clone)]
pub struct ContentJobWorker {
    wake_tx: Sender<()>,
}

impl ContentJobWorker {
    pub fn start<A>(
        app: A,
        db: Arc<Database>,
        settings: Arc<crate::db::SettingsStore>,
        data_dir: PathBuf,
        image_dedup: Arc<Mutex<ImageDedupState>>,
    ) -> Self
    where
        A: EventEmitter + Clone + Send + 'static,
    {
        let (wake_tx, wake_rx) = mpsc::channel::<()>();
        thread::spawn(move || {
            while wake_rx.recv().is_ok() {
                loop {
                    let job = match db.claim_next_queued_job() {
                        Ok(Some(job)) => job,
                        Ok(None) => break,
                        Err(err) => {
                            warn!("Failed to claim deferred content job: {}", err);
                            break;
                        }
                    };
                    let (expiry_seconds, max_history) = match settings.load_runtime_app_settings() {
                        Ok(settings) => (settings.expiry_seconds, settings.max_history),
                        Err(err) => {
                            warn!(
                                "Failed to load settings for deferred content job; using defaults: {}",
                                err
                            );
                            (
                                crate::constants::DEFAULT_EXPIRY_SECONDS,
                                crate::constants::DEFAULT_MAX_HISTORY,
                            )
                        }
                    };
                    if let Err(err) = pipeline::run_claimed_deferred_job(
                        &app,
                        &db,
                        &data_dir,
                        expiry_seconds,
                        max_history,
                        &image_dedup,
                        job,
                    ) {
                        warn!("Failed to run deferred content job: {}", err);
                    }
                }
            }
            debug!("Deferred content job worker stopped");
        });
        Self { wake_tx }
    }

    pub fn wake(&self) -> Result<(), String> {
        self.wake_tx
            .send(())
            .map_err(|_| "Failed to wake deferred content worker".to_string())
    }
}

pub fn ensure_staging_dirs(data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir.join("staging").join("image_ingest"))
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeferredCleanupPlan {
    pub removed_ids: Vec<String>,
    pub cleanup_paths: Vec<String>,
    pub dedup_keys: Vec<String>,
}

impl DeferredCleanupPlan {
    pub fn is_empty(&self) -> bool {
        self.removed_ids.is_empty() && self.cleanup_paths.is_empty() && self.dedup_keys.is_empty()
    }

    pub fn clear_polling_dedup(&self, image_dedup: &Arc<Mutex<ImageDedupState>>) {
        for dedup_key in &self.dedup_keys {
            clear_polling_image_dedup_if_current(image_dedup, dedup_key);
        }
    }
}

pub fn cleanup_plan_from_db(mut cleanup: EntryJobCleanup) -> DeferredCleanupPlan {
    let mut cleanup_paths = Vec::new();
    cleanup_paths.append(&mut cleanup.artifact_paths);

    let mut seen_paths = HashSet::new();
    let mut dedup_keys = Vec::new();
    for job in cleanup.active_jobs {
        if !job.input_ref.is_empty() && seen_paths.insert(job.input_ref.clone()) {
            cleanup_paths.push(job.input_ref);
        }
        if !job.dedup_key.is_empty() {
            dedup_keys.push(job.dedup_key.clone());
        }
        if job.kind == ClipboardJobKind::ImageIngest {
            for path in image::generated_candidate_paths(&job.entry_id) {
                if seen_paths.insert(path.clone()) {
                    cleanup_paths.push(path);
                }
            }
        }
    }

    DeferredCleanupPlan {
        removed_ids: cleanup.removed_ids,
        cleanup_paths,
        dedup_keys,
    }
}

pub fn delete_entry_and_cancel_jobs(
    db: &Database,
    id: &str,
) -> Result<Option<DeferredCleanupPlan>, String> {
    db.delete_entry_with_job_cleanup(id)
        .map(|cleanup| cleanup.map(cleanup_plan_from_db))
}

pub fn delete_entries_and_cancel_jobs(
    db: &Database,
    ids: &[String],
) -> Result<DeferredCleanupPlan, String> {
    db.delete_entries_with_job_cleanup(ids)
        .map(cleanup_plan_from_db)
}

pub fn clear_all_entries_and_cancel_jobs(db: &Database) -> Result<DeferredCleanupPlan, String> {
    db.clear_all_with_job_cleanup().map(cleanup_plan_from_db)
}

pub fn generated_cleanup_paths_for_job(job: &ClipboardJob) -> Vec<String> {
    match job.kind {
        ClipboardJobKind::ImageIngest => image::generated_candidate_paths(&job.entry_id),
        _ => Vec::new(),
    }
}

pub fn staging_cleanup_path_for_job(job: &ClipboardJob) -> Vec<String> {
    if job.input_ref.is_empty() {
        Vec::new()
    } else {
        vec![job.input_ref.clone()]
    }
}

pub fn staging_input_exists(data_dir: &Path, job: &ClipboardJob) -> bool {
    store::validate_cleanup_relative_path(data_dir, &job.input_ref)
        .is_some_and(|path| path.exists())
}

pub fn is_recoverable_image_ingest_job(job: &ClipboardJob) -> bool {
    job.kind == ClipboardJobKind::ImageIngest
        && matches!(
            job.status,
            ClipboardJobStatus::Queued | ClipboardJobStatus::Running
        )
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupJobRecovery {
    pub requeued_running: usize,
    pub removed_ids: Vec<String>,
}

pub fn run_startup_recovery(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
) -> Result<StartupJobRecovery, String> {
    ensure_staging_dirs(data_dir)?;
    let (summary, effects) = plan_startup_recovery(db, data_dir)?;
    let report = apply_pipeline_effects(app, db, data_dir, effects);
    for error in report.event_errors {
        warn!("Post-commit startup job recovery effect warning: {}", error);
    }
    Ok(summary)
}

pub fn plan_startup_recovery(
    db: &Database,
    data_dir: &Path,
) -> Result<(StartupJobRecovery, PipelineEffects), String> {
    let requeued_running = db.requeue_running_image_ingest_jobs()?;
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

    remove_ids.extend(db.get_pending_image_entries_without_active_job()?);
    remove_ids.sort();
    remove_ids.dedup();

    let plan = delete_entries_and_cancel_jobs(db, &remove_ids)?;
    let _ = db.cleanup_terminal_jobs()?;
    let effects = PipelineEffects {
        removed_ids: plan.removed_ids.clone(),
        cleanup_paths: plan.cleanup_paths,
        stale_reason: (!plan.removed_ids.is_empty())
            .then_some(crate::models::ClipboardQueryStaleReason::SettingsOrStartup),
        ..PipelineEffects::default()
    };
    Ok((
        StartupJobRecovery {
            requeued_running,
            removed_ids: plan.removed_ids,
        },
        effects,
    ))
}
