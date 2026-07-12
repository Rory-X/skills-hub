use std::path::Path;

use crate::core::projects::{
    assign_project_skill_tools_and_scan, can_ignore_project_issue, collect_project_skill,
    create_or_get_project, diagnose_project, inject_project_skill, project_issue_fingerprint,
    project_issue_projections, rebuild_project_git_exclude, remove_project_skill,
    repair_project_issue, repair_project_issue_and_scan, scan_project,
    update_project_configuration, visible_project_issue_projections, ProjectIssue, ProjectToolSpec,
};
use crate::core::skill_store::{
    IgnoredIssueRecord, ProjectSkillLinkRecord, SkillAssignmentRecord, SkillRecord, SkillStore,
    SkillTargetRecord,
};

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
    let assignments = store
        .list_skill_assignments("library-review")
        .expect("list assignments");
    assert_eq!(assignments.len(), 2);
    assert!(assignments
        .iter()
        .all(|assignment| assignment.ownership == "native"));
}

#[test]
fn collecting_matching_project_copies_inherits_all_agent_tools() {
    let app = tauri::test::mock_app();
    let (_db_dir, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    store
        .set_setting(
            "central_repo_path",
            central.path().to_string_lossy().as_ref(),
        )
        .unwrap();
    let project_dir = tempfile::tempdir().expect("project");
    write_skill(
        &project_dir.path().join(".codex/skills/review"),
        "same project skill",
    );
    write_skill(
        &project_dir.path().join(".cursor/skills/review"),
        "same project skill",
    );
    let project = create_or_get_project(
        &store,
        project_dir.path(),
        &["codex".to_string(), "cursor".to_string()],
    )
    .unwrap();
    let discovered = scan_project(
        &store,
        &project,
        &[
            ProjectToolSpec::new("codex", ".codex/skills"),
            ProjectToolSpec::new("cursor", ".cursor/skills"),
        ],
    )
    .unwrap();
    assert_eq!(discovered.len(), 2);

    collect_project_skill(app.handle(), &store, &project.id, &discovered[0].id, None).unwrap();

    let skill = store.list_skills().unwrap().remove(0);
    let link = store
        .get_project_skill_link(&project.id, &skill.id)
        .unwrap()
        .unwrap();
    assert_eq!(link.desired_tools, vec!["codex", "cursor"]);
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert_eq!(assignments.len(), 2);
    assert!(assignments
        .iter()
        .all(|assignment| assignment.ownership == "native"));
}

#[test]
fn scan_backfills_missing_library_hash_and_avoids_false_conflict() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "same content");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let mut skill = make_skill("library-review", "review", central_dir.path(), &hash);
    skill.content_hash = None;
    store.upsert_skill(&skill).expect("insert library skill");
    write_skill(
        &project_dir.path().join(".agents/skills/review"),
        "same content",
    );
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];

    let scanned = scan_project(&store, &project, &tools).expect("scan project");
    let issues = diagnose_project(&store, &project, &tools).expect("diagnose project");

    assert_eq!(
        store
            .get_skill_by_id("library-review")
            .expect("get library skill")
            .expect("library skill exists")
            .content_hash
            .as_deref(),
        Some(hash.as_str())
    );
    assert_eq!(
        scanned[0].matched_skill_id.as_deref(),
        Some("library-review")
    );
    assert!(issues
        .iter()
        .all(|issue| issue.kind != "same_name_content_conflict"));
}

#[test]
fn project_version_does_not_create_a_library_conflict_until_the_library_skill_is_added() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library content");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    store
        .upsert_skill(&make_skill(
            "library-review",
            "review",
            central_dir.path(),
            &hash,
        ))
        .expect("insert library skill");
    write_skill(
        &project_dir.path().join(".agents/skills/review"),
        "project content",
    );
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];

    scan_project(&store, &project, &tools).expect("scan project");

    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .iter()
        .all(|issue| issue.kind != "same_name_content_conflict"));
}

