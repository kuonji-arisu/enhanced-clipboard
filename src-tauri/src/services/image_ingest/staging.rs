use std::path::Path;

use crate::services::artifacts::store;

pub const PIXEL_FORMAT_RGBA8: &str = "rgba8";

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
