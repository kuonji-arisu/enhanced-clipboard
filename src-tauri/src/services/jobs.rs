use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use log::{debug, warn};

use crate::db::Database;
use crate::services::view_events::EventEmitter;

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
                    match crate::services::image_ingest::run_next_job(
                        &app,
                        &db,
                        &data_dir,
                        expiry_seconds,
                        max_history,
                        &image_dedup,
                    ) {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(err) => warn!("Failed to run deferred content job: {}", err),
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