#[test]
fn diagnostics_use_live_project_content_after_a_library_update() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "old library content");
    let old_hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let mut skill = make_skill("library-review", "review", central_dir.path(), &old_hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject skill");
    scan_project(&store, &project, &tools).expect("scan project");

    std::fs::write(central_dir.path().join("SKILL.md"), "new library content")
        .expect("update central skill");
    let target = project_dir.path().join(".agents/skills/review/SKILL.md");
    if !std::fs::read_to_string(&target).is_ok_and(|content| content == "new library content") {
        std::fs::write(&target, "new library content").expect("update copied target");
    }
    skill.content_hash =
        Some(crate::core::content_hash::hash_dir(central_dir.path()).expect("hash updated skill"));
    store.upsert_skill(&skill).expect("update library hash");

    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose without rescan")
        .iter()
        .all(|issue| issue.kind != "same_name_content_conflict"));
}

#[test]
fn injection_skips_unavailable_legacy_tools_without_deleting_their_selection() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library content");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project = create_or_get_project(
        &store,
        project_dir.path(),
        &["codex".to_string(), "cursor".to_string()],
    )
    .expect("create legacy project");
    store
        .upsert_project_skill_link(&ProjectSkillLinkRecord {
            project_id: project.id.clone(),
            skill_id: skill.id.clone(),
            enabled: true,
            desired_tools: vec!["codex".to_string(), "cursor".to_string()],
            status: "pending".to_string(),
            created_at: 1,
            updated_at: 1,
        })
        .expect("insert legacy link");
    let tools = [
        ProjectToolSpec::new("codex", ".agents/skills"),
        ProjectToolSpec::new("cursor", ".cursor/skills").with_available(false),
    ];

    inject_project_skill(&store, &project, &skill, &tools).expect("inject available targets");

    assert!(project_dir.path().join(".agents/skills/review").exists());
    assert!(!project_dir.path().join(".cursor/skills/review").exists());
    assert_eq!(
        store
            .get_project_skill_link(&project.id, &skill.id)
            .expect("get link")
            .expect("link exists")
            .desired_tools,
        vec!["codex", "cursor"]
    );
}

#[test]
fn library_conflict_can_keep_the_project_version_and_cancel_the_library_link() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library content");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    write_skill(
        &project_dir.path().join(".agents/skills/review"),
        "project content",
    );
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];

    scan_project(&store, &project, &tools).expect("scan project");
    inject_project_skill(&store, &project, &skill, &tools).expect("record blocked injection");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "same_name_content_conflict")
        .expect("library conflict");

    assert_eq!(issue.actions.len(), 2);
    assert!(issue
        .actions
        .iter()
        .any(|action| action.kind == "replace_with_library"));
    assert!(issue
        .actions
        .iter()
        .any(|action| action.kind == "keep_project_version"));

    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "keep_project_version",
        None,
    )
    .expect("keep project version");
    scan_project(&store, &project, &tools).expect("rescan project");

    assert_eq!(
        std::fs::read_to_string(project_dir.path().join(".agents/skills/review/SKILL.md"))
            .expect("read project skill"),
        "project content"
    );
    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose repaired project")
        .iter()
        .all(|issue| issue.kind != "same_name_content_conflict"));
}

#[test]
fn library_conflict_can_replace_the_project_version() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library content");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
    let project_skill = project_dir.path().join(".agents/skills/review");
    write_skill(&project_skill, "project content");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    scan_project(&store, &project, &tools).expect("scan project");
    inject_project_skill(&store, &project, &skill, &tools).expect("record blocked injection");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "same_name_content_conflict")
        .expect("library conflict");

    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "replace_with_library",
        None,
    )
    .expect("replace project version");
    scan_project(&store, &project, &tools).expect("rescan project");

    assert_eq!(
        std::fs::read_to_string(project_skill.join("SKILL.md")).expect("read replaced skill"),
        "library content"
    );
    assert!(store
        .get_skill_target(&skill.id, "codex", "project", Some(&project.root_path),)
        .expect("get project target")
        .is_some());
    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose repaired project")
        .iter()
        .all(|issue| issue.kind != "same_name_content_conflict"));
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

    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "same_project_name_content_mismatch")
        .expect("variant mismatch");
    let selected = issue
        .actions
        .iter()
        .find(|action| {
            action.kind == "keep_project_variant"
                && action
                    .label
                    .as_deref()
                    .is_some_and(|label| label.contains(".agents"))
        })
        .expect("agents variant action")
        .value
        .clone()
        .expect("variant id");

    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "keep_project_variant",
        Some(&selected),
    )
    .expect("keep agents variant");
    scan_project(&store, &project, &tools).expect("rescan project");

    assert_eq!(
        std::fs::read_to_string(project_dir.path().join(".claude/skills/review/SKILL.md"))
            .expect("read replaced variant"),
        "agents version"
    );
    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose repaired project")
        .iter()
        .all(|issue| issue.kind != "same_project_name_content_mismatch"));
}

