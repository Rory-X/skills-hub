use std::fs;
use std::path::{Path, PathBuf};

use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SkillStore::new(dir.path().join("test.db"));
    store.ensure_schema().expect("ensure_schema");
    (dir, store)
}

fn set_central_path(store: &SkillStore, central: &Path) {
    store
        .set_setting("central_repo_path", central.to_string_lossy().as_ref())
        .unwrap();
}

fn init_git_repo(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    let sig = git2::Signature::now("t", "t@example.com").unwrap();

    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    let tree_id = index.write_tree().unwrap();
    {
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    repo
}

fn commit_all(repo: &git2::Repository, msg: &str) -> git2::Oid {
    let sig = git2::Signature::now("t", "t@example.com").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();

    let parent = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok());
    match parent {
        Some(p) => repo
            .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&p])
            .unwrap(),
        None => repo
            .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[])
            .unwrap(),
    }
}

#[test]
fn parses_github_urls() {
    let p = super::parse_github_url("https://github.com/owner/repo");
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert!(p.branch.is_none());
    assert!(p.subpath.is_none());

    let p = super::parse_github_url("anthropics/skills");
    assert_eq!(p.clone_url, "https://github.com/anthropics/skills.git");
    assert!(p.branch.is_none());
    assert!(p.subpath.is_none());

    let p = super::parse_github_url("github.com/owner/repo");
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert!(p.branch.is_none());
    assert!(p.subpath.is_none());

    let p = super::parse_github_url("https://github.com/owner/repo/tree/main/skills/x");
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("skills/x"));

    let p = super::parse_github_url("owner/repo/tree/main/skills/x");
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("skills/x"));

    let p = super::parse_github_url("https://github.com/owner/repo/blob/main/skills/x/SKILL.md");
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("skills/x"));

    let p = super::parse_github_url("https://github.com/owner/repo/blob/main/SKILL.md");
    assert_eq!(p.clone_url, "https://github.com/owner/repo.git");
    assert_eq!(p.branch.as_deref(), Some("main"));
    assert_eq!(p.subpath.as_deref(), Some("."));

    let p = super::parse_github_url("/local/path/to/repo");
    assert_eq!(p.clone_url, "/local/path/to/repo");
}

#[test]
fn parses_skill_md_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(
        &p,
        r#"---
name: "My Skill"
description: "Desc"
---

body
"#,
    )
    .unwrap();

    let (name, desc) = super::parse_skill_md(&p).unwrap();
    assert_eq!(name, "My Skill");
    assert_eq!(desc.as_deref(), Some("Desc"));
}

#[test]
fn parses_skill_md_frontmatter_literal_description() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(
        &p,
        r#"---
name: technical-writer
description: |
  Creates clear documentation, API references, guides, and
  technical content for developers and users.
author: awesome-llm-apps
---

body
"#,
    )
    .unwrap();

    let (name, desc) = super::parse_skill_md(&p).unwrap();
    assert_eq!(name, "technical-writer");
    assert_eq!(
        desc.as_deref(),
        Some("Creates clear documentation, API references, guides, and\ntechnical content for developers and users.")
    );
}

#[test]
fn parses_skill_md_frontmatter_folded_chomp_description() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("SKILL.md");
    fs::write(
        &p,
        r#"---
name: fireworks-tech-graph
description: >-
  Use when the user wants to create any technical diagram - architecture, data
  flow, flowchart, sequence, agent/memory, or concept map - and export as
  SVG+PNG.
---

body
"#,
    )
    .unwrap();

    let (name, desc) = super::parse_skill_md(&p).unwrap();
    assert_eq!(name, "fireworks-tech-graph");
    assert_eq!(
        desc.as_deref(),
        Some(
            "Use when the user wants to create any technical diagram - architecture, data flow, flowchart, sequence, agent/memory, or concept map - and export as SVG+PNG."
        )
    );
}

