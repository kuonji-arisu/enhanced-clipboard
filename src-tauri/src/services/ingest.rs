use std::path::Path;
use std::sync::{Arc, Mutex};

use arboard::Clipboard;
use chrono::Utc;
use log::debug;
use uuid::Uuid;

use crate::db::{Database, SettingsStore};
use crate::models::{ClipboardContentType, ClipboardEntry, EntryStatus};
use crate::services::entry_tags::{detect_tags_for_text, ENTRY_ATTR_TYPE_TAG};
use crate::services::image_ingest::{self, CaptureImageDeps};
use crate::services::jobs::{ContentJobWorker, ImageDedupState, TextDedupState};
use crate::services::pipeline;
use crate::services::search_preview::build_canonical_search_text;
use crate::services::view_events::EventEmitter;
use crate::utils::image::hash_image_content;
use crate::utils::string::hash_text_content;

/// 文本条目最大字节数（1 MB）
const MAX_TEXT_BYTES: usize = 1_048_576;

/// 图片条目最大原始 RGBA 字节数（100 MiB），覆盖常见 4K 和部分高分辨率截图。
const MAX_IMAGE_BYTES: usize = 104_857_600;

pub struct WatcherSettingsSnapshot {
    pub retention: RetentionSettings,
    pub capture_images: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RetentionSettings {
    pub expiry_seconds: i64,
    pub max_history: u32,
}

pub struct WatcherBootstrap {
    pub settings: Option<WatcherSettingsSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardProbeAction {
    Continue,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardIgnoreReason {
    Empty,
    Duplicate,
    TooLarge,
}

impl ClipboardIgnoreReason {
    pub fn action(self) -> ClipboardProbeAction {
        match self {
            Self::Empty => ClipboardProbeAction::Continue,
            Self::Duplicate | Self::TooLarge => ClipboardProbeAction::Stop,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Duplicate => "duplicate",
            Self::TooLarge => "too_large",
        }
    }
}

pub enum ClipboardProbeOutcome<T> {
    Accepted(T),
    Ignored(ClipboardIgnoreReason),
}

impl<T> ClipboardProbeOutcome<T> {
    pub fn action(&self) -> ClipboardProbeAction {
        match self {
            Self::Accepted(_) => ClipboardProbeAction::Stop,
            Self::Ignored(reason) => reason.action(),
        }
    }
}

pub fn should_probe_next_carrier(carrier_enabled: bool, action: ClipboardProbeAction) -> bool {
    carrier_enabled && action == ClipboardProbeAction::Continue
}

pub struct AcceptedImageChange {
    pub persist_result: Result<(), String>,
}

pub struct AcceptedTextChange {
    pub persist_result: Result<(), String>,
}

pub struct ImageIngestDeps<'a, A> {
    pub app_handle: &'a A,
    pub db: &'a Arc<Database>,
    pub data_dir: &'a Path,
    pub worker: &'a ContentJobWorker,
}

pub fn bootstrap_watcher(
    clipboard: &mut Clipboard,
    settings: &SettingsStore,
    text_dedup: &Arc<Mutex<TextDedupState>>,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
) -> WatcherBootstrap {
    // 用当前剪贴板内容初始化种子，避免启动时重复保存已有内容
    if let Ok(text) = clipboard.get_text() {
        if let Ok(mut state) = text_dedup.lock() {
            state.last_hash = Some(hash_text_content(&text));
        }
    }
    if let Ok(img) = clipboard.get_image() {
        if let Ok(mut state) = image_dedup.lock() {
            state.last_hash = Some(hash_image_content(&img));
        }
    }

    let settings =
        settings
            .load_runtime_app_settings()
            .ok()
            .map(|settings| WatcherSettingsSnapshot {
                retention: RetentionSettings {
                    expiry_seconds: settings.expiry_seconds,
                    max_history: settings.max_history,
                },
                capture_images: settings.capture_images,
            });

    WatcherBootstrap { settings }
}

pub fn accept_text_clipboard_change<A>(
    app_handle: &A,
    db: &Database,
    data_dir: &Path,
    text: String,
    source_app: &str,
    text_dedup: &Arc<Mutex<TextDedupState>>,
    retention: RetentionSettings,
) -> Result<ClipboardProbeOutcome<AcceptedTextChange>, String>
where
    A: EventEmitter,
{
    if text.is_empty() {
        debug!(
            "Ignored text clipboard change: reason={}, source_app={}",
            ClipboardIgnoreReason::Empty.as_str(),
            source_app
        );
        return Ok(ClipboardProbeOutcome::Ignored(ClipboardIgnoreReason::Empty));
    }

    let content_hash = hash_text_content(&text);
    {
        let mut state = text_dedup.lock().map_err(|e| e.to_string())?;
        if state.last_hash.as_deref() == Some(content_hash.as_str()) {
            debug!(
                "Ignored text clipboard change: reason={}, source_app={}",
                ClipboardIgnoreReason::Duplicate.as_str(),
                source_app
            );
            return Ok(ClipboardProbeOutcome::Ignored(
                ClipboardIgnoreReason::Duplicate,
            ));
        }
        // Record the hash before size filtering so repeated oversized text does not keep probing lower-priority carriers.
        state.last_hash = Some(content_hash);
    }

    if text.len() > MAX_TEXT_BYTES {
        debug!(
            "Ignored text clipboard change: reason={}, bytes={}, max_bytes={}, source_app={}",
            ClipboardIgnoreReason::TooLarge.as_str(),
            text.len(),
            MAX_TEXT_BYTES,
            source_app
        );
        return Ok(ClipboardProbeOutcome::Ignored(
            ClipboardIgnoreReason::TooLarge,
        ));
    }

    debug!(
        "Accepted text clipboard change: bytes={}, source_app={}",
        text.len(),
        source_app
    );
    Ok(ClipboardProbeOutcome::Accepted(AcceptedTextChange {
        persist_result: save_text_entry(
            app_handle,
            db,
            data_dir,
            text,
            source_app.to_owned(),
            retention.expiry_seconds,
            retention.max_history,
        ),
    }))
}

pub fn accept_image_clipboard_change<A>(
    deps: ImageIngestDeps<'_, A>,
    img: &arboard::ImageData,
    source_app: &str,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
) -> Result<ClipboardProbeOutcome<AcceptedImageChange>, String>
where
    A: EventEmitter + Clone + Send + 'static,
{
    if img.bytes.len() > MAX_IMAGE_BYTES {
        debug!(
            "Ignored image clipboard change: reason={}, bytes={}, max_bytes={}, source_app={}",
            ClipboardIgnoreReason::TooLarge.as_str(),
            img.bytes.len(),
            MAX_IMAGE_BYTES,
            source_app
        );
        // Avoid an extra full traversal of oversized RGBA data; this carrier is present, so stop.
        return Ok(ClipboardProbeOutcome::Ignored(
            ClipboardIgnoreReason::TooLarge,
        ));
    }

    let content_hash = hash_image_content(img);
    {
        let mut state = image_dedup.lock().map_err(|e| e.to_string())?;
        if state.last_hash.as_deref() == Some(content_hash.as_str()) {
            debug!(
                "Ignored image clipboard change: reason={}, source_app={}",
                ClipboardIgnoreReason::Duplicate.as_str(),
                source_app
            );
            return Ok(ClipboardProbeOutcome::Ignored(
                ClipboardIgnoreReason::Duplicate,
            ));
        }
        state.last_hash = Some(content_hash.clone());
    }

    debug!(
        "Accepted image clipboard change: bytes={}, width={}, height={}, source_app={}",
        img.bytes.len(),
        img.width,
        img.height,
        source_app
    );

    let persist_result = image_ingest::capture_image(
        CaptureImageDeps {
            app_handle: deps.app_handle,
            db: deps.db,
            data_dir: deps.data_dir,
            worker: deps.worker,
        },
        img,
        source_app.to_owned(),
        image_dedup.clone(),
        content_hash,
    );

    Ok(ClipboardProbeOutcome::Accepted(AcceptedImageChange {
        persist_result,
    }))
}

/// 保存文本条目并通知前端。
pub fn save_text_entry<A>(
    app_handle: &A,
    db: &Database,
    data_dir: &Path,
    text: String,
    source_app: String,
    expiry_seconds: i64,
    max_history: u32,
) -> Result<(), String>
where
    A: EventEmitter,
{
    let tags = detect_tags_for_text(&text);
    let entry = ClipboardEntry {
        id: Uuid::new_v4().to_string(),
        content_type: ClipboardContentType::Text,
        status: EntryStatus::Ready,
        content: text.clone(),
        canonical_search_text: build_canonical_search_text(&text),
        tags: tags.clone(),
        created_at: Utc::now().timestamp(),
        is_pinned: false,
        source_app: source_app.clone(),
    };

    pipeline::insert_ready_entry(
        app_handle,
        db,
        data_dir,
        &entry,
        &[(ENTRY_ATTR_TYPE_TAG, tags.as_slice())],
        expiry_seconds,
        max_history,
    )?;
    debug!(
        "Stored text entry: id={}, bytes={}, source_app={}, tags={}",
        entry.id,
        text.len(),
        source_app,
        tags.join(",")
    );
    Ok(())
}