#[test]
fn resolving_project_variants_reconciles_the_link_status() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "agents version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert library skill");
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
    inject_project_skill(&store, &project, &skill, &tools).expect("record partial injection");
    scan_project(&store, &project, &tools).expect("rescan project");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "same_project_name_content_mismatch")
        .expect("variant mismatch");
    let selected = issue
        .actions
        .iter()
        .find(|action| {
            action
                .label
                .as_deref()
                .is_some_and(|label| label.contains(".agents"))
        })
        .and_then(|action| action.value.clone())
        .expect("agents variant id");

    repair_project_issue_and_scan(
        &store,
        &project.id,
        &issue.id,
        "keep_project_variant",
        Some(&selected),
        &tools,
    )
    .expect("resolve project variants");

    assert_eq!(
        store
            .get_project_skill_link(&project.id, &skill.id)
            .expect("get project link")
            .expect("project link exists")
            .status,
        "ok"
    );
}

#[test]
fn invalid_skill_issue_exposes_delete_action_and_removes_only_that_directory() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let invalid_path = project_dir.path().join(".agents/skills/broken");
    std::fs::create_dir_all(&invalid_path).expect("create invalid skill");
    std::fs::write(invalid_path.join("notes.txt"), "not a skill").expect("write invalid skill");
    let sibling_path = project_dir.path().join(".agents/skills/healthy");
    write_skill(&sibling_path, "healthy");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    scan_project(&store, &project, &tools).expect("scan project");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "invalid_skill")
        .expect("invalid issue");

    assert_eq!(issue.actions.len(), 1);
    assert_eq!(issue.actions[0].kind, "delete_invalid_skill");
    assert!(issue.actions[0].confirmation_required);

    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "delete_invalid_skill",
        None,
    )
    .expect("delete invalid skill");
    scan_project(&store, &project, &tools).expect("rescan project");

    assert!(!invalid_path.exists());
    assert!(sibling_path.exists());
    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose repaired project")
        .iter()
        .all(|issue| issue.kind != "invalid_skill"));
}

