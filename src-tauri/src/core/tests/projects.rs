use std::path::Path;

use crate::core::projects::{
    create_or_get_project, diagnose_project, inject_project_skill, rebuild_project_git_exclude,
    remove_project_skill, repair_project_issue, scan_project, update_project_configuration,
    ProjectToolSpec,
};
use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SkillStore::new(dir.path().join("test.db"));
    store.ensure_schema().expect("ensure schema");
    (dir, store)
}

fn write_skill(path: &Path, body: &str) {
    std::fs::create_dir_all(path).expect("create skill dir");
    std::fs::write(path.join("SKILL.md"), body).expect("write SKILL.md");
}

fn make_skill(id: &str, name: &str, central_path: &Path, content_hash: &str) -> SkillRecord {
    SkillRecord {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: None,
        source_subpath: None,
        source_revision: None,
        central_path: central_path.to_string_lossy().to_string(),
        content_hash: Some(content_hash.to_string()),
        created_at: 1,
        updated_at: 1,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    }
}

#[test]
fn project_import_reuses_the_normalized_root_and_persists_settings() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");

    let first = create_or_get_project(
        &store,
        project_dir.path(),
        &["codex".to_string(), "cursor".to_string()],
    )
    .expect("create project");
    let second = create_or_get_project(&store, project_dir.path(), &["claude_code".to_string()])
        .expect("reuse project");

    assert_eq!(first.id, second.id);
    assert_eq!(second.enabled_tool_keys, vec!["codex", "cursor"]);
    assert_eq!(store.list_projects().expect("list projects").len(), 1);
}

#[test]
fn scan_deduplicates_shared_tool_directories_and_matches_library_content() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let skill_path = project_dir.path().join(".agents/skills/review");
    write_skill(&skill_path, "# Review\nCheck the change.\n");
    let hash = crate::core::content_hash::hash_dir(&skill_path).expect("hash skill");
    store
        .upsert_skill(&make_skill("library-review", "review", &skill_path, &hash))
        .expect("insert library skill");
    let project = create_or_get_project(
        &store,
        project_dir.path(),
        &["codex".to_string(), "cursor".to_string()],
    )
    .expect("create project");

    let scanned = scan_project(
        &store,
        &project,
        &[
            ProjectToolSpec::new("codex", ".agents/skills"),
            ProjectToolSpec::new("cursor", ".agents/skills"),
        ],
    )
    .expect("scan project");

    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].name, "review");
    assert_eq!(scanned[0].tool_keys, vec!["codex", "cursor"]);
    assert_eq!(
        scanned[0].matched_skill_id.as_deref(),
        Some("library-review")
    );
}

#[test]
fn diagnostics_report_same_name_with_different_content_across_tool_directories() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    write_skill(
        &project_dir.path().join(".agents/skills/review"),
        "agents version",
    );
    write_skill(
        &project_dir.path().join(".claude/skills/review"),
        "claude version",
    );
    let project = create_or_get_project(
        &store,
        project_dir.path(),
        &["codex".to_string(), "claude_code".to_string()],
    )
    .expect("create project");
    let tools = [
        ProjectToolSpec::new("codex", ".agents/skills"),
        ProjectToolSpec::new("claude_code", ".claude/skills"),
    ];
    scan_project(&store, &project, &tools).expect("scan project");

    let issues = diagnose_project(&store, &project, &tools).expect("diagnose project");

    assert!(issues
        .iter()
        .any(|issue| issue.kind == "same_project_name_content_mismatch"));
}

