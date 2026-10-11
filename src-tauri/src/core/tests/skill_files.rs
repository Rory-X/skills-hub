use std::fs;

use tempfile::tempdir;

use super::{list_files, read_file, FileContent};

#[test]
fn manifest_filters_generated_directories_and_keeps_skill_content() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Skill").unwrap();
    fs::create_dir_all(dir.path().join("assets")).unwrap();
    fs::write(dir.path().join("assets/logo.svg"), "<svg />").unwrap();
    fs::create_dir_all(dir.path().join("dist")).unwrap();
    fs::write(dir.path().join("dist/index.js"), "export {};").unwrap();

    for ignored in [".git", "node_modules", ".venv", "__pycache__", "target"] {
        fs::create_dir_all(dir.path().join(ignored)).unwrap();
        fs::write(dir.path().join(ignored).join("ignored.txt"), ignored).unwrap();
    }

    let manifest = list_files(dir.path()).unwrap();
    let paths = manifest
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(paths, ["SKILL.md", "assets/logo.svg", "dist/index.js"]);
    assert!(!manifest.truncated);
}

#[test]
fn manifest_reports_when_directory_depth_is_truncated() {
    let dir = tempdir().unwrap();
    let mut nested = dir.path().to_path_buf();
    for index in 0..24 {
        nested = nested.join(format!("level-{index}"));
    }
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("too-deep.txt"), "hidden").unwrap();

    let manifest = list_files(dir.path()).unwrap();

    assert!(manifest.entries.is_empty());
    assert!(manifest.truncated);
}

#[test]
fn read_file_returns_text_image_and_expected_unsupported_states() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("script.py"), "print('ok')\n").unwrap();
    fs::write(dir.path().join("logo.png"), [0x89, b'P', b'N', b'G']).unwrap();
    fs::write(dir.path().join("binary.bin"), [0xff, 0xfe, 0xfd]).unwrap();
    fs::write(dir.path().join("large.txt"), vec![b'x'; 1_048_577]).unwrap();

    assert_eq!(
        read_file(dir.path(), "script.py").unwrap(),
        FileContent::Text("print('ok')\n".to_string())
    );
    assert_eq!(
        read_file(dir.path(), "logo.png").unwrap(),
        FileContent::Image {
            media_type: "image/png".to_string(),
            bytes: vec![0x89, b'P', b'N', b'G'],
        }
    );
    assert_eq!(
        read_file(dir.path(), "binary.bin").unwrap(),
        FileContent::Unsupported("binary".to_string())
    );
    assert_eq!(
        read_file(dir.path(), "large.txt").unwrap(),
        FileContent::Unsupported("too_large".to_string())
    );
}

#[test]
fn read_file_rejects_paths_outside_the_skill_root() {
    let parent = tempdir().unwrap();
    let skill = parent.path().join("skill");
    fs::create_dir_all(&skill).unwrap();
    fs::write(parent.path().join("secret.txt"), "secret").unwrap();

    let error = read_file(&skill, "../secret.txt").unwrap_err();

    assert!(error.to_string().contains("path traversal"));
}