#[test]
fn invalid_skill_delete_refuses_content_repaired_after_the_scan() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let invalid_path = project_dir.path().join(".agents/skills/repaired");
    std::fs::create_dir_all(&invalid_path).expect("create invalid skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    scan_project(&store, &project, &tools).expect("scan project");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "invalid_skill")
        .expect("invalid issue");
    std::fs::write(invalid_path.join("SKILL.md"), "repaired content")
        .expect("repair invalid skill");

    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "delete_invalid_skill",
        None,
    )
    .expect_err("stale delete action must be rejected");

    assert!(invalid_path.join("SKILL.md").is_file());
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
    let exclude = std::fs::read_to_string(&exclude_path).expect("read exclude");

    assert!(exclude.contains("*.local"));
    assert!(exclude.contains("# Skills Hub: begin"));
    assert!(exclude.contains("/.agents/skills/review/"));
    assert!(!exclude.contains("/.agents/skills/\n"));

    project.local_git_exclude_enabled = false;
    store
        .update_project(&project)
        .expect("disable local exclude");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    scan_project(&store, &project, &tools).expect("scan project");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "git_exclude_drift")
        .expect("git exclude drift");
    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "repair_git_exclude",
        None,
    )
    .expect("remove Hub exclude block");
    let repaired = std::fs::read_to_string(&exclude_path).expect("read repaired exclude");

    assert!(repaired.contains("*.local"));
    assert!(!repaired.contains("# Skills Hub: begin"));
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
fn changing_project_tools_does_not_expand_a_skill_override() {
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
    assert!(issues.iter().all(|issue| {
        issue.kind != "tool_added_pending_sync" || issue.tool_key.as_deref() != Some("claude_code")
    }));
    assert_eq!(
        store
            .get_project_skill_link(&project.id, &skill.id)
            .unwrap()
            .unwrap()
            .desired_tools,
        vec!["codex"]
    );
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
    let mut link = store
        .get_project_skill_link(&project.id, &skill.id)
        .unwrap()
        .unwrap();
    link.desired_tools = vec!["codex".to_string(), "cursor".to_string()];
    store.upsert_project_skill_link(&link).unwrap();
    scan_project(&store, &updated, &tools).expect("scan shared directory");
    let issue = diagnose_project(&store, &updated, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| {
            issue.kind == "tool_added_pending_sync" && issue.tool_key.as_deref() == Some("cursor")
        })
        .expect("cursor pending issue");

    repair_project_issue(&store, &updated, &tools, &issue.id, "retry_injection", None)
        .expect("repair cursor target");

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
fn disabled_library_skill_can_be_enabled_and_retried() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let mut skill = make_skill("library-review", "review", central_dir.path(), &hash);
    skill.enabled = false;
    store.upsert_skill(&skill).expect("insert disabled skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    inject_project_skill(&store, &project, &skill, &tools)
        .expect_err("disabled injection must fail");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "library_skill_unavailable")
        .expect("unavailable library issue");

    assert!(issue
        .actions
        .iter()
        .any(|action| action.kind == "enable_library_skill"));
    assert!(issue
        .actions
        .iter()
        .any(|action| action.kind == "remove_unavailable_library_skill"));

    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "enable_library_skill",
        None,
    )
    .expect("enable and retry skill");
    scan_project(&store, &project, &tools).expect("rescan project");

    assert!(
        store
            .get_skill_by_id(&skill.id)
            .expect("get skill")
            .expect("skill exists")
            .enabled
    );
    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose repaired project")
        .iter()
        .all(|issue| issue.kind != "library_skill_unavailable"));
}

#[test]
fn missing_library_skill_can_be_removed_from_the_project() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_root = tempfile::tempdir().expect("central root");
    let central_path = central_root.path().join("review");
    write_skill(&central_path, "library version");
    let hash = crate::core::content_hash::hash_dir(&central_path).expect("hash skill");
    let skill = make_skill("library-review", "review", &central_path, &hash);
    store.upsert_skill(&skill).expect("insert skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject skill");
    std::fs::remove_dir_all(&central_path).expect("remove central skill");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "library_skill_unavailable")
        .expect("unavailable library issue");

    assert_eq!(issue.actions.len(), 1);
    assert_eq!(issue.actions[0].kind, "remove_unavailable_library_skill");
    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "remove_unavailable_library_skill",
        None,
    )
    .expect("remove unavailable skill");
    scan_project(&store, &project, &tools).expect("rescan project");

    assert!(diagnose_project(&store, &project, &tools)
        .expect("diagnose repaired project")
        .iter()
        .all(|issue| issue.kind != "library_skill_unavailable"));
    assert!(
        !store
            .get_project_skill_link(&project.id, &skill.id)
            .expect("get project link")
            .expect("project link exists")
            .enabled
    );
}

