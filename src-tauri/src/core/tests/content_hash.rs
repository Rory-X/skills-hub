use std::fs;

use crate::core::content_hash::hash_dir;

#[test]
fn hash_changes_with_content_and_ignores_git_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("a.txt"), b"hello").unwrap();
    fs::write(root.join("sub/b.txt"), b"world").unwrap();

    let h1 = hash_dir(root).unwrap();

    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/ignored"), b"ignored").unwrap();
    let h2 = hash_dir(root).unwrap();
    assert_eq!(h1, h2, "应忽略 .git 内容");

    fs::write(root.join("a.txt"), b"hello2").unwrap();
    let h3 = hash_dir(root).unwrap();
    assert_ne!(h2, h3);
}

#[test]
fn hash_ignores_skillshub_management_metadata() {
    let first = tempfile::tempdir().expect("first tempdir");
    let second = tempfile::tempdir().expect("second tempdir");

    fs::write(first.path().join("SKILL.md"), b"# Demo").unwrap();
    fs::write(second.path().join("SKILL.md"), b"# Demo").unwrap();
    fs::write(
        second.path().join("_skillhub_meta.json"),
        br#"{"managed":true}"#,
    )
    .unwrap();

    assert_eq!(
        hash_dir(first.path()).unwrap(),
        hash_dir(second.path()).unwrap(),
        "Skills Hub management metadata must not create a content conflict"
    );
}

#[test]
fn hash_frames_paths_and_file_contents_without_boundary_collisions() {
    let first = tempfile::tempdir().expect("first tempdir");
    let second = tempfile::tempdir().expect("second tempdir");
    fs::write(first.path().join("a"), b"bc").unwrap();
    fs::write(second.path().join("ab"), b"c").unwrap();

    assert_ne!(
        hash_dir(first.path()).unwrap(),
        hash_dir(second.path()).unwrap()
    );
}
