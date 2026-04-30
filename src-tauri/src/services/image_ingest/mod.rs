use std::path::Path;
use std::sync::Arc;

use crate::db::Database;
use crate::services::jobs::ContentJobWorker;

mod capture;
mod cleanup;
mod recovery;
mod runner;
pub mod staging;

pub use capture::capture_image;
pub use cleanup::{cancel_all, cancel_entries, cancel_entry, CleanupPlan};
pub use recovery::{plan_startup_recovery, recover_startup, StartupRecovery};
pub use runner::{run_claimed_job, run_next_job};
pub use staging::ensure_dirs as ensure_staging_dirs;

pub const MAX_ACTIVE_IMAGE_INGEST_JOBS: i64 = 3;
pub const MAX_ACTIVE_IMAGE_STAGING_BYTES: i64 = 300 * 1024 * 1024;
pub const MAX_IMAGE_INGEST_ATTEMPTS: i64 = 2;

pub struct CaptureImageDeps<'a, A> {
    pub app_handle: &'a A,
    pub db: &'a Arc<Database>,
    pub data_dir: &'a Path,
    pub worker: &'a ContentJobWorker,
}