#[test]
fn unavailable_git_exclude_offers_both_resolutions_and_can_be_disabled() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert skill");
    let mut project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    project.local_git_exclude_enabled = true;
    store
        .update_project(&project)
        .expect("enable local exclude");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject skill");
    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose project")
        .into_iter()
        .find(|issue| issue.kind == "git_exclude_unavailable")
        .expect("git exclude issue");

    assert_eq!(issue.actions.len(), 2);
    assert!(issue
        .actions
        .iter()
        .any(|action| action.kind == "disable_git_exclude"));
    let initialize_action = issue
        .actions
        .iter()
        .find(|action| action.kind == "initialize_git_repository")
        .expect("initialize action");
    assert!(initialize_action.confirmation_required);
    assert!(initialize_action.destructive);

    repair_project_issue(
        &store,
        &project,
        &tools,
        &issue.id,
        "disable_git_exclude",
        None,
    )
    .expect("disable local exclude");
    let updated = store
        .get_project(&project.id)
        .expect("get project")
        .expect("project exists");

    assert!(!updated.local_git_exclude_enabled);
    assert!(diagnose_project(&store, &updated, &tools)
        .expect("diagnose repaired project")
        .iter()
        .all(|issue| issue.kind != "git_exclude_unavailable"));

    let mut reenabled = updated;
    reenabled.local_git_exclude_enabled = true;
    store
        .update_project(&reenabled)
        .expect("reenable local exclude");
    let issue = diagnose_project(&store, &reenabled, &tools)
        .expect("diagnose project again")
        .into_iter()
        .find(|issue| issue.kind == "git_exclude_unavailable")
        .expect("git exclude issue again");

    repair_project_issue(
        &store,
        &reenabled,
        &tools,
        &issue.id,
        "initialize_git_repository",
        None,
    )
    .expect("initialize Git repository");

    assert!(project_dir.path().join(".git").is_dir());
    assert!(
        std::fs::read_to_string(project_dir.path().join(".git/info/exclude"))
            .expect("read Git exclude")
            .contains("/.agents/skills/review/")
    );
    assert!(diagnose_project(&store, &reenabled, &tools)
        .expect("diagnose initialized repository")
        .iter()
        .all(|issue| issue.kind != "git_exclude_unavailable"));
}

#[test]
fn broken_git_metadata_does_not_offer_repository_initialization() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert skill");
    let mut project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    project.local_git_exclude_enabled = true;
    store
        .update_project(&project)
        .expect("enable local exclude");
    let tools = [ProjectToolSpec::new("codex", ".agents/skills")];
    inject_project_skill(&store, &project, &skill, &tools).expect("inject skill");
    std::fs::write(project_dir.path().join(".git"), "corrupt git metadata")
        .expect("write corrupt Git marker");

    let issue = diagnose_project(&store, &project, &tools)
        .expect("diagnose broken Git project")
        .into_iter()
        .find(|issue| issue.kind == "git_exclude_unavailable")
        .expect("git exclude issue");

    assert_eq!(issue.actions.len(), 1);
    assert_eq!(issue.actions[0].kind, "disable_git_exclude");
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

    repair_project_issue(
        &store,
        &updated,
        &tools,
        &issue.id,
        "remove_stale_target",
        None,
    )
    .expect("repair unsync");

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
fn project_settings_do_not_replace_per_skill_tool_overrides() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let tools = [
        ProjectToolSpec::new("codex", ".agents/skills"),
        ProjectToolSpec::new("cursor", ".cursor/skills"),
    ];
    assign_project_skill_tools_and_scan(
        &store,
        &project.id,
        &skill.id,
        &["codex".to_string()],
        &tools,
        &std::collections::HashSet::new(),
    )
    .expect("assign project skill");

    update_project_configuration(
        &store,
        &project.id,
        &["codex".to_string(), "cursor".to_string()],
        false,
    )
    .expect("update project settings");

    let link = store
        .get_project_skill_link(&project.id, &skill.id)
        .unwrap()
        .unwrap();
    assert_eq!(link.desired_tools, vec!["codex"]);
}

