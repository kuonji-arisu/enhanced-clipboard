use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use image::GenericImageView;
use log::warn;

use crate::utils::image::{
    choose_preview_format, save_preview_asset, write_image_to_file, PreviewAssetFormat,
};

const IMAGE_ROOT: &str = "images";
const THUMBNAIL_ROOT: &str = "thumbnails";
const MANAGED_ROOTS: [&str; 2] = [IMAGE_ROOT, THUMBNAIL_ROOT];

/// The clipboard engine's filesystem boundary for committed image assets.
///
/// The type deliberately owns only the data-directory path. It is not `Clone`:
/// the engine should keep the sole instance next to its SQLite connection.
#[derive(Debug)]
pub(crate) struct ImageArtifacts {
    data_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WrittenImageArtifacts {
    pub original_rel_path: String,
    pub preview_rel_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RebuiltPreview {
    pub preview_rel_path: String,
    /// Deterministic preview candidates superseded by the rebuilt asset.
    /// The newly written path is never included.
    pub obsolete_preview_rel_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageReadError {
    Missing,
    Broken(String),
}

impl fmt::Display for ImageReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(formatter, "image original is missing"),
            Self::Broken(error) => write!(formatter, "image original is broken: {error}"),
        }
    }
}

impl std::error::Error for ImageReadError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RebuildPreviewError {
    OriginalMissing,
    OriginalBroken(String),
    PreviewWrite(String),
}

impl fmt::Display for RebuildPreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OriginalMissing => write!(formatter, "original artifact is missing"),
            Self::OriginalBroken(error) => {
                write!(formatter, "original artifact is broken: {error}")
            }
            Self::PreviewWrite(error) => {
                write!(formatter, "preview artifact write failed: {error}")
            }
        }
    }
}

impl std::error::Error for RebuildPreviewError {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StartupCleanupReport {
    pub removed_files: usize,
    pub failed_removals: usize,
}

impl ImageArtifacts {
    pub(crate) fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn ensure_dirs(&self) -> Result<(), String> {
        for root in MANAGED_ROOTS {
            ensure_real_directory(&self.data_dir.join(root))?;
        }
        Ok(())
    }

    /// Removes the complete committed-asset roots after a clipboard schema
    /// rebuild, then recreates only the two roots owned by this module.
    pub(crate) fn reset_roots(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for root in MANAGED_ROOTS {
            let path = self.data_dir.join(root);
            if let Err(error) = remove_managed_root(&path) {
                errors.push(format!(
                    "Failed to wipe managed image directory {}: {error}",
                    path.display()
                ));
            }
        }
        for root in MANAGED_ROOTS {
            let path = self.data_dir.join(root);
            if let Err(error) = ensure_real_directory(&path) {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Deletes temporary files and committed files no longer referenced by the
    /// freshly opened database. Cleanup failures are warnings, not startup
    /// failures; inability to establish safe managed roots remains fatal.
    pub(crate) fn cleanup_startup(
        &self,
        referenced_paths: &HashSet<String>,
    ) -> Result<StartupCleanupReport, String> {
        self.ensure_dirs()?;
        let mut report = StartupCleanupReport::default();

        for root in MANAGED_ROOTS {
            let root_path = self.data_dir.join(root);
            let entries = match std::fs::read_dir(&root_path) {
                Ok(entries) => entries,
                Err(error) => {
                    warn!(
                        "Failed to scan managed image directory {}: {}",
                        root_path.display(),
                        error
                    );
                    report.failed_removals += 1;
                    continue;
                }
            };

            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        warn!(
                            "Failed to inspect managed image directory {}: {}",
                            root_path.display(),
                            error
                        );
                        report.failed_removals += 1;
                        continue;
                    }
                };
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(error) => {
                        warn!(
                            "Failed to inspect managed image artifact {}: {}",
                            entry.path().display(),
                            error
                        );
                        report.failed_removals += 1;
                        continue;
                    }
                };

                // Artifacts are always direct files. Never recurse into an
                // unexpected directory or junction during normal startup.
                if file_type.is_dir() && !file_type.is_symlink() {
                    warn!(
                        "Skipping unexpected directory in managed image root: {}",
                        entry.path().display()
                    );
                    report.failed_removals += 1;
                    continue;
                }

                let file_name = entry.file_name();
                let Some(file_name) = file_name.to_str() else {
                    warn!(
                        "Removing non-UTF-8 artifact from managed image root: {}",
                        entry.path().display()
                    );
                    remove_startup_entry(&entry.path(), &mut report);
                    continue;
                };
                let rel_path = format!("{root}/{file_name}");
                if file_type.is_symlink()
                    || is_temp_file_name(file_name)
                    || !referenced_paths.contains(&rel_path)
                {
                    remove_startup_entry(&entry.path(), &mut report);
                }
            }
        }

