use std::fs;

use tempfile::tempdir;

use super::resolve_local_preview_root;

#[test]
fn local_preview_resolves_a_skill_subdirectory() {
    let dir = tempdir().unwrap();
    let skill = dir.path().join("skills/demo");
    fs::create_dir_all(&skill).unwrap();

    let resolved = resolve_local_preview_root(dir.path(), "skills/demo").unwrap();

    assert_eq!(resolved, skill.canonicalize().unwrap());
}

#[test]
fn local_preview_rejects_parent_and_absolute_paths() {
    let dir = tempdir().unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();

    assert!(resolve_local_preview_root(dir.path(), "../outside").is_err());
    assert!(resolve_local_preview_root(dir.path(), outside.to_str().unwrap()).is_err());
}
