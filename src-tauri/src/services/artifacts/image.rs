use image::GenericImageView;
use std::path::Path;

use crate::models::{ArtifactRole, ClipboardArtifactDraft};
use crate::services::artifacts::store;
use crate::utils::image::{
    choose_display_format, display_asset_dimensions, needs_downscale, save_display_asset,
    write_image_to_file, DisplayAssetFormat,
};

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
