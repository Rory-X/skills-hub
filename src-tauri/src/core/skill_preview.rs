use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::installer::resolve_git_preview_root;
use super::skill_files::{list_files, read_file, FileContent, FileManifest};
use super::skill_store::SkillStore;

#[derive(Debug)]
pub enum PreviewSource {
    Managed { skill_id: String },
    Local { base_path: PathBuf, subpath: String },
    Git { repo_url: String, subpath: String },
}

#[derive(Debug)]
pub struct PreviewRoot {
    pub path: PathBuf,
    pub revision: Option<String>,
}

pub fn resolve_preview_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    source: &PreviewSource,
    force_refresh: bool,
) -> Result<PreviewRoot> {
    match source {
        PreviewSource::Managed { skill_id } => {
            let record = store
                .get_skill_by_id(skill_id)?
                .with_context(|| format!("managed skill not found: {skill_id}"))?;
            Ok(PreviewRoot {
                path: PathBuf::from(record.central_path),
                revision: record.source_revision,
            })
        }
        PreviewSource::Local { base_path, subpath } => Ok(PreviewRoot {
            path: resolve_local_preview_root(base_path, subpath)?,
            revision: None,
        }),
        PreviewSource::Git { repo_url, subpath } => {
            let (path, revision) =
                resolve_git_preview_root(app, store, repo_url, subpath, force_refresh)?;
            Ok(PreviewRoot {
                path,
                revision: Some(revision),
            })
        }
    }
}

pub fn resolve_local_preview_root(base_path: &Path, subpath: &str) -> Result<PathBuf> {
    let relative = safe_relative_path(subpath)?;
    let canonical_base = base_path
        .canonicalize()
        .with_context(|| format!("resolve local preview base: {base_path:?}"))?;
    let candidate = if relative == Path::new(".") {
        canonical_base.clone()
    } else {
        canonical_base.join(relative)
    };
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("resolve local preview path: {candidate:?}"))?;
    if !canonical.starts_with(&canonical_base) || !canonical.is_dir() {
        bail!("invalid local preview path");
    }
    Ok(canonical)
}

pub fn list_preview_files(root: &PreviewRoot) -> Result<FileManifest> {
    list_files(&root.path)
}

pub fn read_preview_file(root: &PreviewRoot, relative_path: &str) -> Result<FileContent> {
    read_file(&root.path, relative_path)
}

fn safe_relative_path(path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("invalid preview subpath");
    }
    Ok(if path.trim().is_empty() {
        PathBuf::from(".")
    } else {
        relative.to_path_buf()
    })
}

#[cfg(test)]
#[path = "tests/skill_preview.rs"]
mod tests;
