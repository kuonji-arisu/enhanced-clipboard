use std::path::Path;
use std::sync::Arc;

use arboard::Clipboard;
use chrono::Utc;
use log::{debug, error, warn};
use tauri::AppHandle;
use uuid::Uuid;

use crate::db::{Database, SettingsStore};
use crate::models::ClipboardEntry;
use crate::services::entry_tags::{detect_tags_for_text, ENTRY_ATTR_TYPE_TAG};
use crate::services::{prune, view_events};
use crate::utils::image::{hash_image_sample, image_quick_fingerprint};
use crate::utils::image::{save_thumbnail, write_image_to_file};

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
    pub last_image_hash: String,
    /// 图片快速指纹，发生变化时才触发 SHA-256 哈希计算
    pub last_image_fingerprint: u64,
    pub settings: Option<WatcherSettingsSnapshot>,
}

pub struct AcceptedImageChange {
    pub last_image_hash: String,
    pub last_image_fingerprint: u64,
    pub persist_result: Result<(), String>,
}

pub struct AcceptedTextChange {
    pub last_text: String,
    pub persist_result: Result<(), String>,
}

pub fn bootstrap_watcher(clipboard: &mut Clipboard, settings: &SettingsStore) -> WatcherBootstrap {
    let mut last_text = String::new();
    let mut last_image_hash = String::new();
    let mut last_image_fingerprint = 0;

    // 用当前剪贴板内容初始化种子，避免启动时重复保存已有内容
    if let Ok(text) = clipboard.get_text() {
        last_text = text;
    }
    if let Ok(img) = clipboard.get_image() {
        last_image_hash = hash_image_sample(&img.bytes);
        last_image_fingerprint = image_quick_fingerprint(&img);
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
        last_image_hash,
        last_image_fingerprint,
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

pub fn accept_image_clipboard_change(
    app_handle: &AppHandle,
    db: &Arc<Database>,
    data_dir: &Path,
    img: &arboard::ImageData,
    source_app: &str,
    last_image_hash: &str,
    last_image_fingerprint: u64,
    expiry_seconds: i64,
    max_history: u32,
) -> Result<Option<AcceptedImageChange>, String> {
    if img.bytes.len() > MAX_IMAGE_BYTES {
        return Ok(None);
    }

    let fingerprint = image_quick_fingerprint(img);
    if fingerprint == last_image_fingerprint {
        return Ok(None);
    }

    let hash = hash_image_sample(&img.bytes);
    if hash == last_image_hash {
        return Ok(None);
    }

    prune::prepare_for_insert(app_handle, db, data_dir, expiry_seconds, max_history)?;
    debug!(
        "Accepted image clipboard change: bytes={}, width={}, height={}, source_app={}",
        img.bytes.len(),
        img.width,
        img.height,
        source_app
    );

    Ok(Some(AcceptedImageChange {
        last_image_hash: hash,
        last_image_fingerprint: fingerprint,
        persist_result: save_image_entry(app_handle, db, data_dir, img, source_app.to_owned()),
    }))
}

/// 根据缩略图生成结果确定最终使用的相对路径。
/// 返回 `None` 表示生成失败，调用方负责回滚。
fn resolve_thumb_outcome(
    result: Result<bool, String>,
    rel_image: &str,
    rel_thumb: &str,
) -> Option<String> {
    match result {
        Ok(true) => Some(rel_thumb.to_owned()),
        Ok(false) => Some(rel_image.to_owned()),
        Err(_) => None,
    }
}

fn cleanup_image_files(abs_image: &Path, abs_thumb: Option<&Path>) {
    let _ = std::fs::remove_file(abs_image);
    if let Some(path) = abs_thumb {
        let _ = std::fs::remove_file(path);
    }
}

/// 图片异步链路失败时的统一回滚：
/// 1. 删除占位/半完成的 DB 记录
/// 2. 通知前端移除该条目（幂等）
/// 3. 最后再清理磁盘文件
fn rollback_image_entry(
    db: &Database,
    app: &AppHandle,
    id: &str,
    abs_image: &Path,
    abs_thumb: Option<&Path>,
) {
    error!(
        "Rolling back image entry {} after async pipeline failure",
        id
    );
    let _ = db.delete_entry(id);
    let _ = view_events::emit_entries_removed(app, vec![id.to_owned()]);
    cleanup_image_files(abs_image, abs_thumb);
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
    Ok(())
}

/// 保存图片条目：立即 DB insert + emit clipboard_stream_item_added（image_path / thumbnail_path 均为 null），
/// 全部磁盘 I/O（原图写入 + 缩略图）在后台线程完成后再 emit clipboard_stream_item_updated。
/// 这样从剪贴板粘贴到条目出现在列表的感知延迟 < 50ms，与图片大小无关。
pub fn save_image_entry(
    app_handle: &AppHandle,
    db: &Arc<Database>,
    data_dir: &Path,
    img: &arboard::ImageData,
    source_app: String,
) -> Result<(), String> {
    let id = Uuid::new_v4().to_string();
    let rel_image = format!("images/{}.png", id);
    let rel_thumb = format!("thumbnails/{}.jpg", id);
    let abs_image = data_dir.join(&rel_image);
    let abs_thumb = data_dir.join(&rel_thumb);
    let w = img.width as u32;
    let h = img.height as u32;

    // 1. 立即写入 DB（image_path / thumbnail_path 暂为 null）
    let entry = ClipboardEntry {
        id: id.clone(),
        content_type: "image".to_string(),
        content: String::new(),
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

    // 3. 后台线程：写原图 → 写缩略图 → 更新 DB → emit clipboard_stream_item_updated
    let db = db.clone();
    let app = app_handle.clone();
    let data_dir = data_dir.to_path_buf();
    let raw_bytes = img.bytes.to_vec();
    std::thread::spawn(move || {
        // 写原图（4K 约 1-2s，原先阻塞主线程的瓶颈，现在异步化）
        if let Err(e) = write_image_to_file(&abs_image, &raw_bytes, w, h) {
            error!("Failed to write source image for entry {}: {}", id, e);
            rollback_image_entry(&db, &app, &id, &abs_image, None);
            return;
        }

        // 生成缩略图；失败时删原图 + 回滚
        let Some(final_thumb_rel) = resolve_thumb_outcome(
            save_thumbnail(&raw_bytes, w, h, &abs_thumb),
            &rel_image,
            &rel_thumb,
        ) else {
            error!("Failed to build thumbnail for entry {}", id);
            rollback_image_entry(&db, &app, &id, &abs_image, Some(&abs_thumb));
            return;
        };

        let thumb_file = (final_thumb_rel != rel_image).then_some(abs_thumb.as_path());
        match commit_image_entry(&db, &id, &rel_image, &final_thumb_rel) {
            Ok(Some(entry)) => {
                if let Err(err) =
                    view_events::emit_stream_item_updated(&app, data_dir.as_path(), &entry)
                {
                    warn!(
                        "Failed to emit clipboard_stream_item_updated for image entry {}: {}",
                        id, err
                    );
                }
                debug!("Completed image entry pipeline: id={}", id);
            }
            Ok(None) => {
                debug!(
                    "Image entry {} disappeared before finalize; cleaning up generated files",
                    id
                );
                cleanup_image_files(&abs_image, thumb_file);
            }
            Err(e) => {
                error!("Failed to commit image entry {}: {}", id, e);
                rollback_image_entry(&db, &app, &id, &abs_image, thumb_file);
            }
        }
    });

    Ok(())
}
