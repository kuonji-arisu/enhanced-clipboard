use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use arboard::{Clipboard, Error as ClipboardError};
use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use log::{debug, error, info, warn};
use tauri::{AppHandle, Manager, Theme};

use crate::constants::{DEFAULT_MAX_HISTORY, MAIN_WINDOW_LABEL};
use crate::db::{Database, SettingsStore};
use crate::models::{RuntimeStatusPatch, RuntimeStatusState};
use crate::services;
use crate::services::ingest::{ImageIngestDeps, RetentionSettings};
use crate::services::jobs::{ContentJobWorker, ImageDedupState};
use crate::utils::os::get_foreground_process_name;

fn report_capture_available(
    app_handle: &AppHandle,
    runtime_status: &Arc<RuntimeStatusState>,
    available: bool,
) {
    if let Err(e) = services::runtime::apply_patch(
        app_handle,
        runtime_status,
        RuntimeStatusPatch {
            clipboard_capture_available: Some(available),
            ..RuntimeStatusPatch::default()
        },
    ) {
        error!("Failed to update runtime status: {}", e);
    }
}

fn report_system_theme(
    app_handle: &AppHandle,
    runtime_status: &Arc<RuntimeStatusState>,
    theme: Theme,
) {
    let system_theme = match theme {
        Theme::Dark => "dark",
        _ => "light",
    };

    if let Err(e) = services::runtime::apply_patch(
        app_handle,
        runtime_status,
        RuntimeStatusPatch {
            system_theme: Some(system_theme.to_string()),
            ..RuntimeStatusPatch::default()
        },
    ) {
        error!("Failed to update runtime system theme: {}", e);
    }
}

/// 后台线程，监听系统剪贴板变化事件。
/// Windows 使用 AddClipboardFormatListener / WM_CLIPBOARDUPDATE（真正的 OS 推送，无忙等）；
pub struct ClipboardWatcher {
    /// 由 copy_to_clipboard 在写入剪贴板前设置，
    /// 防止 watcher 将刚写入的内容重复保存为新条目。
    text_seed: Arc<Mutex<Option<String>>>,
    /// 缓存设置值，由 save_settings 时更新，避免每次回调都查数据库
    cached_expiry: Arc<AtomicI64>,
    cached_max_history: Arc<AtomicU32>,
    cached_capture_images: Arc<AtomicBool>,
    image_dedup: Arc<Mutex<ImageDedupState>>,
}

pub struct WatcherStartContext {
    pub app_handle: AppHandle,
    pub db: Arc<Database>,
    pub settings: Arc<SettingsStore>,
    pub data_dir: PathBuf,
    pub content_worker: ContentJobWorker,
    pub runtime_status: Arc<RuntimeStatusState>,
}

impl ClipboardWatcher {
    pub fn new() -> Self {
        Self {
            text_seed: Arc::new(Mutex::new(None)),
            cached_expiry: Arc::new(AtomicI64::new(0)),
            cached_max_history: Arc::new(AtomicU32::new(DEFAULT_MAX_HISTORY)),
            cached_capture_images: Arc::new(AtomicBool::new(true)),
            image_dedup: Arc::new(Mutex::new(ImageDedupState::default())),
        }
    }

    /// 在向剪贴板写入明文前调用。
    /// 防止 watcher 将该内容重复保存为新条目。
    pub fn begin_text_suppression(&self, text: String) {
        *self.text_seed.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
    }

    /// 文本写入系统剪贴板失败时回滚待抑制状态。
    /// 如果 watcher 已经消费掉这次抑制，则保持现状。
    pub fn rollback_text_suppression(&self, text: &str) {
        let mut seed = self.text_seed.lock().unwrap_or_else(|e| e.into_inner());
        if seed.as_deref() == Some(text) {
            *seed = None;
        }
    }

    /// 刷新缓存的设置值（由 save_settings 调用，避免每次轮询都查 DB）。
    pub fn refresh_settings(&self, expiry_seconds: i64, max_history: u32, capture_images: bool) {
        self.refresh_retention_settings(expiry_seconds, max_history);
        self.refresh_capture_images(capture_images);
        debug!(
            "Watcher settings refreshed: expiry_seconds={}, max_history={}, capture_images={}",
            expiry_seconds, max_history, capture_images
        );
    }

    pub fn refresh_retention_settings(&self, expiry_seconds: i64, max_history: u32) {
        self.cached_expiry.store(expiry_seconds, Ordering::Relaxed);
        self.cached_max_history
            .store(max_history, Ordering::Relaxed);
    }

    pub fn refresh_capture_images(&self, capture_images: bool) {
        self.cached_capture_images
            .store(capture_images, Ordering::Relaxed);
    }

    pub fn image_dedup_state(&self) -> Arc<Mutex<ImageDedupState>> {
        self.image_dedup.clone()
    }

    pub fn initialize_system_theme(
        &self,
        app_handle: &AppHandle,
        runtime_status: &Arc<RuntimeStatusState>,
    ) {
        let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) else {
            warn!("Main window not found while initializing system theme");
            return;
        };