#[test]
fn git_exclude_rebuild_preserves_user_rules_and_only_adds_injected_skill_paths() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    git2::Repository::init(project_dir.path()).expect("init git repository");
    let skill_path = project_dir.path().join(".agents/skills/review");
    write_skill(&skill_path, "review");
    let central = tempfile::tempdir().expect("central tempdir");
    let hash = crate::core::content_hash::hash_dir(&skill_path).expect("hash skill");
    store
        .upsert_skill(&make_skill(
            "library-review",
            "review",
            central.path(),
            &hash,
        ))
        .expect("insert skill");
    let mut project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    project.local_git_exclude_enabled = true;
    store.update_project(&project).expect("update project");
    store
        .upsert_skill_target(&SkillTargetRecord {
            id: "target-review".to_string(),
            skill_id: "library-review".to_string(),
            tool: "codex".to_string(),
            scope: "project".to_string(),
            project_path: Some(project.root_path.clone()),
            target_path: skill_path.to_string_lossy().to_string(),
            mode: "copy".to_string(),
            status: "ok".to_string(),
            last_error: None,
            synced_at: Some(1),
        })
        .expect("insert target");
    let exclude_path = project_dir.path().join(".git/info/exclude");
    std::fs::write(&exclude_path, "*.local\n").expect("write existing exclude");

    rebuild_project_git_exclude(&store, &project).expect("rebuild exclude");
    let exclude = std::fs::read_to_string(exclude_path).expect("read exclude");

    assert!(exclude.contains("*.local"));
    assert!(exclude.contains("# Skills Hub: begin"));
    assert!(exclude.contains("/.agents/skills/review/"));
    assert!(!exclude.contains("/.agents/skills/\n"));
}

#[test]
fn injection_blocks_same_name_different_content_without_touching_project_files() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project_skill = project_dir.path().join(".agents/skills/review");
    write_skill(&project_skill, "project version");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");

    let result = inject_project_skill(
        &store,
        &project,
        &skill,
        &[ProjectToolSpec::new("codex", ".agents/skills")],
    )
    .expect("record blocked injection");

    assert_eq!(result.failed, 1);
    assert_eq!(
        std::fs::read_to_string(project_skill.join("SKILL.md")).expect("read project skill"),
        "project version"
    );
    assert_eq!(
        store
            .get_project_skill_link(&project.id, &skill.id)
            .expect("get link")
            .expect("link exists")
            .status,
        "conflict"
    );
    assert!(store
        .get_skill_target(&skill.id, "codex", "project", Some(&project.root_path))
        .expect("get target")
        .is_none());
}

#[test]
fn changing_project_tools_updates_desired_state_and_creates_repairable_issue() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [
        ProjectToolSpec::new("codex", ".agents/skills"),
        ProjectToolSpec::new("claude_code", ".claude/skills"),
    ];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject initial skill");

    let updated = update_project_configuration(
        &store,
        &project.id,
        &["codex".to_string(), "claude_code".to_string()],
        false,
    )
    .expect("update project tools");
    let issues = diagnose_project(&store, &updated, &tools).expect("diagnose project");

    assert!(!project_dir.path().join(".claude/skills/review").exists());
    assert!(issues.iter().any(|issue| {
        issue.kind == "tool_added_pending_sync" && issue.tool_key.as_deref() == Some("claude_code")
    }));
}

#[test]
fn adding_matching_native_skill_records_desired_state_without_creating_a_hub_target() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "same content");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project_skill = project_dir.path().join(".agents/skills/review");
    write_skill(&project_skill, "same content");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    scan_project(&store, &project, &tools).expect("scan native skill");

    let result =
        inject_project_skill(&store, &project, &skill, &tools).expect("add matching skill");
    scan_project(&store, &project, &tools).expect("refresh scan");
    let issues = diagnose_project(&store, &project, &tools).expect("diagnose project");

    assert_eq!(result.matched, 1);
    assert!(issues
        .iter()
        .all(|issue| issue.kind != "tool_added_pending_sync"));
    assert!(store
        .get_skill_target(&skill.id, "codex", "project", Some(&project.root_path))
        .expect("get target")
        .is_none());
    assert_eq!(
        std::fs::read_to_string(project_skill.join("SKILL.md")).expect("read native skill"),
        "same content"
    );
}

#[test]
fn repairing_a_new_tool_that_shares_a_directory_reuses_the_existing_hub_target() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [
        ProjectToolSpec::new("codex", ".agents/skills"),
        ProjectToolSpec::new("cursor", ".agents/skills"),
    ];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject for codex");
    let updated = update_project_configuration(
        &store,
        &project.id,
        &["codex".to_string(), "cursor".to_string()],
        false,
    )
    .expect("enable cursor");
    scan_project(&store, &updated, &tools).expect("scan shared directory");
    let issue = diagnose_project(&store, &updated, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| {
            issue.kind == "tool_added_pending_sync" && issue.tool_key.as_deref() == Some("cursor")
        })
        .expect("cursor pending issue");

    repair_project_issue(&store, &updated, &tools, &issue.id).expect("repair cursor target");

    assert!(store
        .get_skill_target(&skill.id, "cursor", "project", Some(&project.root_path))
        .expect("get cursor target")
        .is_some());
}