#[test]
fn backfill_skill_descriptions_replaces_stale_frontmatter_marker() {
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    fs::write(
        central.path().join("SKILL.md"),
        r#"---
name: fireworks-tech-graph
description: >-
  Correct folded description.
---
"#,
    )
    .unwrap();

    store
        .upsert_skill(&SkillRecord {
            id: "fireworks".to_string(),
            name: "fireworks-tech-graph".to_string(),
            description: Some(">-".to_string()),
            source_type: "local".to_string(),
            source_ref: None,
            source_subpath: None,
            source_revision: None,
            central_path: central.path().to_string_lossy().to_string(),
            content_hash: None,
            created_at: 1,
            updated_at: 1,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    super::backfill_skill_descriptions(&store);

    let skill = store.get_skill_by_id("fireworks").unwrap().unwrap();
    assert_eq!(
        skill.description.as_deref(),
        Some("Correct folded description.")
    );
}

#[test]
fn backfill_skill_content_hashes_populates_legacy_records() {
    let (_dir, store) = make_store();
    let central = tempfile::tempdir().unwrap();
    fs::write(central.path().join("SKILL.md"), "same content").unwrap();
    let expected_hash = crate::core::content_hash::hash_dir(central.path()).unwrap();
    store
        .upsert_skill(&SkillRecord {
            id: "legacy-skill".to_string(),
            name: "legacy-skill".to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: None,
            source_subpath: None,
            source_revision: None,
            central_path: central.path().to_string_lossy().to_string(),
            content_hash: Some("legacy-hash".to_string()),
            created_at: 1,
            updated_at: 1,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    assert_eq!(super::backfill_skill_content_hashes(&store), 1);
    assert_eq!(super::backfill_skill_content_hashes(&store), 0);
    assert_eq!(
        store
            .get_skill_by_id("legacy-skill")
            .unwrap()
            .unwrap()
            .content_hash
            .as_deref(),
        Some(expected_hash.as_str())
    );
}

#[test]
fn hash_migration_retries_a_legacy_skill_when_its_directory_returns() {
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    let central_path = central_root.path().join("temporarily-missing");
    store
        .upsert_skill(&SkillRecord {
            id: "missing-legacy-skill".to_string(),
            name: "missing-legacy-skill".to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: None,
            source_subpath: None,
            source_revision: None,
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: Some("legacy-hash".to_string()),
            created_at: 1,
            updated_at: 1,
            last_sync_at: None,
            last_seen_at: 1,
            enabled: true,
            status: "ok".to_string(),
        })
        .unwrap();

    assert_eq!(super::backfill_skill_content_hashes(&store), 0);
    assert!(store
        .get_skill_by_id("missing-legacy-skill")
        .unwrap()
        .unwrap()
        .content_hash
        .is_none());

    fs::create_dir_all(&central_path).unwrap();
    fs::write(central_path.join("SKILL.md"), "restored").unwrap();
    assert_eq!(super::backfill_skill_content_hashes(&store), 1);
    assert!(store
        .get_skill_by_id("missing-legacy-skill")
        .unwrap()
        .unwrap()
        .content_hash
        .is_some());
}

#[test]
fn installs_local_skill_and_updates_from_source() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();

    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("SKILL.md"), b"---\nname: x\n---\n").unwrap();
    fs::write(source.path().join("a.txt"), b"v1").unwrap();

    let res = super::install_local_skill(
        app.handle(),
        &store,
        source.path(),
        Some("local1".to_string()),
    )
    .unwrap();
    assert!(res.central_path.exists());
    assert!(res.content_hash.is_some());

    let skill = store.get_skill_by_id(&res.skill_id).unwrap().unwrap();
    assert_eq!(skill.name, "local1");
    assert_eq!(skill.content_hash.as_deref(), res.content_hash.as_deref());

    // add a copy target so update will resync it
    let target_root = tempfile::tempdir().unwrap();
    let target = target_root.path().join("target");
    let t = SkillTargetRecord {
        id: "t1".to_string(),
        skill_id: res.skill_id.clone(),
        tool: "unknown_tool".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: target.to_string_lossy().to_string(),
        mode: "copy".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: None,
    };
    store.upsert_skill_target(&t).unwrap();

    fs::write(source.path().join("a.txt"), b"v2").unwrap();
    let up = super::update_managed_skill_from_source(app.handle(), &store, &res.skill_id).unwrap();
    assert_eq!(up.skill_id, res.skill_id);
    assert!(up.content_hash.is_some());
    assert!(up.updated_targets.contains(&"unknown_tool".to_string()));
    assert!(PathBuf::from(
        store
            .get_skill_by_id(&res.skill_id)
            .unwrap()
            .unwrap()
            .central_path
    )
    .exists());
    assert!(
        target.join("a.txt").exists(),
        "目标路径应存在并包含同步后的文件"
    );
    assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"v2");

    let err = match super::install_local_skill(
        app.handle(),
        &store,
        source.path(),
        Some("local1".to_string()),
    ) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("skill already exists"));
}

