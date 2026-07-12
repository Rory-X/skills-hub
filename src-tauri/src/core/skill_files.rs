use std::path::{Component, Path};

use anyhow::{bail, Context, Result};
use walkdir::{DirEntry, WalkDir};

const IGNORE_NAMES: [&str; 9] = [
    ".git",
    ".DS_Store",
    "Thumbs.db",
    ".gitignore",
    "node_modules",
    ".venv",
    "__pycache__",
    "target",
    ".cache",
];
const MAX_FILE_SIZE: u64 = 1_048_576;
const MAX_FILES: usize = 2_000;
const MAX_DEPTH: usize = 20;

fn is_ignored(entry: &DirEntry) -> bool {
    let file_name = entry.file_name().to_string_lossy();
    IGNORE_NAMES.iter().any(|name| name == &file_name.as_ref())
}

#[derive(Debug)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug)]
pub struct FileManifest {
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
}

#[derive(Debug, PartialEq)]
pub enum FileContent {
    Text(String),
    Image { media_type: String, bytes: Vec<u8> },
    Unsupported(String),
}

pub fn list_files(skill_root: &Path) -> Result<FileManifest> {
    let mut entries = Vec::new();
    let mut truncated = false;

    let walker = WalkDir::new(skill_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_ignored(entry));

    for entry in walker {
        let entry = entry?;
        if entry.depth() > MAX_DEPTH {
            truncated = true;
            continue;
        }
        if !entry.file_type().is_file() || is_ignored(&entry) {
            continue;
        }
        if entries.len() == MAX_FILES {
            truncated = true;
            break;
        }

        let relative = entry
            .path()
            .strip_prefix(skill_root)
            .with_context(|| format!("strip prefix {:?}", entry.path()))?;
        let path = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        entries.push(FileEntry {
            path,
            size: entry.metadata()?.len(),
        });
    }

    entries.sort_by(|a, b| {
        let a_is_skill = a.path.eq_ignore_ascii_case("skill.md");
        let b_is_skill = b.path.eq_ignore_ascii_case("skill.md");
        match (a_is_skill, b_is_skill) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
        }
    });

    Ok(FileManifest { entries, truncated })
}

pub fn read_file(skill_root: &Path, relative_path: &str) -> Result<FileContent> {
    let relative = Path::new(relative_path);
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("Invalid file path: path traversal not allowed");
    }

    let full_path = skill_root.join(relative);
    let canonical = full_path
        .canonicalize()
        .with_context(|| format!("resolve path: {:?}", full_path))?;
    let canonical_base = skill_root
        .canonicalize()
        .with_context(|| format!("resolve base: {:?}", skill_root))?;

    if !canonical.starts_with(&canonical_base) {
        bail!("Invalid file path: outside skill directory");
    }

    let metadata =
        std::fs::metadata(&canonical).with_context(|| format!("read metadata: {:?}", canonical))?;
    if !metadata.is_file() {
        bail!("Invalid file path: not a file");
    }
    if metadata.len() > MAX_FILE_SIZE {
        return Ok(FileContent::Unsupported("too_large".to_string()));
    }

    let bytes = std::fs::read(&canonical).with_context(|| format!("read file: {:?}", canonical))?;
    if let Some(media_type) = image_media_type(relative_path) {
        return Ok(FileContent::Image {
            media_type: media_type.to_string(),
            bytes,
        });
    }

    match String::from_utf8(bytes) {
        Ok(content) => Ok(FileContent::Text(content)),
        Err(_) => Ok(FileContent::Unsupported("binary".to_string())),
    }
}

fn image_media_type(path: &str) -> Option<&'static str> {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/skill_files.rs"]
mod tests;
