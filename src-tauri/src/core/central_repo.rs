use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dirs::home_dir;
use tauri::Manager;

use super::runtime_paths::RuntimePaths;
use super::skill_store::SkillStore;

const CENTRAL_DIR_NAME: &str = ".skillshub";

pub fn resolve_central_repo_from_runtime(
    paths: &RuntimePaths,
    store: &SkillStore,
) -> Result<PathBuf> {
    if let Some(path) = store.get_setting("central_repo_path")? {
        return Ok(PathBuf::from(path));
    }
    Ok(paths.default_central_repo.clone())
}

pub fn resolve_central_repo_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
) -> Result<PathBuf> {
    if let Some(path) = store.get_setting("central_repo_path")? {
        return Ok(PathBuf::from(path));
    }

    if let Some(home) = home_dir() {
        return Ok(home.join(CENTRAL_DIR_NAME));
    }

    let base = app
        .path()
        .app_data_dir()
        .context("failed to resolve app data dir")?;
    Ok(base.join(CENTRAL_DIR_NAME))
}

pub fn ensure_central_repo(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("create {:?}", path))?;
    Ok(())
}

#[cfg(test)]
#[path = "tests/central_repo.rs"]
mod tests;