#[test]
fn project_tool_directory_cannot_escape_the_project_root() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let project = create_or_get_project(&store, project_dir.path(), &["custom".to_string()])
        .expect("create project");

    let error = scan_project(
        &store,
        &project,
        &[ProjectToolSpec::new("custom", "../outside/skills")],
    )
    .expect_err("escaping path must be rejected");

    assert!(error.to_string().contains("escapes project root"));
}

#[cfg(unix)]
#[test]
fn project_tool_directory_cannot_escape_through_a_symlinked_parent() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let outside_dir = tempfile::tempdir().expect("outside tempdir");
    write_skill(&outside_dir.path().join("skills/review"), "outside skill");
    std::os::unix::fs::symlink(outside_dir.path(), project_dir.path().join(".agents"))
        .expect("create escaping symlink");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");

    let error = scan_project(
        &store,
        &project,
        &[ProjectToolSpec::new("codex", ".agents/skills")],
    )
    .expect_err("symlink escape must be rejected");

    assert!(error.to_string().contains("escapes project root"));
}

#[test]
fn failed_injection_still_records_the_project_desired_state() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "disabled library skill");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let mut skill = make_skill("library-review", "review", central_dir.path(), &hash);
    skill.enabled = false;
    store.upsert_skill(&skill).expect("insert disabled skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");

    inject_project_skill(
        &store,
        &project,
        &skill,
        &[ProjectToolSpec::new("codex", ".agents/skills")],
    )
    .expect_err("disabled skill injection must fail");

    let link = store
        .get_project_skill_link(&project.id, &skill.id)
        .expect("get project link")
        .expect("desired link is retained");
    assert!(link.enabled);
    assert_eq!(link.desired_tools, vec!["codex"]);
    assert_eq!(link.status, "error");
}

#[test]
fn removing_a_hub_link_preserves_project_content_that_the_user_replaced() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project = create_or_get_project(&store, project_dir.path(), &["cursor".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("cursor", ".agents/skills")];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject skill");
    let target = project_dir.path().join(".agents/skills/review");
    std::fs::remove_dir_all(&target).expect("remove Hub copy");
    write_skill(&target, "user replacement");

    remove_project_skill(&store, &project, &skill.id).expect("remove project link");

    assert_eq!(
        std::fs::read_to_string(target.join("SKILL.md")).expect("read replacement"),
        "user replacement"
    );
}

#[test]
fn successful_unsync_repair_restores_the_link_status() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project = create_or_get_project(
        &store,
        project_dir.path(),
        &["codex".to_string(), "claude_code".to_string()],
    )
    .expect("create project");
    let tools = [
        ProjectToolSpec::new("codex", ".agents/skills"),
        ProjectToolSpec::new("claude_code", ".claude/skills"),
    ];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject skill");
    let updated = update_project_configuration(&store, &project.id, &["codex".to_string()], false)
        .expect("disable claude tool");
    let issue = diagnose_project(&store, &updated, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "tool_removed_pending_unsync")
        .expect("pending unsync issue");

    repair_project_issue(&store, &updated, &tools, &issue.id).expect("repair unsync");

    assert_eq!(
        store
            .get_project_skill_link(&project.id, &skill.id)
            .expect("get link")
            .expect("link exists")
            .status,
        "ok"
    );
}

#[test]
fn malformed_git_exclude_marker_does_not_remove_following_user_rules() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    git2::Repository::init(project_dir.path()).expect("init git repository");
    let project = create_or_get_project(&store, project_dir.path(), &[]).expect("create project");
    let exclude_path = project_dir.path().join(".git/info/exclude");
    std::fs::write(
        &exclude_path,
        "# Skills Hub: begin\n*.private\nuser-generated/\n",
    )
    .expect("write malformed block");

    rebuild_project_git_exclude(&store, &project).expect("repair exclude");
    let exclude = std::fs::read_to_string(exclude_path).expect("read exclude");

    assert!(exclude.contains("*.private"));
    assert!(exclude.contains("user-generated/"));
    assert!(!exclude.contains("# Skills Hub: begin"));
}
