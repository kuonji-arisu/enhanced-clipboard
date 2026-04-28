use std::path::Path;
use std::sync::{Arc, Mutex};

use arboard::Clipboard;
use chrono::Utc;
use log::{debug, error, warn};
use tauri::AppHandle;
use uuid::Uuid;

use crate::db::{Database, SettingsStore};
use crate::models::{ClipboardEntry, ClipboardQueryStaleReason};
use crate::services::entry_tags::{detect_tags_for_text, ENTRY_ATTR_TYPE_TAG};
use crate::services::search_preview::build_canonical_search_text;
use crate::services::view_events::EventEmitter;
use crate::services::{image_assets, prune, view_events};
use crate::utils::image::hash_image_content;

/// 文本条目最大字节数（1 MB）
const MAX_TEXT_BYTES: usize = 1_048_576;

/// 图片条目最大原始 RGBA 字节数（100 MB），覆盖 8K 截图场景
const MAX_IMAGE_BYTES: usize = 104_857_600;

pub struct WatcherSettingsSnapshot {
    pub expiry_seconds: i64,
    pub max_history: u32,
    pub capture_images: bool,
}

pub struct WatcherBootstrap {
    pub last_text: String,
    pub image_dedup: Arc<Mutex<ImageDedupState>>,
    pub settings: Option<WatcherSettingsSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct ImageDedupState {
    pub last_hash: Option<String>,
}

#[derive(Clone)]
pub struct ImageDedupUpdate {
    state: Arc<Mutex<ImageDedupState>>,
    content_hash: String,
}

pub struct AcceptedImageChange {
    pub persist_result: Result<(), String>,
}

pub struct AcceptedTextChange {
    pub last_text: String,
    pub persist_result: Result<(), String>,
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
                expiry_seconds: settings.expiry_seconds,
                max_history: settings.max_history,
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
    expiry_seconds: i64,
    max_history: u32,
) -> Result<Option<AcceptedTextChange>, String> {
    if text.is_empty() || text == last_text || text.len() > MAX_TEXT_BYTES {
        return Ok(None);
    }

    prune::prepare_for_insert(app_handle, db, data_dir, expiry_seconds, max_history)?;
    debug!(
        "Accepted text clipboard change: bytes={}, source_app={}",
        text.len(),
        source_app
    );
    Ok(Some(AcceptedTextChange {
        last_text: text.clone(),
        persist_result: save_text_entry(app_handle, db, text, source_app.to_owned()),
    }))
}

pub fn accept_image_clipboard_change<A>(
    app_handle: &A,
    db: &Arc<Database>,
    data_dir: &Path,
    img: &arboard::ImageData,
    source_app: &str,
    image_dedup: &Arc<Mutex<ImageDedupState>>,
    expiry_seconds: i64,
    max_history: u32,
) -> Result<Option<AcceptedImageChange>, String>
where
    A: EventEmitter + Clone + Send + 'static,
{
    if img.bytes.len() > MAX_IMAGE_BYTES {
        return Ok(None);
    }

    let content_hash = hash_image_content(img);
    let prior = image_dedup.lock().map_err(|e| e.to_string())?.clone();

    if prior.last_hash.as_deref() == Some(content_hash.as_str()) {
        return Ok(None);
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
    let persist_result = save_image_entry(
        app_handle,
        db,
        data_dir,
        img,
        source_app.to_owned(),
        expiry_seconds,
        max_history,
        Some(dedup_update),
    );

    if persist_result.is_ok() {
        let mut state = image_dedup.lock().map_err(|e| e.to_string())?;
        state.last_hash = Some(content_hash);
    }

    Ok(Some(AcceptedImageChange { persist_result }))
}

/// 图片异步链路失败时的统一回滚：
/// 1. 删除占位/半完成的 DB 记录
/// 2. 通知前端移除该条目（幂等）
/// 3. 最后再清理磁盘文件
fn rollback_image_entry(db: &Database, app: &impl EventEmitter, id: &str, data_dir: &Path) {
    error!(
        "Rolling back image entry {} after async pipeline failure",
        id
    );
    let _ = db.delete_entry(id);
    let _ = view_events::emit_entries_removed_and_mark_query_stale(
        app,
        vec![id.to_owned()],
        ClipboardQueryStaleReason::EntriesRemoved,
    );
    let paths = image_assets::paths_for_id(data_dir, id);
    image_assets::cleanup_absolute_paths([paths.abs_image.as_path(), paths.abs_thumb.as_path()]);
}

fn clear_failed_dedup(update: &ImageDedupUpdate) {
    if let Ok(mut state) = update.state.lock() {
        if state.last_hash.as_deref() == Some(update.content_hash.as_str()) {
            state.last_hash = None;
        }
    }
}

/// 将图片路径写入 DB 并返回最终条目（成功路径的公共尾部）。
fn commit_image_entry(
    db: &Database,
    id: &str,
    rel_image: &str,
    final_thumb_rel: &str,
) -> Result<Option<ClipboardEntry>, String> {
    db.finalize_image_entry(id, rel_image, Some(final_thumb_rel))
}

/// 保存文本条目并通知前端。
pub fn save_text_entry(
    app_handle: &AppHandle,
    db: &Database,
    text: String,
    source_app: String,
) -> Result<(), String> {
    let tags = detect_tags_for_text(&text);
    let entry = ClipboardEntry {
        id: Uuid::new_v4().to_string(),
        content_type: "text".to_string(),
        content: text.clone(),
        canonical_search_text: build_canonical_search_text(&text),
        tags: tags.clone(),
        created_at: Utc::now().timestamp(),
        is_pinned: false,
        source_app: source_app.clone(),
        image_path: None,
        thumbnail_path: None,
    };

    db.insert_entry_with_attrs(&entry, &[(ENTRY_ATTR_TYPE_TAG, tags.as_slice())])?;
    debug!(
        "Stored text entry: id={}, bytes={}, source_app={}, tags={}",
        entry.id,
        text.len(),
        source_app,
        tags.join(",")
    );

    let _ = view_events::emit_stream_text_item_added(app_handle, &entry);
    let _ =
        view_events::emit_query_results_stale(app_handle, ClipboardQueryStaleReason::EntryCreated);
    Ok(())
}

/// 保存图片条目：立即 DB insert + emit clipboard_stream_item_added（image_path / thumbnail_path 均为 null），
/// 全部磁盘 I/O（原图写入 + 缩略图）在后台线程完成后再 emit clipboard_stream_item_updated。
/// 这样从剪贴板粘贴到条目出现在列表的感知延迟 < 50ms，与图片大小无关。
pub fn save_image_entry<A>(
    app_handle: &A,
    db: &Arc<Database>,
    data_dir: &Path,
    img: &arboard::ImageData,
    source_app: String,
    expiry_seconds: i64,
    max_history: u32,
    dedup_update: Option<ImageDedupUpdate>,
) -> Result<(), String>
where
    A: EventEmitter + Clone + Send + 'static,
{
    let id = Uuid::new_v4().to_string();
    let w = img.width as u32;
    let h = img.height as u32;

    // 1. 立即写入 DB（image_path / thumbnail_path 暂为 null）
    let entry = ClipboardEntry {
        id: id.clone(),
        content_type: "image".to_string(),
        content: String::new(),
        canonical_search_text: String::new(),
        tags: Vec::new(),
        created_at: Utc::now().timestamp(),
        is_pinned: false,
        source_app,
        image_path: None,
        thumbnail_path: None,
    };
    db.insert_entry(&entry)?;
    debug!(
        "Queued image entry: id={}, width={}, height={}",
        entry.id, w, h
    );

    // 2. 立即通知前端（条目出现在列表，暂无图片）
    let _ = view_events::emit_stream_item_added(app_handle, data_dir, &entry);
    let _ =
        view_events::emit_query_results_stale(app_handle, ClipboardQueryStaleReason::EntryCreated);

    // 3. 后台线程：写原图 → 写缩略图 → 更新 DB → emit clipboard_stream_item_updated
    let db = db.clone();
    let app = app_handle.clone();
    let data_dir = data_dir.to_path_buf();
    let raw_bytes = img.bytes.to_vec();
    std::thread::spawn(move || {
        let asset_outcome = match image_assets::write_image_assets(&data_dir, &id, &raw_bytes, w, h)
        {
            Ok(outcome) => outcome,
            Err(e) => {
                error!("Failed to write image assets for entry {}: {}", id, e);
                if let Some(update) = dedup_update.as_ref() {
                    clear_failed_dedup(update);
                }
                rollback_image_entry(&db, &app, &id, &data_dir);
                return;
            }
        };

        match commit_image_entry(
            &db,
            &id,
            &asset_outcome.rel_image,
            &asset_outcome.final_thumb_rel,
        ) {
            Ok(Some(entry)) => {
                if let Err(err) =
                    view_events::emit_stream_item_updated(&app, data_dir.as_path(), &entry)
                {
                    warn!(
                        "Failed to emit clipboard_stream_item_updated for image entry {}: {}",
                        id, err
                    );
                }
                if let Err(err) = prune::prune_after_successful_insert(
                    &app,
                    &db,
                    &data_dir,
                    &id,
                    expiry_seconds,
                    max_history,
                ) {
                    warn!(
                        "Failed to prune after image entry {} finalized: {}",
                        id, err
                    );
                }
                debug!(
                    "Completed image entry pipeline: id={}, generated_thumb={}",
                    id, asset_outcome.generated_thumb
                );
            }
            Ok(None) => {
                debug!(
                    "Image entry {} disappeared before finalize; cleaning up generated files",
                    id
                );
                if let Some(update) = dedup_update.as_ref() {
                    clear_failed_dedup(update);
                }
                let paths = image_assets::paths_for_id(&data_dir, &id);
                image_assets::cleanup_absolute_paths([
                    paths.abs_image.as_path(),
                    paths.abs_thumb.as_path(),
                ]);
            }
            Err(e) => {
                error!("Failed to commit image entry {}: {}", id, e);
                if let Some(update) = dedup_update.as_ref() {
                    clear_failed_dedup(update);
                }
                rollback_image_entry(&db, &app, &id, &data_dir);
            }
        }
    });

    Ok(())
}