#[test]
fn update_restores_a_missing_central_directory_from_local_source() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("SKILL.md"),
        b"---\nname: restored\n---\n",
    )
    .unwrap();
    fs::write(source.path().join("version.txt"), b"v1").unwrap();
    let installed = super::install_local_skill(
        app.handle(),
        &store,
        source.path(),
        Some("restored".to_string()),
    )
    .unwrap();

    fs::remove_dir_all(&installed.central_path).unwrap();
    fs::write(source.path().join("version.txt"), b"v2").unwrap();

    let restored =
        super::update_managed_skill_from_source(app.handle(), &store, &installed.skill_id).unwrap();

    assert_eq!(restored.skill_id, installed.skill_id);
    assert_eq!(
        fs::read(restored.central_path.join("version.txt")).unwrap(),
        b"v2"
    );
    assert!(restored.content_hash.is_some());
}

#[test]
fn reimport_local_selection_replaces_the_existing_record() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let original = tempfile::tempdir().unwrap();
    fs::write(
        original.path().join("SKILL.md"),
        b"---\nname: original\n---\n",
    )
    .unwrap();
    let installed = super::install_local_skill(
        app.handle(),
        &store,
        original.path(),
        Some("stable-name".to_string()),
    )
    .unwrap();

    let replacement_root = tempfile::tempdir().unwrap();
    let replacement = replacement_root.path().join("nested");
    fs::create_dir_all(&replacement).unwrap();
    fs::write(
        replacement.join("SKILL.md"),
        b"---\nname: replacement\n---\n",
    )
    .unwrap();
    fs::write(replacement.join("new.txt"), b"replacement").unwrap();

    let reimported = super::reimport_local_skill_from_selection(
        &store,
        &installed.skill_id,
        replacement_root.path(),
        "nested",
    )
    .unwrap();

    assert_eq!(reimported.skill_id, installed.skill_id);
    assert_eq!(reimported.name, "stable-name");
    assert_eq!(store.list_skills().unwrap().len(), 1);
    assert_eq!(
        fs::read(reimported.central_path.join("new.txt")).unwrap(),
        b"replacement"
    );
    let record = store.get_skill_by_id(&installed.skill_id).unwrap().unwrap();
    assert_eq!(record.source_type, "local");
    assert_eq!(
        record.source_ref.as_deref(),
        Some(replacement.to_string_lossy().as_ref())
    );
}

