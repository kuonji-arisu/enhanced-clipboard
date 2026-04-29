use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use log::{debug, warn};

use crate::models::ClipboardArtifactDraft;
use crate::services::artifacts::{image, store};

pub const CONTENT_JOB_QUEUE_CAPACITY: usize = 2;

#[derive(Debug, Clone, Default)]
pub struct ImageDedupState {
    pub last_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImageDedupUpdate {
    pub state: Arc<Mutex<ImageDedupState>>,
    pub content_hash: String,
}

impl ImageDedupUpdate {
    pub fn clear_if_current(&self) {
        if let Ok(mut state) = self.state.lock() {
            if state.last_hash.as_deref() == Some(self.content_hash.as_str()) {
                state.last_hash = None;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeferredJobContext {
    pub entry_id: String,
    pub dedup_update: Option<ImageDedupUpdate>,
}

impl DeferredJobContext {
    pub fn release_dedup_claim(&self) {
        if let Some(update) = self.dedup_update.as_ref() {
            update.clear_if_current();
        }
    }
}

#[derive(Debug, Default)]
pub struct DeferredClaimRegistry {
    claims: Mutex<HashMap<String, ImageDedupUpdate>>,
}

impl DeferredClaimRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, context: &DeferredJobContext) {
        let Some(update) = context.dedup_update.clone() else {
            return;
        };

        match self.claims.lock() {
            Ok(mut claims) => {
                claims.insert(context.entry_id.clone(), update);
            }
            Err(err) => {
                warn!("Failed to register deferred dedup claim: {err}");
            }
        }
    }

    pub fn release(&self, entry_id: &str) -> bool {
        let update = match self.claims.lock() {
            Ok(mut claims) => claims.remove(entry_id),
            Err(err) => {
                warn!("Failed to release deferred dedup claim: {err}");
                None
            }
        };

        if let Some(update) = update {
            update.clear_if_current();
            true
        } else {
            false
        }
    }

    pub fn forget(&self, entry_id: &str) -> bool {
        match self.claims.lock() {
            Ok(mut claims) => claims.remove(entry_id).is_some(),
            Err(err) => {
                warn!("Failed to forget deferred dedup claim: {err}");
                false
            }
        }
    }

    pub fn release_many<'a>(&self, entry_ids: impl IntoIterator<Item = &'a str>) -> usize {
        let entry_ids: Vec<&str> = entry_ids.into_iter().collect();
        let mut updates = Vec::new();

        match self.claims.lock() {
            Ok(mut claims) => {
                for entry_id in entry_ids {
                    if let Some(update) = claims.remove(entry_id) {
                        updates.push(update);
                    }
                }
            }
            Err(err) => {
                warn!("Failed to release deferred dedup claims: {err}");
            }
        }

        let released = updates.len();
        for update in updates {
            update.clear_if_current();
        }
        released
    }

    pub fn is_active(&self, entry_id: &str) -> bool {
        match self.claims.lock() {
            Ok(claims) => claims.contains_key(entry_id),
            Err(err) => {
                warn!("Failed to inspect deferred dedup claims: {err}");
                false
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageAssetJob {
    pub context: DeferredJobContext,
    pub data_dir: PathBuf,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub start_gate: Option<DeferredJobStartGate>,
}

#[derive(Debug, Clone)]
pub enum DeferredContentJob {
    Image(ImageAssetJob),
}

impl DeferredContentJob {
    pub fn context(&self) -> &DeferredJobContext {
        match self {
            Self::Image(job) => &job.context,
        }
    }

    pub fn candidate_cleanup_paths(&self) -> Vec<String> {
        match self {
            Self::Image(job) => {
                let mut paths = vec![image::original_rel_path(&job.context.entry_id)];
                paths.extend(image::display_candidate_paths(&job.context.entry_id));
                paths
            }
        }
    }

    pub fn with_start_gate(mut self, gate: DeferredJobStartGate) -> Self {
        match &mut self {
            Self::Image(job) => job.start_gate = Some(gate),
        }
        self
    }
}

#[derive(Debug, Clone)]
pub enum DeferredJobResult {
    Ready {
        context: DeferredJobContext,
        artifacts: Vec<ClipboardArtifactDraft>,
    },
    Failed {
        context: DeferredJobContext,
        cleanup_paths: Vec<String>,
        error: String,
    },
}

#[derive(Clone)]
pub struct ContentJobWorker {
    sender: SyncSender<DeferredContentJob>,
}

pub trait DeferredJobQueue {
    fn enqueue(&self, job: DeferredContentJob) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct DeferredJobStartGate {
    ready: Arc<(Mutex<bool>, Condvar)>,
}

impl DeferredJobStartGate {
    pub fn new() -> Self {
        Self {
            ready: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    pub fn release(&self) {
        let (lock, cv) = &*self.ready;
        let mut ready = lock.lock().unwrap_or_else(|e| e.into_inner());
        *ready = true;
        cv.notify_all();
    }

    fn wait(&self) {
        let (lock, cv) = &*self.ready;
        let mut ready = lock.lock().unwrap_or_else(|e| e.into_inner());
        while !*ready {
            ready = cv.wait(ready).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Default for DeferredJobStartGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentJobWorker {
    pub fn start() -> (Self, Receiver<DeferredJobResult>) {
        // Memory backpressure is intentionally simple: at most one running image job
        // plus CONTENT_JOB_QUEUE_CAPACITY queued jobs can hold RGBA buffers.
        let (job_tx, job_rx) = mpsc::sync_channel::<DeferredContentJob>(CONTENT_JOB_QUEUE_CAPACITY);
        let (result_tx, result_rx) = mpsc::channel::<DeferredJobResult>();
        thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let result = run_job(job);
                if result_tx.send(result).is_err() {
                    warn!("Deferred content job result receiver dropped; stopping worker");
                    break;
                }
            }
            debug!("Deferred content job worker stopped");
        });
        (Self { sender: job_tx }, result_rx)
    }

    pub fn enqueue(&self, job: DeferredContentJob) -> Result<(), String> {
        self.sender.try_send(job).map_err(|e| match e {
            TrySendError::Full(_) => "Deferred content job queue is full".to_string(),
            TrySendError::Disconnected(_) => {
                "Failed to enqueue deferred content job: worker stopped".to_string()
            }
        })
    }
}

impl DeferredJobQueue for ContentJobWorker {
    fn enqueue(&self, job: DeferredContentJob) -> Result<(), String> {
        self.enqueue(job)
    }
}

fn run_job(job: DeferredContentJob) -> DeferredJobResult {
    match job {
        DeferredContentJob::Image(job) => {
            if let Some(gate) = job.start_gate.as_ref() {
                gate.wait();
            }
            let context = job.context;
            let entry_id = context.entry_id.clone();
            match image::write_image_artifacts(
                &job.data_dir,
                &entry_id,
                &job.rgba,
                job.width,
                job.height,
            ) {
                Ok(outcome) => DeferredJobResult::Ready {
                    context,
                    artifacts: outcome.artifacts,
                },
                Err(error) => {
                    store::cleanup_generated_paths_for_id(&job.data_dir, &entry_id);
                    DeferredJobResult::Failed {
                        cleanup_paths: vec![
                            image::original_rel_path(&entry_id),
                            image::display_rel_path(
                                &entry_id,
                                crate::utils::image::DisplayAssetFormat::Png,
                            ),
                            image::display_rel_path(
                                &entry_id,
                                crate::utils::image::DisplayAssetFormat::Jpeg,
                            ),
                        ],
                        context,
                        error,
                    }
                }
            }
        }
    }
}
