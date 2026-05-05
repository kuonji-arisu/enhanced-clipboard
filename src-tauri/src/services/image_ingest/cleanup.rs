use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::db::{Database, EntryJobCleanup, ImageIngestJobCleanupRecord};
use crate::models::ClipboardJob;
use crate::services::artifacts::{image, store};
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
        .map(|cleanup| cleanup.map(cleanup_plan_from_entry_removal))
}

pub fn cancel_entries(db: &Database, ids: &[String]) -> Result<CleanupPlan, String> {
    db.delete_entries_with_job_cleanup(ids)
        .map(cleanup_plan_from_entry_removal)
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

pub fn cleanup_plan_from_entry_removal(mut cleanup: EntryJobCleanup) -> CleanupPlan {
    let mut seen_paths = cleanup
        .artifact_paths
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let mut cleanup_paths = Vec::new();
    cleanup_paths.append(&mut cleanup.artifact_paths);

    let mut dedup_keys = Vec::new();
    for job in cleanup.image_jobs {
        if !job.input_ref.is_empty() && seen_paths.insert(job.input_ref.clone()) {
            cleanup_paths.push(job.input_ref.clone());
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
    image::generated_candidate_paths(&job.entry_id)
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
