use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use arboard::{Clipboard, Error as ClipboardError};
use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use log::{debug, error, info};
use tauri::{AppHandle, Emitter};

use crate::constants::{DEFAULT_MAX_HISTORY, EVENT_RUNTIME_STATUS_CHANGED};
use crate::db::Database;
use crate::models::RuntimeStatusState;
use crate::services;
use crate::settings::SettingsStore;
use crate::utils::image::{hash_image_sample, image_quick_fingerprint};
use crate::utils::os::get_foreground_process_name;

// ── 本模块常量 ────────────────────────────────────────────────────────────────

/// 文本条目最大字节数（1 MB）
const MAX_TEXT_BYTES: usize = 1_048_576;

/// 图片条目最大原始 RGBA 字节数（100 MB），覆盖 8K 截图场景
const MAX_IMAGE_BYTES: usize = 104_857_600;

fn set_capture_available(
    app_handle: &AppHandle,
    runtime_status: &Arc<RuntimeStatusState>,
    available: bool,
) {
    let payload = {
        let mut status = runtime_status
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if status.clipboard_capture_available == available {
            return;
        }
        status.clipboard_capture_available = available;
        status.clone()
    };

    if let Err(e) = app_handle.emit(EVENT_RUNTIME_STATUS_CHANGED, payload) {
        error!("Failed to emit runtime status change event: {}", e);
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
}

impl ClipboardWatcher {
    pub fn new() -> Self {
        Self {
            text_seed: Arc::new(Mutex::new(None)),
            cached_expiry: Arc::new(AtomicI64::new(0)),
            cached_max_history: Arc::new(AtomicU32::new(DEFAULT_MAX_HISTORY)),
            cached_capture_images: Arc::new(AtomicBool::new(true)),
        }
    }

    /// 在向剪贴板写入明文前调用。
    /// 防止 watcher 将该内容重复保存为新条目。
    pub fn suppress_text(&self, text: String) {
        *self.text_seed.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
    }

    /// 刷新缓存的设置值（由 save_settings 调用，避免每次轮询都查 DB）。
    pub fn refresh_settings(&self, expiry_seconds: i64, max_history: u32, capture_images: bool) {
        self.cached_expiry.store(expiry_seconds, Ordering::Relaxed);
        self.cached_max_history
            .store(max_history, Ordering::Relaxed);
        self.cached_capture_images
            .store(capture_images, Ordering::Relaxed);
        debug!(
            "Watcher settings refreshed: expiry_seconds={}, max_history={}, capture_images={}",
            expiry_seconds, max_history, capture_images
        );
    }

    pub fn start(
        &self,
        app_handle: AppHandle,
        db: Arc<Database>,
        settings: Arc<SettingsStore>,
        data_dir: PathBuf,
        runtime_status: Arc<RuntimeStatusState>,
    ) {
        let text_seed = self.text_seed.clone();
        let cached_expiry = self.cached_expiry.clone();
        let cached_max_history = self.cached_max_history.clone();
        let cached_capture_images = self.cached_capture_images.clone();
        let runtime_status_for_thread = runtime_status.clone();

        thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to initialize clipboard watcher: {}", e);
                    set_capture_available(&app_handle, &runtime_status_for_thread, false);
                    return;
                }
            };

            let mut last_text = String::new();
            let mut last_image_hash = String::new();
            let mut last_image_fingerprint: u64 = 0;

            // 用当前剪贴板内容初始化种子，避免启动时重复保存已有内容
            if let Ok(text) = clipboard.get_text() {
                last_text = text;
            }
            if let Ok(img) = clipboard.get_image() {
                last_image_hash = hash_image_sample(&img.bytes);
                last_image_fingerprint = image_quick_fingerprint(&img);
            }

            // 初始化缓存的设置值
            if let Ok(s) = settings.load_app_settings() {
                cached_expiry.store(s.expiry_seconds, Ordering::Relaxed);
                cached_max_history.store(s.max_history, Ordering::Relaxed);
                cached_capture_images.store(s.capture_images, Ordering::Relaxed);
            }
            info!("Clipboard watcher started");

            let handler = WatcherHandler {
                clipboard,
                app_handle: app_handle.clone(),
                db,
                data_dir,
                runtime_status: runtime_status_for_thread.clone(),
                last_text,
                last_image_hash,
                last_image_fingerprint,
                text_seed,
                cached_expiry,
                cached_max_history,
                cached_capture_images,
            };

            if let Err(e) = Master::new(handler).run() {
                error!("Clipboard watcher exited: {e}");
                set_capture_available(&app_handle, &runtime_status_for_thread, false);
            }
        });
    }
}