#[test]
fn reimport_git_selection_replaces_the_existing_record() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let original = tempfile::tempdir().unwrap();
    fs::write(
        original.path().join("SKILL.md"),
        b"---\nname: original\n---\n",
    )
    .unwrap();
    let installed = super::install_local_skill(
        app.handle(),
        &store,
        original.path(),
        Some("stable-name".to_string()),
    )
    .unwrap();

    let repo_dir = tempfile::tempdir().unwrap();
    let selected = repo_dir.path().join("skills/replacement");
    fs::create_dir_all(&selected).unwrap();
    fs::write(selected.join("SKILL.md"), b"---\nname: replacement\n---\n").unwrap();
    fs::write(selected.join("new.txt"), b"git replacement").unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add replacement");
    let repo_url = repo_dir.path().to_string_lossy().to_string();

    let reimported = super::reimport_git_skill_from_selection(
        app.handle(),
        &store,
        &installed.skill_id,
        &repo_url,
        "skills/replacement",
    )
    .unwrap();

    assert_eq!(reimported.skill_id, installed.skill_id);
    assert_eq!(reimported.name, "stable-name");
    assert_eq!(
        fs::read(reimported.central_path.join("new.txt")).unwrap(),
        b"git replacement"
    );
    let record = store.get_skill_by_id(&installed.skill_id).unwrap().unwrap();
    assert_eq!(record.source_type, "git");
    assert_eq!(record.source_ref.as_deref(), Some(repo_url.as_str()));
    assert_eq!(record.source_subpath.as_deref(), Some("skills/replacement"));
    assert!(record.source_revision.is_some());
}

#[test]
fn git_update_persists_an_inferred_multi_skill_subpath() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    let central_path = central_root.path().join("stable-name");
    fs::create_dir_all(&central_path).unwrap();
    fs::write(
        central_path.join("SKILL.md"),
        b"---\nname: stable-name\n---\nold",
    )
    .unwrap();

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/a")).unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/b")).unwrap();
    fs::write(
        repo_dir.path().join("skills/a/SKILL.md"),
        b"---\nname: stable-name\n---\nnew",
    )
    .unwrap();
    fs::write(
        repo_dir.path().join("skills/b/SKILL.md"),
        b"---\nname: other\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add skills");
    let record = SkillRecord {
        id: "legacy-git".to_string(),
        name: "stable-name".to_string(),
        description: None,
        source_type: "git".to_string(),
        source_ref: Some(repo_dir.path().to_string_lossy().to_string()),
        source_subpath: None,
        source_revision: None,
        central_path: central_path.to_string_lossy().to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 1,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&record).unwrap();

    super::update_managed_skill_from_source(app.handle(), &store, &record.id).unwrap();

    let updated = store.get_skill_by_id(&record.id).unwrap().unwrap();
    assert_eq!(updated.source_subpath.as_deref(), Some("skills/a"));
    assert!(fs::read_to_string(central_path.join("SKILL.md"))
        .unwrap()
        .contains("new"));
}

#[test]
fn discovered_import_reuses_same_managed_skill() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let original = tempfile::tempdir().unwrap();
    fs::write(original.path().join("SKILL.md"), b"# Same").unwrap();
    let installed = super::install_local_skill(
        app.handle(),
        &store,
        original.path(),
        Some("same-skill".to_string()),
    )
    .unwrap();

    let discovered = tempfile::tempdir().unwrap();
    fs::write(discovered.path().join("SKILL.md"), b"# Same").unwrap();
    fs::write(discovered.path().join("_skillhub_meta.json"), b"{}").unwrap();
    let reused =
        super::import_discovered_skill(app.handle(), &store, discovered.path(), "same-skill", None)
            .unwrap();

    assert_eq!(reused.skill_id, installed.skill_id);
    assert_eq!(store.list_skills().unwrap().len(), 1);
}

