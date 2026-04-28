use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use image::GenericImageView;
use log::{debug, info, warn};

use crate::db::{Database, ImageAssetRecord};
use crate::models::ClipboardQueryStaleReason;
use crate::services::view_events::{self, EventEmitter};
use crate::utils::image::{save_thumbnail, write_image_to_file, THUMB_MAX_H, THUMB_MAX_W};

#[derive(Debug, Clone)]
pub struct ImageAssetPaths {
    pub rel_image: String,
    pub rel_thumb: String,
    pub abs_image: PathBuf,
    pub abs_thumb: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ImageAssetWriteOutcome {
    pub rel_image: String,
    pub final_thumb_rel: String,
    pub generated_thumb: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StartupImageAssetRepair {
    pub removed_ids: Vec<String>,
    pub cleared_thumbnail_ids: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImageAssetMaintenance {
    pub rebuilt_thumbnails: Vec<String>,
    pub orphan_files_removed: usize,
}

const ORPHAN_FILE_PROTECTION_WINDOW: Duration = Duration::from_secs(60);

pub fn paths_for_id(data_dir: &Path, id: &str) -> ImageAssetPaths {
    let rel_image = format!("images/{id}.png");
    let rel_thumb = format!("thumbnails/{id}.jpg");
    ImageAssetPaths {
        abs_image: data_dir.join(&rel_image),
        abs_thumb: data_dir.join(&rel_thumb),
        rel_image,
        rel_thumb,
    }
}

pub fn ensure_asset_dirs(data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir.join("images")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data_dir.join("thumbnails")).map_err(|e| e.to_string())
}

pub fn write_image_assets(
    data_dir: &Path,
    id: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<ImageAssetWriteOutcome, String> {
    ensure_asset_dirs(data_dir)?;
    let paths = paths_for_id(data_dir, id);

    if let Err(err) = write_image_to_file(&paths.abs_image, rgba, width, height) {
        cleanup_absolute_paths([paths.abs_image.as_path(), paths.abs_thumb.as_path()]);
        return Err(err);
    }

    match save_thumbnail(rgba, width, height, &paths.abs_thumb) {
        Ok(()) => Ok(ImageAssetWriteOutcome {
            rel_image: paths.rel_image,
            final_thumb_rel: paths.rel_thumb,
            generated_thumb: width > THUMB_MAX_W || height > THUMB_MAX_H,
        }),
        Err(err) => {
            cleanup_absolute_paths([paths.abs_image.as_path(), paths.abs_thumb.as_path()]);
            Err(err)
        }
    }
}

pub fn cleanup_absolute_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) {
    for path in paths {
        if let Err(err) = std::fs::remove_file(path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to remove image asset {}: {}", path.display(), err);
            }
        }
    }
}

pub fn cleanup_relative_paths(data_dir: &Path, paths: Vec<String>) {
    let mut seen = HashSet::new();
    for rel_path in paths {
        if rel_path.trim().is_empty() || !seen.insert(rel_path.clone()) {
            continue;
        }
        let path = data_dir.join(&rel_path);
        if let Err(err) = std::fs::remove_file(&path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to remove image asset {}: {}", path.display(), err);
            }
        }
    }
}

pub fn cleanup_relative_paths_async(data_dir: &Path, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let data_dir = data_dir.to_path_buf();
    std::thread::spawn(move || cleanup_relative_paths(&data_dir, paths));
}

pub fn repair_startup_image_assets(
    app: &impl EventEmitter,
    db: &Database,
    data_dir: &Path,
) -> Result<StartupImageAssetRepair, String> {
    ensure_asset_dirs(data_dir)?;
    let records = db.get_image_asset_records()?;
    let mut remove_ids = Vec::new();
    let mut cleared_thumbnail_ids = Vec::new();

    for record in &records {
        match inspect_record_lightweight(data_dir, record) {
            StartupRecordRepairAction::Keep => {}
            StartupRecordRepairAction::Remove => remove_ids.push(record.id.clone()),
            StartupRecordRepairAction::ClearThumbnail => {
                db.clear_image_thumbnail_path(&record.id)?;
                cleared_thumbnail_ids.push(record.id.clone());
            }
        }
    }

    let (removed_ids, removed_paths) = db.delete_entries_with_assets(&remove_ids)?;
    cleanup_relative_paths(data_dir, removed_paths);

    if !removed_ids.is_empty() {
        view_events::emit_entries_removed_and_mark_query_stale(
            app,
            removed_ids.clone(),
            ClipboardQueryStaleReason::SettingsOrStartup,
        )?;
    }

    if !removed_ids.is_empty() || !cleared_thumbnail_ids.is_empty() {
        info!(
            "Repaired image assets on startup: removed_entries={}, cleared_thumbnails={}",
            removed_ids.len(),
            cleared_thumbnail_ids.len()
        );
    }

    Ok(StartupImageAssetRepair {
        removed_ids,
        cleared_thumbnail_ids,
    })
}

pub fn start_image_asset_maintenance<A>(app: A, db: Arc<Database>, data_dir: PathBuf)
where
    A: EventEmitter + Clone + Send + 'static,
{
    std::thread::spawn(move || {
        if let Err(err) = run_image_asset_maintenance(&app, &db, &data_dir) {
            warn!("Failed to run image asset maintenance: {}", err);
        }
    });
}