        Ok(report)
    }

    pub(crate) fn write_image(
        &self,
        id: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<WrittenImageArtifacts, String> {
        validate_rgba(rgba, width, height)?;
        self.ensure_dirs()?;

        let original_rel_path = original_rel_path(id);
        let preview_format = choose_preview_format(rgba, width, height);
        let preview_rel_path = preview_rel_path(id, preview_format);

        write_temp_then_rename(&self.data_dir, &original_rel_path, |path| {
            write_image_to_file(path, rgba, width, height)
        })?;

        if let Err(error) = write_temp_then_rename(&self.data_dir, &preview_rel_path, |path| {
            save_preview_asset(rgba, width, height, path, preview_format)
        }) {
            self.cleanup_paths(&[original_rel_path.clone(), preview_rel_path.clone()]);
            return Err(error);
        }

        Ok(WrittenImageArtifacts {
            original_rel_path,
            preview_rel_path,
        })
    }

    pub(crate) fn resolve_original_path(&self, rel_path: &str) -> Result<PathBuf, ImageReadError> {
        let path = validate_relative_path_for_root(&self.data_dir, rel_path, IMAGE_ROOT)
            .ok_or_else(|| ImageReadError::Broken("invalid managed path".to_string()))?;
        if !path.is_file() {
            return Err(ImageReadError::Missing);
        }
        Ok(path)
    }

    pub(crate) fn read_original_rgba(
        &self,
        rel_path: &str,
    ) -> Result<DecodedImage, ImageReadError> {
        let path = self.resolve_original_path(rel_path)?;
        let image = image::open(path).map_err(|error| ImageReadError::Broken(error.to_string()))?;
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err(ImageReadError::Broken(
                "decoded image has zero dimensions".to_string(),
            ));
        }
        Ok(DecodedImage {
            rgba: image.to_rgba8().into_raw(),
            width,
            height,
        })
    }

    pub(crate) fn rebuild_preview(
        &self,
        id: &str,
        original_rel_path: &str,
    ) -> Result<RebuiltPreview, RebuildPreviewError> {
        let decoded = self
            .read_original_rgba(original_rel_path)
            .map_err(|error| match error {
                ImageReadError::Missing => RebuildPreviewError::OriginalMissing,
                ImageReadError::Broken(error) => RebuildPreviewError::OriginalBroken(error),
            })?;
        let preview_format = choose_preview_format(&decoded.rgba, decoded.width, decoded.height);
        // Repair into a fresh final path. The old preview remains valid until
        // the DB transaction switches paths, so a failed rename cannot leave
        // the entry pointing at a file that was removed first on Windows.
        let preview_rel_path = rebuilt_preview_rel_path(id, preview_format);
        write_temp_then_rename(&self.data_dir, &preview_rel_path, |path| {
            save_preview_asset(
                &decoded.rgba,
                decoded.width,
                decoded.height,
                path,
                preview_format,
            )
        })
        .map_err(RebuildPreviewError::PreviewWrite)?;

        let obsolete_preview_rel_paths = preview_candidate_paths(id)
            .into_iter()
            .filter(|candidate| candidate != &preview_rel_path)
            .collect();
        Ok(RebuiltPreview {
            preview_rel_path,
            obsolete_preview_rel_paths,
        })
    }

    /// Synchronous best-effort post-commit cleanup. Invalid DB paths and I/O
    /// failures are logged and never turn an already committed mutation into a
    /// command failure.
    pub(crate) fn cleanup_paths(&self, paths: &[String]) {
        let mut seen = HashSet::new();
        for rel_path in paths {
            if !seen.insert(rel_path.as_str()) {
                continue;
            }
            let Some(path) = validate_relative_path(&self.data_dir, rel_path) else {
                warn!("Skipping invalid managed image path from DB: {rel_path}");
                continue;
            };
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        "Failed to remove managed image artifact {}: {}",
                        path.display(),
                        error
                    );
                }
            }
        }
    }
}

pub(crate) fn validate_relative_path(data_dir: &Path, rel_path: &str) -> Option<PathBuf> {
    let root = first_root(rel_path)?;
    if !MANAGED_ROOTS.contains(&root) {
        return None;
    }
    validate_relative_path_for_root(data_dir, rel_path, root)
}

