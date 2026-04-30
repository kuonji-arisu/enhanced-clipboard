use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::db::{Database, EntryJobCleanup, ImageIngestJobCleanupRecord};
use crate::models::{ClipboardJob, ClipboardJobKind};
use crate::services::artifacts::{image, store};
use crate::services::image_ingest::staging;
use crate::services::jobs::{clear_polling_image_dedup_if_current, ImageDedupState};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupPlan {
    pub removed_ids: Vec<String>,
    pub cleanup_paths: Vec<String>,
    pub dedup_keys: Vec<String>,
}

impl CleanupPlan {
    pub fn is_empty(&self) -> bool {
        self.removed_ids.is_empty() && self.cleanup_paths.is_empty() && self.dedup_keys.is_empty()
    }

    pub fn clear_polling_dedup(&self, image_dedup: &Arc<Mutex<ImageDedupState>>) {
        for dedup_key in &self.dedup_keys {
            clear_polling_image_dedup_if_current(image_dedup, dedup_key);
        }
    }
}

pub fn cancel_entry(db: &Database, id: &str) -> Result<Option<CleanupPlan>, String> {
    db.delete_entry_with_job_cleanup(id)
        .map(|cleanup| cleanup.map(cleanup_plan_from_db))
}

pub fn cancel_entries(db: &Database, ids: &[String]) -> Result<CleanupPlan, String> {
    db.delete_entries_with_job_cleanup(ids)
        .map(cleanup_plan_from_db)
}

pub fn cancel_all(db: &Database) -> Result<CleanupPlan, String> {
    db.clear_all_with_job_cleanup().map(cleanup_plan_from_db)
}

pub(crate) fn plan_staging_orphan_cleanup(
    db: &Database,
    data_dir: &Path,
    protection_window: Duration,
) -> Result<Vec<String>, String> {
    let referenced = db.get_image_ingest_input_refs()?;
    staging::scan_orphan_inputs(data_dir, &referenced, protection_window)
}

pub(crate) fn cleanup_terminal_jobs(db: &Database) -> Result<Vec<String>, String> {
    let terminal_jobs = db.cleanup_terminal_image_ingest_jobs()?;
    Ok(staging_cleanup_paths_for_records(&terminal_jobs))
}

pub(super) fn staging_cleanup_paths_for_records(
    records: &[ImageIngestJobCleanupRecord],
) -> Vec<String> {
    let mut seen_paths = HashSet::new();
    records
        .iter()
        .filter(|job| !job.input_ref.is_empty() && seen_paths.insert(job.input_ref.clone()))
        .map(|job| job.input_ref.clone())
        .collect()
}

pub(super) fn cleanup_plan_from_db(mut cleanup: EntryJobCleanup) -> CleanupPlan {
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
        for path in image::generated_candidate_paths(&job.entry_id) {
            if seen_paths.insert(path.clone()) {
                cleanup_paths.push(path);
            }
        }
    }

    CleanupPlan {
        removed_ids: cleanup.removed_ids,
        cleanup_paths,
        dedup_keys,
    }
}

pub(super) fn generated_cleanup_paths_for_job(job: &ClipboardJob) -> Vec<String> {
    match job.kind {
        ClipboardJobKind::ImageIngest => image::generated_candidate_paths(&job.entry_id),
        _ => Vec::new(),
    }
}

pub(super) fn staging_cleanup_path_for_job(job: &ClipboardJob) -> Vec<String> {
    if job.input_ref.is_empty() {
        Vec::new()
    } else {
        vec![job.input_ref.clone()]
    }
}

pub(super) fn staging_input_exists(data_dir: &Path, job: &ClipboardJob) -> bool {
    store::validate_cleanup_relative_path(data_dir, &job.input_ref)
        .is_some_and(|path| path.exists())
}

pub(super) fn cleanup_uncommitted_retry_files(data_dir: &Path, cleanup_paths: Vec<String>) {
    if cleanup_paths.is_empty() {
        return;
    }
    store::cleanup_relative_paths(data_dir, cleanup_paths);
}