#[test]
fn discovered_import_requires_resolution_and_can_replace_managed_skill() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let original = tempfile::tempdir().unwrap();
    fs::write(original.path().join("SKILL.md"), b"# Hub version").unwrap();
    let installed = super::install_local_skill(
        app.handle(),
        &store,
        original.path(),
        Some("conflicting-skill".to_string()),
    )
    .unwrap();

    let discovered = tempfile::tempdir().unwrap();
    fs::write(discovered.path().join("SKILL.md"), b"# Discovered version").unwrap();
    let error = super::import_discovered_skill(
        app.handle(),
        &store,
        discovered.path(),
        "conflicting-skill",
        None,
    )
    .unwrap_err();
    assert!(error.to_string().starts_with("HUB_NAME_CONFLICT|"));

    let kept = super::import_discovered_skill(
        app.handle(),
        &store,
        discovered.path(),
        "conflicting-skill",
        Some("use_hub"),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(kept.central_path.join("SKILL.md")).unwrap(),
        "# Hub version"
    );

    let target_root = tempfile::tempdir().unwrap();
    let target_path = target_root.path().join("conflicting-skill");
    fs::create_dir_all(&target_path).unwrap();
    fs::write(target_path.join("SKILL.md"), b"# Hub version").unwrap();
    store
        .upsert_skill_target(&SkillTargetRecord {
            id: "copy-target".to_string(),
            skill_id: installed.skill_id.clone(),
            tool: "unknown_tool".to_string(),
            scope: "global".to_string(),
            project_path: None,
            target_path: target_path.to_string_lossy().to_string(),
            mode: "copy".to_string(),
            status: "ok".to_string(),
            last_error: None,
            synced_at: None,
        })
        .unwrap();

    let replaced = super::import_discovered_skill(
        app.handle(),
        &store,
        discovered.path(),
        "conflicting-skill",
        Some("replace_hub"),
    )
    .unwrap();
    assert_eq!(replaced.skill_id, installed.skill_id);
    assert_eq!(
        fs::read_to_string(replaced.central_path.join("SKILL.md")).unwrap(),
        "# Discovered version"
    );
    assert_eq!(
        fs::read_to_string(target_path.join("SKILL.md")).unwrap(),
        "# Discovered version"
    );
    let record = store.get_skill_by_id(&installed.skill_id).unwrap().unwrap();
    assert_eq!(
        record.source_ref.as_deref(),
        Some(discovered.path().to_string_lossy().as_ref())
    );
}

#[test]
fn lists_and_installs_git_skills_without_network() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::write(repo_dir.path().join("SKILL.md"), "---\nname: Root\n---\n").unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/a")).unwrap();
    fs::write(
        repo_dir.path().join("skills/a/SKILL.md"),
        "---\nname: A\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add skills");

    let candidates = super::list_git_skills(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
    )
    .unwrap();
    let subpaths: Vec<String> = candidates.into_iter().map(|c| c.subpath).collect();
    assert!(subpaths.contains(&".".to_string()));
    assert!(subpaths.iter().any(|s| s.ends_with("skills/a")));

    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "skills/a",
        None,
    )
    .unwrap();
    assert!(res.central_path.exists());
}

#[test]
fn install_git_skill_errors_on_multi_skills_repo_root() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/a")).unwrap();
    fs::create_dir_all(repo_dir.path().join("skills/b")).unwrap();
    fs::write(
        repo_dir.path().join("skills/a/SKILL.md"),
        "---\nname: A\n---\n",
    )
    .unwrap();
    fs::write(
        repo_dir.path().join("skills/b/SKILL.md"),
        "---\nname: B\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "multi skills");

    let err = match super::install_git_skill(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        None,
        None,
    ) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("MULTI_SKILLS|"));
}