#[test]
fn project_issue_fingerprint_ignores_scan_timestamps_but_tracks_relevant_state() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    let issue_path = project_dir.path().join(".agents/skills/review");
    write_skill(&issue_path, "project content");
    let issue = ProjectIssue {
        id: "git_exclude_drift".to_string(),
        kind: "git_exclude_drift".to_string(),
        skill_id: None,
        skill_name: None,
        tool_key: None,
        path: Some(issue_path.to_string_lossy().to_string()),
        actions: Vec::new(),
    };
    let first = project_issue_fingerprint(&store, &project, &issue).unwrap();

    let mut rescanned = project.clone();
    rescanned.updated_at += 10_000;
    rescanned.last_scanned_at = Some(20_000);
    store.update_project(&rescanned).unwrap();
    let second = project_issue_fingerprint(&store, &rescanned, &issue).unwrap();
    assert_eq!(first, second);

    rescanned.local_git_exclude_enabled = true;
    let changed = project_issue_fingerprint(&store, &rescanned, &issue).unwrap();
    assert_ne!(second, changed);
}

#[test]
fn target_failure_requires_another_real_assignment_before_it_can_be_ignored() {
    let (_db_dir, store) = make_store();
    let project_dir = tempfile::tempdir().expect("project tempdir");
    let central_dir = tempfile::tempdir().expect("central tempdir");
    write_skill(central_dir.path(), "library version");
    let hash = crate::core::content_hash::hash_dir(central_dir.path()).expect("hash skill");
    let skill = make_skill("library-review", "review", central_dir.path(), &hash);
    store.upsert_skill(&skill).expect("insert skill");
    let project = create_or_get_project(&store, project_dir.path(), &["codex".to_string()])
        .expect("create project");
    store
        .upsert_skill_assignment(&SkillAssignmentRecord {
            id: "project-assignment".to_string(),
            skill_id: skill.id.clone(),
            tool: "codex".to_string(),
            scope: "project".to_string(),
            project_id: Some(project.id.clone()),
            ownership: "hub".to_string(),
            state: "error".to_string(),
            last_error: Some("missing".to_string()),
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    let issue = ProjectIssue {
        id: format!("target_missing:{}:codex", skill.id),
        kind: "target_missing".to_string(),
        skill_id: Some(skill.id.clone()),
        skill_name: Some(skill.name.clone()),
        tool_key: Some("codex".to_string()),
        path: None,
        actions: Vec::new(),
    };
    assert!(!can_ignore_project_issue(&store, &project, &issue).unwrap());

    let global_dir = tempfile::tempdir().expect("global target");
    let target_path = global_dir.path().join("review");
    write_skill(&target_path, "library version");
    store
        .upsert_skill_target(&SkillTargetRecord {
            id: "global-target".to_string(),
            skill_id: skill.id.clone(),
            tool: "cursor".to_string(),
            scope: "global".to_string(),
            project_path: None,
            target_path: target_path.to_string_lossy().to_string(),
            mode: "copy".to_string(),
            status: "ok".to_string(),
            last_error: None,
            synced_at: Some(1),
        })
        .unwrap();
    store
        .upsert_skill_assignment(&SkillAssignmentRecord {
            id: "global-assignment".to_string(),
            skill_id: skill.id.clone(),
            tool: "cursor".to_string(),
            scope: "global".to_string(),
            project_id: None,
            ownership: "hub".to_string(),
            state: "active".to_string(),
            last_error: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    assert!(can_ignore_project_issue(&store, &project, &issue).unwrap());
    let projection = project_issue_projections(&store, &project, vec![issue.clone()])
        .unwrap()
        .remove(0);
    store
        .upsert_ignored_issue(&IgnoredIssueRecord {
            id: "ignored-project-issue".to_string(),
            scope: "project".to_string(),
            scope_id: Some(project.id.clone()),
            issue_key: issue.id.clone(),
            fingerprint: projection.fingerprint,
            ignored_at: 1,
        })
        .unwrap();
    assert!(project_issue_projections(&store, &project, vec![issue.clone()]).unwrap()[0].ignored);
    assert!(
        visible_project_issue_projections(&store, &project, vec![issue.clone()])
            .unwrap()
            .is_empty()
    );

    std::fs::remove_dir_all(&target_path).unwrap();
    let blocker = project_issue_projections(&store, &project, vec![issue.clone()]).unwrap();
    assert!(!blocker[0].can_ignore);
    assert!(!blocker[0].ignored);
    assert_eq!(
        visible_project_issue_projections(&store, &project, vec![issue])
            .unwrap()
            .len(),
        1
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
