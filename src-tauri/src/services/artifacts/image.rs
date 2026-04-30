use std::path::Path;

use image::GenericImageView;

use crate::models::{ArtifactRole, ClipboardArtifactDraft};
use crate::services::artifacts::store;
use crate::utils::image::{
    choose_display_format, display_asset_dimensions, needs_downscale, save_display_asset,
    write_image_to_file, DisplayAssetFormat,
};

pub const STAGING_PIXEL_FORMAT_RGBA8: &str = "rgba8";

#[derive(Debug, Clone)]
pub struct ImageArtifactsWriteOutcome {
    pub artifacts: Vec<ClipboardArtifactDraft>,
    pub downscaled: bool,
}

#[derive(Debug, Clone)]
pub struct DisplayRebuildOutcome {
    pub artifact: ClipboardArtifactDraft,
    pub old_candidate_paths: Vec<String>,
}

pub fn original_rel_path(id: &str) -> String {
    format!("images/{id}.png")
}

pub(crate) fn display_rel_path(id: &str, format: DisplayAssetFormat) -> String {
    format!("thumbnails/{id}.{}", format.extension())
}

pub fn staging_input_rel_path(job_id: &str) -> String {
    format!("staging/image_ingest/{job_id}.rgba")
}

pub fn display_candidate_paths(id: &str) -> Vec<String> {
    vec![
        display_rel_path(id, DisplayAssetFormat::Png),
        display_rel_path(id, DisplayAssetFormat::Jpeg),
    ]
}

pub fn generated_candidate_paths(id: &str) -> Vec<String> {
    let mut paths = vec![original_rel_path(id)];
    paths.extend(display_candidate_paths(id));
    paths
}

pub fn expected_rgba8_byte_size(width: u32, height: u32) -> Result<i64, String> {
    let bytes = (width as u64)
        .checked_mul(height as u64)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Image dimensions overflow rgba8 byte size".to_string())?;
    i64::try_from(bytes).map_err(|_| "Image rgba8 byte size is too large".to_string())
}

pub fn write_staging_rgba8(
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

pub fn read_staging_rgba8(
    data_dir: &Path,
    input_ref: &str,
    width: i64,
    height: i64,
    pixel_format: Option<&str>,
    byte_size: Option<i64>,
) -> Result<Vec<u8>, String> {
    if pixel_format != Some(STAGING_PIXEL_FORMAT_RGBA8) {
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

pub fn write_image_artifacts(
    data_dir: &Path,
    id: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<ImageArtifactsWriteOutcome, String> {
    let original_rel = original_rel_path(id);
    let original_size = store::write_temp_then_commit(data_dir, &original_rel, |path| {
        write_image_to_file(path, rgba, width, height)
    })?;

    let display_format = choose_display_format(rgba, width, height);
    let (display_width, display_height) = display_asset_dimensions(width, height);
    let display_rel = display_rel_path(id, display_format);
    let display_size = match store::write_temp_then_commit(data_dir, &display_rel, |path| {
        save_display_asset(rgba, width, height, path, display_format)
    }) {
        Ok(size) => size,
        Err(err) => {
            store::cleanup_generated_paths_for_id(data_dir, id);
            return Err(err);
        }
    };

    Ok(ImageArtifactsWriteOutcome {
        artifacts: vec![
            ClipboardArtifactDraft {
                role: ArtifactRole::Original,
                rel_path: original_rel,
                mime_type: "image/png".to_string(),
                width: Some(width as i64),
                height: Some(height as i64),
                byte_size: Some(original_size as i64),
            },
            ClipboardArtifactDraft {
                role: ArtifactRole::Display,
                rel_path: display_rel,
                mime_type: match display_format {
                    DisplayAssetFormat::Png => "image/png",
                    DisplayAssetFormat::Jpeg => "image/jpeg",
                }
                .to_string(),
                width: Some(display_width as i64),
                height: Some(display_height as i64),
                byte_size: Some(display_size as i64),
            },
        ],
        downscaled: needs_downscale(width, height),
    })
}

#[derive(Debug)]
pub enum RebuildDisplayError {
    OriginalMissing,
    OriginalBroken(String),
    DisplayWrite(String),
}

impl std::fmt::Display for RebuildDisplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OriginalMissing => write!(f, "original artifact is missing"),
            Self::OriginalBroken(err) => write!(f, "original artifact is broken: {err}"),
            Self::DisplayWrite(err) => write!(f, "display artifact write failed: {err}"),
        }
    }
}

pub fn rebuild_display_artifact(
    data_dir: &Path,
    id: &str,
    original_rel: &str,
) -> Result<DisplayRebuildOutcome, RebuildDisplayError> {
    let original_abs = store::validate_relative_path(data_dir, original_rel)
        .ok_or(RebuildDisplayError::OriginalMissing)?;
    if !original_abs.exists() {
        return Err(RebuildDisplayError::OriginalMissing);
    }

    let img = image::open(&original_abs)
        .map_err(|e| RebuildDisplayError::OriginalBroken(e.to_string()))?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();
    let display_format = choose_display_format(rgba.as_raw(), width, height);
    let (display_width, display_height) = display_asset_dimensions(width, height);
    let rel_path = display_rel_path(id, display_format);
    let byte_size = store::write_temp_then_commit(data_dir, &rel_path, |path| {
        save_display_asset(rgba.as_raw(), width, height, path, display_format)
    })
    .map_err(RebuildDisplayError::DisplayWrite)?;

    Ok(DisplayRebuildOutcome {
        artifact: ClipboardArtifactDraft {
            role: ArtifactRole::Display,
            rel_path,
            mime_type: match display_format {
                DisplayAssetFormat::Png => "image/png",
                DisplayAssetFormat::Jpeg => "image/jpeg",
            }
            .to_string(),
            width: Some(display_width as i64),
            height: Some(display_height as i64),
            byte_size: Some(byte_size as i64),
        },
        old_candidate_paths: display_candidate_paths(id),
    })
}
