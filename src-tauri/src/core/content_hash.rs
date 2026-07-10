use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

const IGNORE_NAMES: [&str; 5] = [
    ".git",
    ".DS_Store",
    "Thumbs.db",
    ".gitignore",
    "_skillhub_meta.json",
];

fn is_ignored(entry: &DirEntry) -> bool {
    let file_name = entry.file_name().to_string_lossy();
    IGNORE_NAMES.iter().any(|name| name == &file_name.as_ref())
}

pub fn hash_dir(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"skills-hub-content-v2");

    let mut entries = Vec::new();
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_ignored(entry))
    {
        let entry = entry?;
        if is_ignored(&entry) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(path)
            .with_context(|| format!("strip prefix {:?}", entry.path()))?;
        let relative = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        entries.push((relative, entry));
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for (relative, entry) in entries {
        let entry_type = if entry.file_type().is_dir() {
            b'd'
        } else if entry.file_type().is_file() {
            b'f'
        } else if entry.file_type().is_symlink() {
            b'l'
        } else {
            b'o'
        };
        hasher.update([entry_type]);
        update_frame(&mut hasher, relative.as_bytes());

        if entry.file_type().is_file() {
            let bytes = std::fs::read(entry.path())
                .with_context(|| format!("read file {:?}", entry.path()))?;
            update_frame(&mut hasher, &bytes);
        } else if entry.file_type().is_symlink() {
            let target = std::fs::read_link(entry.path())
                .with_context(|| format!("read symlink {:?}", entry.path()))?;
            let normalized = target.to_string_lossy().replace('\\', "/");
            update_frame(&mut hasher, normalized.as_bytes());
        }
    }

    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}

fn update_frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
#[path = "tests/content_hash.rs"]
mod tests;
