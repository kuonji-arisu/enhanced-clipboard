use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use image::GenericImageView;
use log::{debug, info, warn};

use crate::db::{Database, ImageAssetRecord};
use crate::models::ClipboardQueryStaleReason;
use crate::services::view_events::{self, EventEmitter};
use crate::utils::image::{
    choose_display_format, needs_downscale, save_display_asset, write_image_to_file,
    DisplayAssetFormat,
};

#[derive(Debug, Clone)]
pub struct ImageAssetPaths {
    pub rel_image: String,
    pub abs_image: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ImageAssetWriteOutcome {
    pub rel_image: String,
    pub final_thumb_rel: String,
    pub downscaled: bool,
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
    ImageAssetPaths {
        abs_image: data_dir.join(&rel_image),
        rel_image,
    }
}

fn display_path_for_id(data_dir: &Path, id: &str, format: DisplayAssetFormat) -> (String, PathBuf) {
    let rel_path = format!("thumbnails/{id}.{}", format.extension());
    let abs_path = data_dir.join(&rel_path);
    (rel_path, abs_path)
}

fn display_candidate_paths(data_dir: &Path, id: &str) -> Vec<PathBuf> {
    [DisplayAssetFormat::Png, DisplayAssetFormat::Jpeg]
        .into_iter()
        .map(|format| display_path_for_id(data_dir, id, format).1)
        .collect()
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
    let display_format = choose_display_format(rgba, width, height);
    let (rel_display, abs_display) = display_path_for_id(data_dir, id, display_format);

    if let Err(err) = write_image_to_file(&paths.abs_image, rgba, width, height) {
        cleanup_generated_paths_for_id(data_dir, id);
        return Err(err);
    }

    match save_display_asset(rgba, width, height, &abs_display, display_format) {
        Ok(()) => Ok(ImageAssetWriteOutcome {
            rel_image: paths.rel_image,
            final_thumb_rel: rel_display,
            downscaled: needs_downscale(width, height),
        }),
        Err(err) => {
            cleanup_generated_paths_for_id(data_dir, id);
            Err(err)
        }
    }
}

pub fn cleanup_generated_paths_for_id(data_dir: &Path, id: &str) {
    let paths = paths_for_id(data_dir, id);
    let mut cleanup_paths = vec![paths.abs_image];
    cleanup_paths.extend(display_candidate_paths(data_dir, id));
    cleanup_absolute_paths(cleanup_paths.iter().map(|path| path.as_path()));
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
        let Some(path) = validated_asset_path(data_dir, &rel_path) else {
            warn!("Skipping invalid image asset path from DB: {}", rel_path);
            continue;
        };
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
            Err(RebuildThumbnailError::OriginalBroken(err)) => {
                warn!(
                    "Removing image entry {} after original asset decode failed: {}",
                    record.id, err
                );
                let (removed_ids, removed_paths) =
                    db.delete_entries_with_assets(std::slice::from_ref(&record.id))?;
                cleanup_relative_paths(data_dir, removed_paths);
                if !removed_ids.is_empty() {
                    view_events::emit_entries_removed_and_mark_query_stale(
                        app,
                        removed_ids,
                        ClipboardQueryStaleReason::SettingsOrStartup,
                    )?;
                }
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

#[derive(Debug)]
enum RebuildThumbnailError {
    OriginalBroken(String),
    DisplayWrite(String),
}

impl std::fmt::Display for RebuildThumbnailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OriginalBroken(err) => write!(f, "original asset is broken: {err}"),
            Self::DisplayWrite(err) => write!(f, "display asset write failed: {err}"),
        }
    }
}

fn rebuild_thumbnail_for_record(
    db: &Database,
    data_dir: &Path,
    record: &ImageAssetRecord,
) -> Result<String, RebuildThumbnailError> {
    let image_rel = record
        .image_path
        .as_deref()
        .ok_or_else(|| RebuildThumbnailError::OriginalBroken("image path missing".to_string()))?;
    let image_abs = data_dir.join(image_rel);
    let img = image::open(&image_abs)
        .map_err(|e| RebuildThumbnailError::OriginalBroken(e.to_string()))?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();
    let display_format = choose_display_format(rgba.as_raw(), width, height);
    let (rel_display, abs_display) = display_path_for_id(data_dir, &record.id, display_format);
    match save_display_asset(rgba.as_raw(), width, height, &abs_display, display_format) {
        Ok(()) => {
            db.update_image_thumbnail_path(&record.id, &rel_display)
                .map_err(RebuildThumbnailError::DisplayWrite)?;
            Ok(rel_display)
        }
        Err(err) => Err(RebuildThumbnailError::DisplayWrite(err)),
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

fn validated_asset_path(data_dir: &Path, rel_path: &str) -> Option<PathBuf> {
    let path = Path::new(rel_path);
    if path.is_absolute() {
        return None;
    }

    let mut components = path.components();
    let Some(Component::Normal(first)) = components.next() else {
        return None;
    };
    if first != "images" && first != "thumbnails" {
        return None;
    }
    let mut saw_file_component = false;
    for component in components {
        if !matches!(component, Component::Normal(_)) {
            return None;
        }
        saw_file_component = true;
    }
    saw_file_component.then(|| data_dir.join(path))
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