        match window.theme() {
            Ok(theme) => report_system_theme(app_handle, runtime_status, theme),
            Err(err) => warn!("Failed to read initial system theme: {}", err),
        }
    }

    pub fn handle_system_theme_change(
        &self,
        app_handle: &AppHandle,
        runtime_status: &Arc<RuntimeStatusState>,
        theme: Theme,
    ) {
        report_system_theme(app_handle, runtime_status, theme);
    }

    pub fn start(&self, context: WatcherStartContext) {
        let text_seed = self.text_seed.clone();
        let cached_expiry = self.cached_expiry.clone();
        let cached_max_history = self.cached_max_history.clone();
        let cached_capture_images = self.cached_capture_images.clone();
        let image_dedup = self.image_dedup.clone();
        let WatcherStartContext {
            app_handle,
            db,
            settings,
            data_dir,
            content_worker,
            runtime_status,
        } = context;
        let runtime_status_for_thread = runtime_status.clone();

        thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to initialize clipboard watcher: {}", e);
                    report_capture_available(&app_handle, &runtime_status_for_thread, false);
                    return;
                }
            };

            let bootstrap =
                services::ingest::bootstrap_watcher(&mut clipboard, &settings, &image_dedup);
            if let Some(settings) = bootstrap.settings {
                cached_expiry.store(settings.retention.expiry_seconds, Ordering::Relaxed);
                cached_max_history.store(settings.retention.max_history, Ordering::Relaxed);
                cached_capture_images.store(settings.capture_images, Ordering::Relaxed);
            }
            info!("Clipboard watcher started");

            let handler = WatcherHandler {
                clipboard,
                app_handle: app_handle.clone(),
                db,
                data_dir,
                content_worker,
                runtime_status: runtime_status_for_thread.clone(),
                last_text: bootstrap.last_text,
                image_dedup,
                text_seed,
                cached_expiry,
                cached_max_history,
                cached_capture_images,
            };

            if let Err(e) = Master::new(handler).run() {
                error!("Clipboard watcher exited: {e}");
                report_capture_available(&app_handle, &runtime_status_for_thread, false);
            }
        });
    }
}

impl Default for ClipboardWatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ── 剪贴板事件处理器 ──────────────────────────────────────────────────────────

/// 持有 watcher 会话状态，由 clipboard-master 的事件循环在每次剪贴板变化时调用。
struct WatcherHandler {
    clipboard: Clipboard,
    app_handle: AppHandle,
    db: Arc<Database>,
    data_dir: PathBuf,
    content_worker: ContentJobWorker,
    runtime_status: Arc<RuntimeStatusState>,
    last_text: String,
    image_dedup: Arc<Mutex<ImageDedupState>>,
    text_seed: Arc<Mutex<Option<String>>>,
    cached_expiry: Arc<AtomicI64>,
    cached_max_history: Arc<AtomicU32>,
    cached_capture_images: Arc<AtomicBool>,
}

impl ClipboardHandler for WatcherHandler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        // 在检测剪贴板变化前采样前台进程名，作为来源
        let source_app = get_foreground_process_name();

        // 清除 copy_to_clipboard 设置的文本抑制种子，避免重复保存
        if let Some(seeded) = self
            .text_seed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            self.last_text = seeded;
        }

        // 从原子缓存中读取设置，无需访问数据库
        let expiry_sec = self.cached_expiry.load(Ordering::Relaxed);
        let max_hist = self.cached_max_history.load(Ordering::Relaxed);
        let capture_images = self.cached_capture_images.load(Ordering::Relaxed);
        let retention = RetentionSettings {
            expiry_seconds: expiry_sec,
            max_history: max_hist,
        };

        // --- 文本 ---
        let mut text_changed = false;
        match self.clipboard.get_text() {
            Ok(text) => {
                report_capture_available(&self.app_handle, &self.runtime_status, true);
                match services::ingest::accept_text_clipboard_change(
                    &self.app_handle,
                    &self.db,
                    &self.data_dir,
                    text,
                    &source_app,
                    &self.last_text,
                    retention,
                ) {
                    Ok(Some(change)) => {
                        text_changed = true;
                        self.last_text = change.last_text;
                        if let Err(e) = change.persist_result {
                            error!("Failed to persist text clipboard entry: {e}");
                            return CallbackResult::Next;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        error!("Failed to prepare text clipboard entry: {e}");
                        return CallbackResult::Next;
                    }
                }
            }
            Err(ClipboardError::ContentNotAvailable) => {
                report_capture_available(&self.app_handle, &self.runtime_status, true);
            }
            Err(err) => {
                error!("Failed to read text from clipboard: {}", err);
                report_capture_available(&self.app_handle, &self.runtime_status, false);
                return CallbackResult::Next;
            }
        }

        // --- 图片：文本未变化时才检测，避免同帧写入两条记录 ---
        if capture_images && !text_changed {
            match self.clipboard.get_image() {
                Ok(img) => {
                    report_capture_available(&self.app_handle, &self.runtime_status, true);
                    match services::ingest::accept_image_clipboard_change(
                        ImageIngestDeps {
                            app_handle: &self.app_handle,
                            db: &self.db,
                            data_dir: &self.data_dir,
                            worker: &self.content_worker,
                        },
                        &img,
                        &source_app,
                        &self.image_dedup,
                    ) {
                        Ok(Some(change)) => {
                            if let Err(e) = change.persist_result {
                                error!("Failed to persist image clipboard entry: {e}");
                                return CallbackResult::Next;
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            error!("Failed to prepare image clipboard entry: {e}");
                            return CallbackResult::Next;
                        }
                    }
                }
                Err(ClipboardError::ContentNotAvailable) => {
                    report_capture_available(&self.app_handle, &self.runtime_status, true);
                }
                Err(err) => {
                    error!("Failed to read image from clipboard: {}", err);
                    report_capture_available(&self.app_handle, &self.runtime_status, false);
                    return CallbackResult::Next;
                }
            }
        }

        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, error: std::io::Error) -> CallbackResult {
        error!("Clipboard watcher error: {}", error);
        report_capture_available(&self.app_handle, &self.runtime_status, false);
        CallbackResult::Next
    }
}