pub fn run_image_asset_maintenance<A>(
    app: &A,
    db: &Database,
    data_dir: &Path,
) -> Result<ImageAssetMaintenance, String>
where
    A: EventEmitter,
{
    ensure_asset_dirs(data_dir)?;
    let records = db.get_image_asset_records()?;
    let mut rebuilt_thumbnails = Vec::new();

    for record in &records {
        if !needs_thumbnail_rebuild(data_dir, record) {
            continue;
        }
        match rebuild_thumbnail_for_record(db, data_dir, record) {
            Ok(_) => {
                if let Some(entry) = db.get_entry_by_id(&record.id)? {
                    if let Err(err) = view_events::emit_stream_item_updated(app, data_dir, &entry) {
                        warn!(
                            "Failed to emit rebuilt thumbnail update for image entry {}: {}",
                            record.id, err
                        );
                    }
                }
                rebuilt_thumbnails.push(record.id.clone());
            }
            Err(err) => {
                warn!(
                    "Failed to rebuild thumbnail for image entry {} during maintenance: {}",
                    record.id, err
                );
            }
        }
    }

    let latest_records = db.get_image_asset_records()?;
    let referenced = referenced_asset_paths(&latest_records);
    let orphan_files_removed =
        cleanup_orphan_asset_files(data_dir, &referenced, ORPHAN_FILE_PROTECTION_WINDOW)?;

    if orphan_files_removed > 0 {
        view_events::emit_query_results_stale(app, ClipboardQueryStaleReason::SettingsOrStartup)?;
    }

    if !rebuilt_thumbnails.is_empty() || orphan_files_removed > 0 {
        info!(
            "Completed image asset maintenance: rebuilt_thumbnails={}, orphan_files_removed={}",
            rebuilt_thumbnails.len(),
            orphan_files_removed
        );
    }

    Ok(ImageAssetMaintenance {
        rebuilt_thumbnails,
        orphan_files_removed,
    })
}

enum StartupRecordRepairAction {
    Keep,
    Remove,
    ClearThumbnail,
}

fn inspect_record_lightweight(
    data_dir: &Path,
    record: &ImageAssetRecord,
) -> StartupRecordRepairAction {
    let Some(image_path) = record.image_path.as_deref().filter(|path| !path.is_empty()) else {
        return StartupRecordRepairAction::Remove;
    };

    let image_abs = data_dir.join(image_path);
    if !image_abs.exists() {
        return StartupRecordRepairAction::Remove;
    }

    let Some(thumbnail_path) = record
        .thumbnail_path
        .as_deref()
        .filter(|path| !path.is_empty())
    else {
        return StartupRecordRepairAction::Keep;
    };

    if thumbnail_path != image_path && data_dir.join(thumbnail_path).exists() {
        StartupRecordRepairAction::Keep
    } else {
        StartupRecordRepairAction::ClearThumbnail
    }
}

fn needs_thumbnail_rebuild(data_dir: &Path, record: &ImageAssetRecord) -> bool {
    let Some(image_path) = record.image_path.as_deref().filter(|path| !path.is_empty()) else {
        return false;
    };
    if !data_dir.join(image_path).exists() {
        return false;
    }
    match record
        .thumbnail_path
        .as_deref()
        .filter(|path| !path.is_empty())
    {
        Some(thumbnail_path) if thumbnail_path != image_path => {
            !data_dir.join(thumbnail_path).exists()
        }
        _ => true,
    }
}

fn rebuild_thumbnail_for_record(
    db: &Database,
    data_dir: &Path,
    record: &ImageAssetRecord,
) -> Result<String, String> {
    let image_rel = record
        .image_path
        .as_deref()
        .ok_or_else(|| "image path missing".to_string())?;
    let image_abs = data_dir.join(image_rel);
    let img = image::open(&image_abs).map_err(|e| e.to_string())?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();
    let paths = paths_for_id(data_dir, &record.id);
    match save_thumbnail(rgba.as_raw(), width, height, &paths.abs_thumb) {
        Ok(()) => {
            db.update_image_thumbnail_path(&record.id, &paths.rel_thumb)?;
            Ok(paths.rel_thumb)
        }
        Err(err) => Err(err),
    }
}

fn cleanup_orphan_asset_files(
    data_dir: &Path,
    referenced: &HashSet<String>,
    protection_window: Duration,
) -> Result<usize, String> {
    let mut removed = 0;
    for dir_name in ["images", "thumbnails"] {
        let dir = data_dir.join(dir_name);
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let rel_path = format!(
                "{}/{}",
                dir_name,
                entry.file_name().to_string_lossy().replace('\\', "/")
            );
            if referenced.contains(&rel_path) {
                continue;
            }
            if is_recent_file(&path, protection_window) {
                continue;
            }
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    removed += 1;
                    debug!("Removed orphan image asset: {}", path.display());
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => warn!(
                    "Failed to remove orphan image asset {}: {}",
                    path.display(),
                    err
                ),
            }
        }
    }
    Ok(removed)
}

fn referenced_asset_paths(records: &[ImageAssetRecord]) -> HashSet<String> {
    records
        .iter()
        .flat_map(|record| [record.image_path.clone(), record.thumbnail_path.clone()])
        .flatten()
        .filter(|path| !path.trim().is_empty())
        .collect()
}

fn is_recent_file(path: &Path, protection_window: Duration) -> bool {
    if protection_window.is_zero() {
        return false;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = metadata.modified() else {
        return true;
    };
    match SystemTime::now().duration_since(modified) {
        Ok(age) => age < protection_window,
        Err(_) => true,
    }
}
