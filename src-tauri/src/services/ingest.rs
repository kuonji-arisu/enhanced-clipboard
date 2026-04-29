use std::path::Path;
use std::sync::{Arc, Mutex};

use arboard::Clipboard;
use chrono::Utc;
use log::debug;
use tauri::AppHandle;
use uuid::Uuid;

use crate::db::{Database, SettingsStore};
use crate::models::{ClipboardEntry, EntryStatus};
use crate::services::entry_tags::{detect_tags_for_text, ENTRY_ATTR_TYPE_TAG};
use crate::services::jobs::{
    DeferredClaimRegistry, DeferredContentJob, DeferredJobContext, DeferredJobQueue, ImageAssetJob,
    ImageDedupUpdate,
};
use crate::services::pipeline;
use crate::services::search_preview::build_canonical_search_text;
use crate::services::view_events::EventEmitter;
use crate::utils::image::hash_image_content;

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
    pub last_text: String,
    pub image_dedup: Arc<Mutex<ImageDedupState>>,
    pub settings: Option<WatcherSettingsSnapshot>,
}

pub use crate::services::jobs::ImageDedupState;

pub struct AcceptedImageChange {
    pub persist_result: Result<(), String>,
}

pub struct AcceptedTextChange {
    pub last_text: String,
    pub persist_result: Result<(), String>,
}

pub struct ImageIngestDeps<'a, A, Q>
where
    Q: DeferredJobQueue,
{
    pub app_handle: &'a A,
    pub db: &'a Arc<Database>,
    pub data_dir: &'a Path,
    pub worker: &'a Q,
    pub claims: &'a DeferredClaimRegistry,
}

pub fn bootstrap_watcher(clipboard: &mut Clipboard, settings: &SettingsStore) -> WatcherBootstrap {
    let mut last_text = String::new();
    let mut image_dedup = ImageDedupState::default();

    // 用当前剪贴板内容初始化种子，避免启动时重复保存已有内容
    if let Ok(text) = clipboard.get_text() {
        last_text = text;
    }
    if let Ok(img) = clipboard.get_image() {
        image_dedup.last_hash = Some(hash_image_content(&img));
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

    WatcherBootstrap {
        last_text,
        image_dedup: Arc::new(Mutex::new(image_dedup)),
        settings,
    }
}

pub fn accept_text_clipboard_change(
    app_handle: &AppHandle,
    db: &Database,
    data_dir: &Path,
    text: String,
    source_app: &str,
    last_text: &str,
    retention: RetentionSettings,
) -> Result<Option<AcceptedTextChange>, String> {
    if text.is_empty() || text == last_text || text.len() > MAX_TEXT_BYTES {
        return Ok(None);
    }

    debug!(
        "Accepted text clipboard change: bytes={}, source_app={}",
        text.len(),
        source_app
    );
    Ok(Some(AcceptedTextChange {
        last_text: text.clone(),
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

pub fn accept_image_clipboard_change<A, Q>(
    deps: ImageIngestDeps<'_, A, Q>,
    img: &arboard::ImageData,
    source_app: &str,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
) -> Result<Option<AcceptedImageChange>, String>
where
    A: EventEmitter + Clone + Send + 'static,
    Q: DeferredJobQueue,
{
    if img.bytes.len() > MAX_IMAGE_BYTES {
        return Ok(None);
    }

    let content_hash = hash_image_content(img);
    {
        let mut state = image_dedup.lock().map_err(|e| e.to_string())?;
        if state.last_hash.as_deref() == Some(content_hash.as_str()) {
            return Ok(None);
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

    let dedup_update = ImageDedupUpdate {
        state: image_dedup.clone(),
        content_hash: content_hash.clone(),
    };
    let persist_result = save_image_entry(deps, img, source_app.to_owned(), Some(dedup_update));

    Ok(Some(AcceptedImageChange { persist_result }))
}

/// 保存文本条目并通知前端。
pub fn save_text_entry(
    app_handle: &AppHandle,
    db: &Database,
    data_dir: &Path,
    text: String,
    source_app: String,
    expiry_seconds: i64,
    max_history: u32,
) -> Result<(), String> {
    let tags = detect_tags_for_text(&text);
    let entry = ClipboardEntry {
        id: Uuid::new_v4().to_string(),
        content_type: "text".to_string(),
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

/// 保存图片条目：DB pending insert + worker queue 接受 job 后，emit
/// clipboard_stream_item_added（image_path / thumbnail_path 均为 null）。
/// worker 会等到 added effect 执行后再开始磁盘 I/O，并在完成后由 pipeline finalize。
/// 这样从剪贴板粘贴到条目出现在列表的感知延迟 < 50ms，与图片大小无关。
pub fn save_image_entry<A, Q>(
    deps: ImageIngestDeps<'_, A, Q>,
    img: &arboard::ImageData,
    source_app: String,
    dedup_update: Option<ImageDedupUpdate>,
) -> Result<(), String>
where
    A: EventEmitter + Clone + Send + 'static,
    Q: DeferredJobQueue,
{
    // Pending image inserts intentionally skip retention. The current retention
    // settings are reloaded and applied when EntryPipeline finalizes the job.
    let id = Uuid::new_v4().to_string();
    let w = img.width as u32;
    let h = img.height as u32;

    let entry = ClipboardEntry {
        id: id.clone(),
        content_type: "image".to_string(),
        status: EntryStatus::Pending,
        content: String::new(),
        canonical_search_text: String::new(),
        tags: Vec::new(),
        created_at: Utc::now().timestamp(),
        is_pinned: false,
        source_app,
    };
    debug!(
        "Queued image entry: id={}, width={}, height={}",
        entry.id, w, h
    );

    let job = DeferredContentJob::Image(ImageAssetJob {
        context: DeferredJobContext {
            entry_id: id,
            dedup_update: dedup_update.clone(),
        },
        data_dir: deps.data_dir.to_path_buf(),
        rgba: img.bytes.to_vec(),
        width: w,
        height: h,
        start_gate: None,
    });
    pipeline::insert_pending_entry(
        deps.app_handle,
        deps.db,
        deps.data_dir,
        deps.worker,
        deps.claims,
        &entry,
        job,
    )?;

    Ok(())
}