#[test]
fn lists_local_skills_with_invalid_entries() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fs::create_dir_all(base.join("skills/a")).unwrap();
    fs::create_dir_all(base.join("skills/b")).unwrap();
    fs::create_dir_all(base.join("skills/c")).unwrap();
    fs::create_dir_all(base.join("skills/d")).unwrap();

    fs::write(base.join("skills/a/SKILL.md"), "---\nname: A\n---\n").unwrap();
    fs::write(base.join("skills/c/SKILL.md"), "name: C\n").unwrap();
    fs::write(base.join("skills/d/SKILL.md"), "---\ndescription: D\n---\n").unwrap();

    let list = super::list_local_skills(base).unwrap();

    let find = |subpath: &str| list.iter().find(|c| c.subpath == subpath).cloned();

    let a = find("skills/a").expect("skills/a");
    assert!(a.valid);
    assert_eq!(a.name, "A");

    let b = find("skills/b").expect("skills/b");
    assert!(!b.valid);
    assert_eq!(b.reason.as_deref(), Some("missing_skill_md"));

    let c = find("skills/c").expect("skills/c");
    assert!(!c.valid);
    assert_eq!(c.reason.as_deref(), Some("invalid_frontmatter"));

    let d = find("skills/d").expect("skills/d");
    assert!(!d.valid);
    assert_eq!(d.reason.as_deref(), Some("missing_name"));
}

#[test]
fn install_local_selection_validates_skill_md() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();

    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let base = tempfile::tempdir().unwrap();
    fs::create_dir_all(base.path().join("skills/a")).unwrap();
    fs::create_dir_all(base.path().join("skills/b")).unwrap();
    fs::write(
        base.path().join("skills/a/SKILL.md"),
        "---\nname: Local A\n---\n",
    )
    .unwrap();

    let res = super::install_local_skill_from_selection(
        app.handle(),
        &store,
        base.path(),
        "skills/a",
        None,
    )
    .unwrap();
    assert!(res.central_path.exists());
    let skill = store.get_skill_by_id(&res.skill_id).unwrap().unwrap();
    assert_eq!(skill.name, "Local A");

    let err = match super::install_local_skill_from_selection(
        app.handle(),
        &store,
        base.path(),
        "skills/b",
        None,
    ) {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("SKILL_INVALID|missing_skill_md"));
}

/// Issue #28: when a git subpath is "skills", the derived name should be replaced by the
/// SKILL.md name to avoid path duplication (e.g. `~/.claude/skills/skills/`).
#[test]
fn install_git_skill_uses_skill_md_name_over_subpath_skills() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    // Build a repo with skills/<folder> where the folder is named "skills" (simulating
    // a URL like https://github.com/owner/repo/tree/main/skills).
    let repo_dir = tempfile::tempdir().unwrap();
    let skills_dir = repo_dir.path().join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: my-real-skill\ndescription: A real skill\n---\n",
    )
    .unwrap();
    fs::write(skills_dir.join("helper.txt"), b"data").unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add skill in skills dir");

    // install_git_skill_from_selection with subpath "skills" (no user-provided name)
    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "skills",
        None,
    )
    .unwrap();

    // The name should be "my-real-skill" from SKILL.md, NOT "skills" from the subpath.
    assert_eq!(res.name, "my-real-skill");
    assert!(res.central_path.ends_with("my-real-skill"));
    assert!(res.central_path.join("SKILL.md").exists());

    let skill = store.get_skill_by_id(&res.skill_id).unwrap().unwrap();
    assert_eq!(skill.name, "my-real-skill");
    assert_eq!(skill.description.as_deref(), Some("A real skill"));
}

#[test]
fn install_git_skill_rejects_container_subpath_without_skill_md() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer"),
    )
    .unwrap();
    fs::write(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add container skill");

    let err = match super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "awesome_agent_skills",
        None,
    ) {
        Ok(_) => panic!("expected invalid skill path"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("SKILL_INVALID|missing_skill_md"));
}

#[test]
fn install_git_skill_selection_accepts_specific_child_under_container() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer"),
    )
    .unwrap();
    fs::write(
        repo_dir
            .path()
            .join("awesome_agent_skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\ndescription: docs\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add container skill");

    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "awesome_agent_skills/technical-writer",
        None,
    )
    .unwrap();

    assert_eq!(res.name, "technical-writer");
    assert!(res.central_path.join("SKILL.md").exists());
}

