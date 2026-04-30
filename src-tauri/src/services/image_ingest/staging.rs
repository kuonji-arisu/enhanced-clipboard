use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::services::artifacts::store;

pub const PIXEL_FORMAT_RGBA8: &str = "rgba8";

pub fn ensure_dirs(data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir.join("staging").join("image_ingest"))
        .map_err(|e| e.to_string())
}

pub fn input_rel_path(job_id: &str) -> String {
    format!("staging/image_ingest/{job_id}.rgba")
}

pub fn expected_rgba8_byte_size(width: u32, height: u32) -> Result<i64, String> {
    let bytes = (width as u64)
        .checked_mul(height as u64)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Image dimensions overflow rgba8 byte size".to_string())?;
    i64::try_from(bytes).map_err(|_| "Image rgba8 byte size is too large".to_string())
}

pub fn write_rgba8(
    data_dir: &Path,
    input_ref: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<u64, String> {
    let expected = expected_rgba8_byte_size(width, height)?;
    if rgba.len() as i64 != expected {
        return Err(format!(
            "Invalid rgba8 staging byte size: got {}, expected {}",
            rgba.len(),
            expected
        ));
    }

    store::write_temp_then_commit_cleanup_path(data_dir, input_ref, |path| {
        std::fs::write(path, rgba).map_err(|e| e.to_string())
    })
}

pub fn read_rgba8(
    data_dir: &Path,
    input_ref: &str,
    width: i64,
    height: i64,
    pixel_format: Option<&str>,
    byte_size: Option<i64>,
) -> Result<Vec<u8>, String> {
    if pixel_format != Some(PIXEL_FORMAT_RGBA8) {
        return Err("Unsupported image ingest staging pixel format".to_string());
    }
    let width =
        u32::try_from(width).map_err(|_| "Invalid image ingest staging width".to_string())?;
    let height =
        u32::try_from(height).map_err(|_| "Invalid image ingest staging height".to_string())?;
    let expected = expected_rgba8_byte_size(width, height)?;
    if byte_size != Some(expected) {
        return Err(format!(
            "Invalid image ingest staging byte size metadata: {:?}, expected {}",
            byte_size, expected
        ));
    }
    let path = store::validate_cleanup_relative_path(data_dir, input_ref)
        .ok_or_else(|| "Invalid image ingest staging input path".to_string())?;
    if !path.exists() {
        return Err("Image ingest staging input is missing".to_string());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.len() as i64 != expected {
        return Err(format!(
            "Invalid image ingest staging file size: got {}, expected {}",
            bytes.len(),
            expected
        ));
    }
    Ok(bytes)
}

pub fn scan_orphan_inputs(
    data_dir: &Path,
    referenced: &HashSet<String>,
    protection_window: Duration,
) -> Result<Vec<String>, String> {
    let root = data_dir.join("staging").join("image_ingest");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut orphans = Vec::new();
    scan_orphan_input_dir(data_dir, &root, referenced, protection_window, &mut orphans)?;
    Ok(orphans)
}

fn scan_orphan_input_dir(
    data_dir: &Path,
    dir: &Path,
    referenced: &HashSet<String>,
    protection_window: Duration,
    orphans: &mut Vec<String>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            scan_orphan_input_dir(data_dir, &path, referenced, protection_window, orphans)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let rel_path = path
            .strip_prefix(data_dir)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if store::validate_cleanup_relative_path(data_dir, &rel_path).is_none()
            || referenced.contains(&rel_path)
            || is_recent_file(&path, protection_window)
        {
            continue;
        }
        orphans.push(rel_path);
    }
    Ok(())
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