fn validate_relative_path_for_root(
    data_dir: &Path,
    rel_path: &str,
    expected_root: &str,
) -> Option<PathBuf> {
    if rel_path.trim().is_empty() {
        return None;
    }
    let path = Path::new(rel_path);
    if path.is_absolute() {
        return None;
    }
    let mut components = path.components();
    let Some(Component::Normal(root)) = components.next() else {
        return None;
    };
    if root.to_str()? != expected_root {
        return None;
    }
    let Some(Component::Normal(file_name)) = components.next() else {
        return None;
    };
    if file_name.is_empty() || components.next().is_some() {
        return None;
    }
    let managed_root = data_dir.join(expected_root);
    if std::fs::symlink_metadata(&managed_root)
        .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
    {
        return None;
    }
    let resolved = managed_root.join(file_name);
    if std::fs::symlink_metadata(&resolved).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return None;
    }
    Some(resolved)
}

fn first_root(rel_path: &str) -> Option<&str> {
    match Path::new(rel_path).components().next()? {
        Component::Normal(root) => root.to_str(),
        _ => None,
    }
}

fn original_rel_path(id: &str) -> String {
    format!("{IMAGE_ROOT}/{id}.png")
}

fn preview_rel_path(id: &str, format: PreviewAssetFormat) -> String {
    format!("{THUMBNAIL_ROOT}/{id}.{}", format.extension())
}

fn rebuilt_preview_rel_path(id: &str, format: PreviewAssetFormat) -> String {
    format!(
        "{THUMBNAIL_ROOT}/{id}-{}.{}",
        uuid::Uuid::new_v4(),
        format.extension()
    )
}

fn preview_candidate_paths(id: &str) -> [String; 2] {
    [
        preview_rel_path(id, PreviewAssetFormat::Png),
        preview_rel_path(id, PreviewAssetFormat::Jpeg),
    ]
}

fn validate_rgba(rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Invalid image dimensions".to_string());
    }
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| "Invalid image dimensions".to_string())?;
    if rgba.len() != expected {
        return Err(format!(
            "Invalid RGBA image buffer: got {} bytes, expected {expected}",
            rgba.len()
        ));
    }
    Ok(())
}

fn write_temp_then_rename<F>(data_dir: &Path, rel_path: &str, writer: F) -> Result<(), String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let final_path = validate_relative_path(data_dir, rel_path)
        .ok_or_else(|| format!("Invalid managed image path: {rel_path}"))?;
    let parent = final_path
        .parent()
        .ok_or_else(|| format!("Invalid managed image path: {rel_path}"))?;
    ensure_real_directory(parent)?;

    let file_stem = final_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact");
    let extension = final_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin");
    let temp_path = final_path.with_file_name(format!(
        "{file_stem}.{}.tmp.{extension}",
        uuid::Uuid::new_v4()
    ));

    if let Err(error) = writer(&temp_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    if let Err(error) = replace_temp_file(&temp_path, &final_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!(
            "Failed to commit image artifact {}: {error}",
            final_path.display()
        ));
    }
    Ok(())
}

fn replace_temp_file(temp_path: &Path, final_path: &Path) -> std::io::Result<()> {
    match std::fs::rename(temp_path, final_path) {
        Ok(()) => Ok(()),
        Err(first_error) if final_path.exists() => {
            std::fs::remove_file(final_path)?;
            std::fs::rename(temp_path, final_path).map_err(|second_error| {
                std::io::Error::new(
                    second_error.kind(),
                    format!(
                        "failed to replace existing artifact after rename error ({first_error}): {second_error}"
                    ),
                )
            })
        }
        Err(error) => Err(error),
    }
}

fn ensure_real_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "Managed image directory must not be a symlink: {}",
            path.display()
        )),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "Managed image directory is not a directory: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir_all(path)
            .map_err(|error| {
                format!(
                    "Failed to create managed image directory {}: {error}",
                    path.display()
                )
            }),
        Err(error) => Err(format!(
            "Failed to inspect managed image directory {}: {error}",
            path.display()
        )),
    }
}

fn remove_managed_root(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        // A Windows directory junction must be removed as a directory, while
        // a file symlink must be removed as a file. Neither operation follows
        // the link into an external tree.
        std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path))
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

fn is_temp_file_name(file_name: &str) -> bool {
    file_name.contains(".tmp.") || file_name.ends_with(".tmp")
}

fn remove_startup_entry(path: &Path, report: &mut StartupCleanupReport) {
    let removal = std::fs::remove_file(path).or_else(|file_error| {
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            std::fs::remove_dir(path)
        } else {
            Err(file_error)
        }
    });
    if let Err(error) = removal {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(
                "Failed to remove temporary or orphan image artifact {}: {}",
                path.display(),
                error
            );
            report.failed_removals += 1;
        }
    } else {
        report.removed_files += 1;
    }
}