/// Issue #28: when user explicitly provides a name, SKILL.md should NOT override it.
#[test]
fn install_git_skill_respects_user_provided_name() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    let skills_dir = repo_dir.path().join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(skills_dir.join("SKILL.md"), "---\nname: md-name\n---\n").unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add skill");

    let res = super::install_git_skill_from_selection(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        "skills",
        Some("user-custom-name".to_string()),
    )
    .unwrap();

    // User-provided name takes priority.
    assert_eq!(res.name, "user-custom-name");
}

/// Issue #28: install_git_skill (non-selection variant) also uses SKILL.md name.
#[test]
fn install_git_skill_derives_name_from_skill_md() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::write(
        repo_dir.path().join("SKILL.md"),
        "---\nname: proper-name\ndescription: desc\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "init");

    // The repo name (derived from path) will be something like a temp dir name.
    // After install, the name should be "proper-name" from SKILL.md.
    let res = super::install_git_skill(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(res.name, "proper-name");
    assert!(res.central_path.ends_with("proper-name"));
}

/// Issue #18: repos with skills in root-level subdirectories (no `skills/` parent)
/// should be detected as multi-skill repos.
#[test]
fn install_git_skill_detects_root_level_multi_skills() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    // Build a repo with skills directly in root subdirectories (no skills/ parent)
    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("skill-a")).unwrap();
    fs::create_dir_all(repo_dir.path().join("skill-b")).unwrap();
    fs::write(
        repo_dir.path().join("skill-a/SKILL.md"),
        "---\nname: Skill A\n---\n",
    )
    .unwrap();
    fs::write(
        repo_dir.path().join("skill-b/SKILL.md"),
        "---\nname: Skill B\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add root-level skills");

    // install_git_skill should detect multiple skills and bail with MULTI_SKILLS
    let err = match super::install_git_skill(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
        None,
        None,
    ) {
        Ok(_) => panic!("expected MULTI_SKILLS error"),
        Err(e) => e,
    };
    assert!(format!("{:#}", err).contains("MULTI_SKILLS|"));
}

/// Issue #18: list_git_skills should discover skills in root-level subdirectories.
#[test]
fn list_git_skills_finds_root_level_skills() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("my-skill-1")).unwrap();
    fs::create_dir_all(repo_dir.path().join("my-skill-2")).unwrap();
    fs::create_dir_all(repo_dir.path().join("not-a-skill")).unwrap();
    fs::write(
        repo_dir.path().join("my-skill-1/SKILL.md"),
        "---\nname: First\n---\n",
    )
    .unwrap();
    fs::write(
        repo_dir.path().join("my-skill-2/SKILL.md"),
        "---\nname: Second\n---\n",
    )
    .unwrap();
    // not-a-skill has no SKILL.md — should NOT be discovered
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add root-level skills");

    let candidates = super::list_git_skills(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
    )
    .unwrap();

    let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
    assert!(names.contains(&"First".to_string()), "should find First");
    assert!(names.contains(&"Second".to_string()), "should find Second");
    // "not-a-skill" should NOT appear
    assert!(
        !candidates.iter().any(|c| c.subpath.contains("not-a-skill")),
        "should not find not-a-skill"
    );
}

#[test]
fn list_git_skills_finds_root_skill_container_layout() {
    let app = tauri::test::mock_app();
    let (_dir, store) = make_store();
    let central_root = tempfile::tempdir().unwrap();
    set_central_path(&store, central_root.path());

    let repo_dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo_dir.path().join("custom-agent-skills/technical-writer")).unwrap();
    fs::write(
        repo_dir
            .path()
            .join("custom-agent-skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\ndescription: docs\n---\n",
    )
    .unwrap();
    let repo = init_git_repo(repo_dir.path());
    commit_all(&repo, "add container skill");

    let candidates = super::list_git_skills(
        app.handle(),
        &store,
        repo_dir.path().to_string_lossy().as_ref(),
    )
    .unwrap();

    let candidate = candidates
        .iter()
        .find(|c| c.name == "technical-writer")
        .expect("technical-writer should be discovered");
    assert_eq!(candidate.subpath, "custom-agent-skills/technical-writer");
    assert_eq!(candidate.description.as_deref(), Some("docs"));
}

