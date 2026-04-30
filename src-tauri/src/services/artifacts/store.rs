use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use log::{debug, warn};

/// Persistent files under these roots are owned by clipboard_entry_artifacts.
/// Orphan cleanup scans them recursively and removes old files that no DB row references.
pub const ALLOWED_ARTIFACT_ROOTS: &[&str] = &["images", "thumbnails", "files", "previews"];
pub const ALLOWED_CLEANUP_ROOTS: &[&str] =
    &["images", "thumbnails", "files", "previews", "staging"];
pub const ORPHAN_FILE_PROTECTION_WINDOW: Duration = Duration::from_secs(60);

pub fn ensure_artifact_dirs(data_dir: &Path) -> Result<(), String> {
    for root in ALLOWED_ARTIFACT_ROOTS {
        std::fs::create_dir_all(data_dir.join(root)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn validate_relative_path(data_dir: &Path, rel_path: &str) -> Option<PathBuf> {
    validate_relative_path_for_roots(data_dir, rel_path, ALLOWED_ARTIFACT_ROOTS)
}

pub fn validate_cleanup_relative_path(data_dir: &Path, rel_path: &str) -> Option<PathBuf> {
    validate_relative_path_for_roots(data_dir, rel_path, ALLOWED_CLEANUP_ROOTS)
}

fn validate_relative_path_for_roots(
    data_dir: &Path,
    rel_path: &str,
    allowed_roots: &[&str],
) -> Option<PathBuf> {
    if rel_path.trim().is_empty() {
        return None;
    }

    let path = Path::new(rel_path);
    if path.is_absolute() {
        return None;
    }

    let mut components = path.components();
    let Some(Component::Normal(first)) = components.next() else {
        return None;
    };
    let first = first.to_string_lossy();
    if !allowed_roots.iter().any(|root| *root == first) {
        return None;
    }

    let mut saw_child = false;
    for component in components {
        if !matches!(component, Component::Normal(_)) {
            return None;
        }
        saw_child = true;
    }

    saw_child.then(|| data_dir.join(path))
}

pub fn write_temp_then_commit<F>(data_dir: &Path, rel_path: &str, writer: F) -> Result<u64, String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    ensure_artifact_dirs(data_dir)?;
    write_temp_then_commit_validated(data_dir, rel_path, validate_relative_path, writer)
}

pub fn write_temp_then_commit_cleanup_path<F>(
    data_dir: &Path,
    rel_path: &str,
    writer: F,
) -> Result<u64, String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    write_temp_then_commit_validated(data_dir, rel_path, validate_cleanup_relative_path, writer)
}

fn write_temp_then_commit_validated<F>(
    data_dir: &Path,
    rel_path: &str,
    validator: fn(&Path, &str) -> Option<PathBuf>,
    writer: F,
) -> Result<u64, String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let final_path = validator(data_dir, rel_path)
        .ok_or_else(|| format!("Invalid artifact path: {rel_path}"))?;
    let Some(parent) = final_path.parent() else {
        return Err(format!("Invalid artifact path: {rel_path}"));
    };
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp_path = {
        let file_name = final_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact");
        let extension = final_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("tmp");
        final_path.with_file_name(format!(
            "{file_name}.{}.tmp.{extension}",
            uuid::Uuid::new_v4()
        ))
    };

    if let Err(err) = writer(&temp_path) {
        let _ = std::fs::remove_file(&temp_path);
        let _ = std::fs::remove_file(&final_path);
        return Err(err);
    }

    replace_temp_file(&temp_path, &final_path).map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        e.to_string()
    })?;
    std::fs::metadata(&final_path)
        .map(|metadata| metadata.len())
        .map_err(|e| e.to_string())
}

fn replace_temp_file(temp_path: &Path, final_path: &Path) -> std::io::Result<()> {
    match std::fs::rename(temp_path, final_path) {
        Ok(()) => Ok(()),
        Err(rename_err) if final_path.exists() => {
            std::fs::remove_file(final_path)?;
            std::fs::rename(temp_path, final_path).map_err(|second_err| {
                std::io::Error::new(
                    second_err.kind(),
                    format!(
                        "failed to replace existing artifact after rename error ({rename_err}): {second_err}"
                    ),
                )
            })
        }
        Err(rename_err) => Err(rename_err),
    }
}

pub fn cleanup_relative_paths(data_dir: &Path, paths: Vec<String>) {
    let mut seen = HashSet::new();
    for rel_path in paths {
        if !seen.insert(rel_path.clone()) {
            continue;
        }
        let Some(path) = validate_cleanup_relative_path(data_dir, &rel_path) else {
            warn!("Skipping invalid artifact path from DB: {}", rel_path);
            continue;
        };
        if let Err(err) = std::fs::remove_file(&path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to remove artifact {}: {}", path.display(), err);
            }
        }
    }
}

pub fn cleanup_generated_paths_for_id(data_dir: &Path, id: &str) {
    cleanup_relative_paths(
        data_dir,
        vec![
            format!("images/{id}.png"),
            format!("thumbnails/{id}.png"),
            format!("thumbnails/{id}.jpg"),
        ],
    );
}

pub fn scan_orphan_artifact_paths(
    data_dir: &Path,
    referenced: &HashSet<String>,
    protection_window: Duration,
) -> Result<Vec<String>, String> {
    let mut orphans = Vec::new();
    for root in ALLOWED_ARTIFACT_ROOTS {
        let dir = data_dir.join(root);
        if !dir.exists() {
            continue;
        }
        scan_orphan_artifact_dir(data_dir, &dir, referenced, protection_window, &mut orphans)?;
    }
    Ok(orphans)
}

fn scan_orphan_artifact_dir(
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
            scan_orphan_artifact_dir(data_dir, &path, referenced, protection_window, orphans)?;
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
        if validate_relative_path(data_dir, &rel_path).is_none()
            || referenced.contains(&rel_path)
            || is_recent_file(&path, protection_window)
        {
            continue;
        }
        debug!("Found orphan artifact: {}", path.display());
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
