use image::GenericImageView;
use std::path::Path;

use crate::models::{ArtifactRole, ClipboardArtifactDraft};
use crate::services::artifacts::store;
use crate::utils::image::{
    choose_preview_format, needs_downscale, save_preview_asset, write_image_to_file,
    PreviewAssetFormat,
};

#[derive(Debug, Clone)]
pub struct ImageArtifactsWriteOutcome {
    pub artifacts: Vec<ClipboardArtifactDraft>,
    pub downscaled: bool,
}

#[derive(Debug, Clone)]
pub struct PreviewRebuildOutcome {
    pub artifact: ClipboardArtifactDraft,
    pub old_candidate_paths: Vec<String>,
}

pub fn original_rel_path(id: &str) -> String {
    format!("images/{id}.png")
}

pub(crate) fn preview_rel_path(id: &str, format: PreviewAssetFormat) -> String {
    format!("thumbnails/{id}.{}", format.extension())
}

pub fn preview_candidate_paths(id: &str) -> Vec<String> {
    vec![
        preview_rel_path(id, PreviewAssetFormat::Png),
        preview_rel_path(id, PreviewAssetFormat::Jpeg),
    ]
}

pub fn generated_candidate_paths(id: &str) -> Vec<String> {
    let mut paths = vec![original_rel_path(id)];
    paths.extend(preview_candidate_paths(id));
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
    store::write_temp_then_commit(data_dir, &original_rel, |path| {
        write_image_to_file(path, rgba, width, height)
    })?;

    let preview_format = choose_preview_format(rgba, width, height);
    let preview_rel = preview_rel_path(id, preview_format);
    if let Err(err) = store::write_temp_then_commit(data_dir, &preview_rel, |path| {
        save_preview_asset(rgba, width, height, path, preview_format)
    }) {
        store::cleanup_relative_paths(data_dir, generated_candidate_paths(id));
        return Err(err);
    }

    Ok(ImageArtifactsWriteOutcome {
        artifacts: vec![
            ClipboardArtifactDraft {
                role: ArtifactRole::Original,
                rel_path: original_rel,
                mime_type: "image/png".to_string(),
            },
            ClipboardArtifactDraft {
                role: ArtifactRole::Preview,
                rel_path: preview_rel,
                mime_type: match preview_format {
                    PreviewAssetFormat::Png => "image/png",
                    PreviewAssetFormat::Jpeg => "image/jpeg",
                }
                .to_string(),
            },
        ],
        downscaled: needs_downscale(width, height),
    })
}

#[derive(Debug)]
pub enum RebuildPreviewError {
    OriginalMissing,
    OriginalBroken(String),
    PreviewWrite(String),
}

impl std::fmt::Display for RebuildPreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OriginalMissing => write!(f, "original artifact is missing"),
            Self::OriginalBroken(err) => write!(f, "original artifact is broken: {err}"),
            Self::PreviewWrite(err) => write!(f, "preview artifact write failed: {err}"),
        }
    }
}

pub fn rebuild_preview_artifact(
    data_dir: &Path,
    id: &str,
    original_rel: &str,
) -> Result<PreviewRebuildOutcome, RebuildPreviewError> {
    let original_abs = store::validate_relative_path(data_dir, original_rel)
        .ok_or(RebuildPreviewError::OriginalMissing)?;
    if !original_abs.exists() {
        return Err(RebuildPreviewError::OriginalMissing);
    }

    let img = image::open(&original_abs)
        .map_err(|e| RebuildPreviewError::OriginalBroken(e.to_string()))?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();
    let preview_format = choose_preview_format(rgba.as_raw(), width, height);
    let rel_path = preview_rel_path(id, preview_format);
    store::write_temp_then_commit(data_dir, &rel_path, |path| {
        save_preview_asset(rgba.as_raw(), width, height, path, preview_format)
    })
    .map_err(RebuildPreviewError::PreviewWrite)?;

    Ok(PreviewRebuildOutcome {
        artifact: ClipboardArtifactDraft {
            role: ArtifactRole::Preview,
            rel_path,
            mime_type: match preview_format {
                PreviewAssetFormat::Png => "image/png",
                PreviewAssetFormat::Jpeg => "image/jpeg",
            }
            .to_string(),
        },
        old_candidate_paths: preview_candidate_paths(id),
    })
}