#[test]
fn collect_skill_dirs_finds_skills_under_explicit_container() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("technical-writer")).unwrap();
    fs::create_dir_all(dir.path().join("not-a-skill")).unwrap();
    fs::write(
        dir.path().join("technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    let rels: Vec<String> = dirs
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(rels, vec!["technical-writer".to_string()]);
}

#[test]
fn collect_skill_dirs_finds_multiple_skills_under_explicit_container() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("technical-writer")).unwrap();
    fs::create_dir_all(dir.path().join("python-expert")).unwrap();
    fs::create_dir_all(dir.path().join("not-a-skill")).unwrap();
    fs::write(
        dir.path().join("technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("python-expert/SKILL.md"),
        "---\nname: python-expert\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    let rels: Vec<String> = dirs
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(
        rels,
        vec!["python-expert".to_string(), "technical-writer".to_string()]
    );
}

#[test]
fn collect_skill_dirs_scans_named_skill_containers_but_not_generic_dirs() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("agent-pack/hidden-skill")).unwrap();
    fs::create_dir_all(dir.path().join("agent-skills/visible-skill")).unwrap();
    fs::write(
        dir.path().join("agent-pack/hidden-skill/SKILL.md"),
        "---\nname: hidden\n---\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("agent-skills/visible-skill/SKILL.md"),
        "---\nname: visible\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    let rels: Vec<String> = dirs
        .iter()
        .map(|p| {
            p.strip_prefix(dir.path())
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(rels, vec!["agent-skills/visible-skill".to_string()]);
}

#[test]
fn collect_skill_dirs_deduplicates_known_root_containers() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("skills/technical-writer")).unwrap();
    fs::write(
        dir.path().join("skills/technical-writer/SKILL.md"),
        "---\nname: technical-writer\n---\n",
    )
    .unwrap();

    let dirs = super::collect_skill_dirs(dir.path());
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("skills/technical-writer"));
}

#[test]
fn clone_retry_does_not_duplicate_the_same_error() {
    let message = "system git is required for sparse checkout";
    let combined =
        super::combine_clone_retry_error(anyhow::anyhow!(message), anyhow::anyhow!(message));
    let text = format!("{:#}", combined);
    assert_eq!(text, message);
    assert_eq!(text.matches(message).count(), 1);
}

#[test]
fn clone_retry_keeps_a_distinct_first_cause_once() {
    let combined = super::combine_clone_retry_error(
        anyhow::anyhow!("git clone failed: corrupt index"),
        anyhow::anyhow!("git fetch failed: connection reset"),
    );
    let text = format!("{:#}", combined);
    assert!(text.contains("首次拉取失败：git clone failed: corrupt index"));
    assert!(text.contains("git fetch failed: connection reset"));
    assert_eq!(text.matches("corrupt index").count(), 1);
    assert_eq!(text.matches("connection reset").count(), 1);
}

#[test]
fn deterministic_git_failure_is_not_retried() {
    let missing = anyhow::anyhow!("子目录 Skill 需要系统 git 才能做 sparse checkout");
    assert!(!super::git_cache_retry_could_help(&missing));
    let legacy = anyhow::anyhow!("system git is required for sparse checkout");
    assert!(!super::git_cache_retry_could_help(&legacy));
    let cancelled = anyhow::anyhow!("CANCELLED|操作已被用户取消。");
    assert!(!super::git_cache_retry_could_help(&cancelled));
    let transient = anyhow::anyhow!("git fetch failed: connection reset");
    assert!(super::git_cache_retry_could_help(&transient));
}