// ── 剪贴板事件处理器 ──────────────────────────────────────────────────────────

/// 持有全部运行时状态，由 clipboard-master 的事件循环在每次剪贴板变化时调用。
struct WatcherHandler {
    clipboard: Clipboard,
    app_handle: AppHandle,
    db: Arc<Database>,
    data_dir: PathBuf,
    runtime_status: Arc<RuntimeStatusState>,
    last_text: String,
    last_image_hash: String,
    /// 图片快速指纹，发生变化时才触发 SHA-256 哈希计算
    last_image_fingerprint: u64,
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

        // --- 文本 ---
        let mut text_changed = false;
        match self.clipboard.get_text() {
            Ok(text) => {
                set_capture_available(&self.app_handle, &self.runtime_status, true);
                if !text.is_empty() && text != self.last_text && text.len() <= MAX_TEXT_BYTES {
                    if let Err(e) = services::prune::prepare_for_insert(
                        &self.app_handle,
                        &self.db,
                        &self.data_dir,
                        expiry_sec,
                        max_hist,
                    ) {
                        error!("Pre-prune before text insert failed: {e}");
                        return CallbackResult::Next;
                    }

                    text_changed = true;
                    self.last_text = text.clone();
                    debug!(
                        "Accepted text clipboard change: bytes={}, source_app={}",
                        text.len(),
                        source_app
                    );
                    if let Err(e) = services::ingest::save_text_entry(
                        &self.app_handle,
                        &self.db,
                        text,
                        source_app.clone(),
                    ) {
                        error!("Failed to persist text clipboard entry: {e}");
                    }
                }
            }
            Err(ClipboardError::ContentNotAvailable) => {
                set_capture_available(&self.app_handle, &self.runtime_status, true);
            }
            Err(err) => {
                error!("Failed to read text from clipboard: {}", err);
                set_capture_available(&self.app_handle, &self.runtime_status, false);
                return CallbackResult::Next;
            }
        }

        // --- 图片：文本未变化时才检测，避免同帧写入两条记录 ---
        if capture_images && !text_changed {
            match self.clipboard.get_image() {
                Ok(img) => {
                    set_capture_available(&self.app_handle, &self.runtime_status, true);
                    if img.bytes.len() <= MAX_IMAGE_BYTES {
                        let fp = image_quick_fingerprint(&img);
                        if fp != self.last_image_fingerprint {
                            let hash = hash_image_sample(&img.bytes);
                            if hash != self.last_image_hash {
                                if let Err(e) = services::prune::prepare_for_insert(
                                    &self.app_handle,
                                    &self.db,
                                    &self.data_dir,
                                    expiry_sec,
                                    max_hist,
                                ) {
                                    error!("Pre-prune before image insert failed: {e}");
                                    return CallbackResult::Next;
                                }

                                self.last_image_fingerprint = fp;
                                self.last_image_hash = hash;
                                debug!(
                                    "Accepted image clipboard change: bytes={}, width={}, height={}, source_app={}",
                                    img.bytes.len(),
                                    img.width,
                                    img.height,
                                    source_app
                                );
                                if let Err(e) = services::ingest::save_image_entry(
                                    &self.app_handle,
                                    &self.db,
                                    &self.data_dir,
                                    &img,
                                    source_app,
                                ) {
                                    error!("Failed to persist image clipboard entry: {e}");
                                }
                            }
                        }
                    }
                }
                Err(ClipboardError::ContentNotAvailable) => {
                    set_capture_available(&self.app_handle, &self.runtime_status, true);
                }
                Err(err) => {
                    error!("Failed to read image from clipboard: {}", err);
                    set_capture_available(&self.app_handle, &self.runtime_status, false);
                    return CallbackResult::Next;
                }
            }
        }

        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, error: std::io::Error) -> CallbackResult {
        error!("Clipboard watcher error: {}", error);
        set_capture_available(&self.app_handle, &self.runtime_status, false);
        CallbackResult::Next
    }
}
