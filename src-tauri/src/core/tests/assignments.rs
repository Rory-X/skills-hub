use std::collections::HashSet;
use std::path::Path;

use crate::core::skill_store::{ProjectRecord, SkillAssignmentRecord, SkillRecord, SkillStore};

use super::{
    apply_project_skill_assignments, apply_skill_assignment, apply_skill_assignments,
    cleanup_orphaned_skill_symlinks, disable_skill_atomically, enable_skill_and_restore,
    ignore_assignment_issue, managed_skill_projections, preview_project_skill_assignments,
    preview_skill_assignment, project_skill_assignments, AssignmentConflictKey, AssignmentDraft,
    AssignmentToolSpec, ProjectAssignmentDraft,
};

#[test]
fn batch_assignment_reports_a_missing_skill_without_hiding_prior_successes() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    let outcomes = apply_skill_assignments(
        &store,
        &[skill.id.clone(), "missing-skill".to_string()],
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("batch result");

    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].summary.succeeded, 1);
    assert_eq!(outcomes[1].skill_id, "missing-skill");
    assert_eq!(outcomes[1].summary.failed, 1);
    assert!(global.path().join("review").exists());
}

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SkillStore::new(dir.path().join("test.db"));
    store.ensure_schema().expect("ensure schema");
    (dir, store)
}

fn write_skill(path: &Path, content: &str) {
    std::fs::create_dir_all(path).expect("create skill");
    std::fs::write(path.join("SKILL.md"), content).expect("write skill");
}

fn add_skill(store: &SkillStore, central: &Path) -> SkillRecord {
    let skill = SkillRecord {
        id: "skill-1".to_string(),
        name: "review".to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: None,
        source_subpath: None,
        source_revision: None,
        central_path: central.to_string_lossy().to_string(),
        content_hash: Some(crate::core::content_hash::hash_dir(central).expect("hash skill")),
        created_at: 1,
        updated_at: 1,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&skill).expect("insert skill");
    skill
}

fn tool(global: &Path) -> AssignmentToolSpec {
    AssignmentToolSpec {
        key: "codex".to_string(),
        label: "Codex".to_string(),
        global_skills_dir: global.to_path_buf(),
        project_skills_dir: ".agents/skills".to_string(),
        global_available: true,
        enabled: true,
        supports_project_scope: true,
    }
}

#[test]
fn disabling_hides_targets_and_enabling_restores_saved_assignments() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    let target = global.path().join(&skill.name);

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("assign skill");
    disable_skill_atomically(&store, &skill.id, &tools).expect("disable skill");

    assert!(std::fs::symlink_metadata(&target).is_err());
    assert!(!store.get_skill_by_id(&skill.id).unwrap().unwrap().enabled);
    assert_eq!(
        store.list_skill_assignments(&skill.id).unwrap()[0].state,
        "suspended"
    );

    let outcome = enable_skill_and_restore(&store, &skill.id, &tools).expect("enable skill");
    assert_eq!(outcome.restored, 1);
    assert_eq!(outcome.failed, 0);
    assert!(std::fs::symlink_metadata(&target).is_ok());
    assert!(store.get_skill_by_id(&skill.id).unwrap().unwrap().enabled);
    assert_eq!(
        store.list_skill_assignments(&skill.id).unwrap()[0].state,
        "active"
    );
}

#[test]
fn enabling_reports_a_target_conflict_as_a_restore_failure() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    let target = global.path().join(&skill.name);

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("assign skill");
    disable_skill_atomically(&store, &skill.id, &tools).expect("disable skill");
    write_skill(&target, "unmanaged conflict");

    let outcome = enable_skill_and_restore(&store, &skill.id, &tools).expect("enable skill");
    assert_eq!(outcome.restored, 0);
    assert_eq!(outcome.failed, 1);
    assert!(outcome.errors[0].contains("could not be restored"));
    assert!(store.get_skill_by_id(&skill.id).unwrap().unwrap().enabled);
    assert_eq!(
        store.list_skill_assignments(&skill.id).unwrap()[0].state,
        "suspended"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "unmanaged conflict"
    );
}

#[cfg(unix)]
#[test]
fn failed_disable_restores_moved_targets_and_keeps_database_state() {
    use std::os::unix::fs::PermissionsExt;

    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let roots = tempfile::tempdir().expect("tool roots");
    let first_root = roots.path().join("a-writable");
    let second_root = roots.path().join("z-read-only");
    std::fs::create_dir_all(&first_root).unwrap();
    std::fs::create_dir_all(&second_root).unwrap();
    let mut first = tool(&first_root);
    first.key = "first".to_string();
    let mut second = tool(&second_root);
    second.key = "second".to_string();
    let tools = [first, second];

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["first".to_string(), "second".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("assign both targets");
    std::fs::set_permissions(&second_root, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = disable_skill_atomically(&store, &skill.id, &tools);
    std::fs::set_permissions(&second_root, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(result.is_err());
    assert!(std::fs::symlink_metadata(first_root.join(&skill.name)).is_ok());
    assert!(std::fs::symlink_metadata(second_root.join(&skill.name)).is_ok());
    assert!(store.get_skill_by_id(&skill.id).unwrap().unwrap().enabled);
    assert!(store
        .list_skill_assignments(&skill.id)
        .unwrap()
        .iter()
        .all(|assignment| assignment.state == "active"));
}

#[cfg(unix)]
#[test]
fn orphan_cleanup_only_removes_direct_known_unassigned_symlinks() {
    use std::os::unix::fs::symlink;

    let (_db, store) = make_store();
    let central_repo = tempfile::tempdir().expect("central repo");
    let central = central_repo.path().join("review");
    write_skill(&central, "library");
    let skill = add_skill(&store, &central);
    let assigned_root = tempfile::tempdir().expect("assigned root");
    let orphan_root = tempfile::tempdir().expect("orphan root");
    let external = tempfile::tempdir().expect("external");
    write_skill(external.path(), "external");
    let mut assigned_tool = tool(assigned_root.path());
    assigned_tool.key = "assigned".to_string();
    let mut orphan_tool = tool(orphan_root.path());
    orphan_tool.key = "orphan".to_string();
    let mut central_guard = tool(central_repo.path());
    central_guard.key = "central-guard".to_string();
    let tools = [assigned_tool, orphan_tool, central_guard];

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["assigned".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("assign managed target");
    let orphan = orphan_root.path().join(&skill.name);
    symlink(&central, &orphan).unwrap();
    let external_link = orphan_root.path().join("external");
    symlink(external.path(), &external_link).unwrap();
    let regular = orphan_root.path().join("regular");
    write_skill(&regular, "regular");
    let nested = orphan_root.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let nested_link = nested.join(&skill.name);
    symlink(&central, &nested_link).unwrap();
    let broken = orphan_root.path().join("broken");
    symlink(orphan_root.path().join("missing"), &broken).unwrap();
    let central_alias = central_repo.path().join("review-alias");
    symlink(&central, &central_alias).unwrap();

    let removed = cleanup_orphaned_skill_symlinks(&store, &tools).expect("cleanup links");

    assert_eq!(removed, 1);
    assert!(std::fs::symlink_metadata(&orphan).is_err());
    assert!(std::fs::symlink_metadata(assigned_root.path().join(&skill.name)).is_ok());
    assert!(std::fs::symlink_metadata(&external_link).is_ok());
    assert!(regular.is_dir());
    assert!(std::fs::symlink_metadata(&nested_link).is_ok());
    assert!(std::fs::symlink_metadata(&broken).is_ok());
    assert!(std::fs::symlink_metadata(&central_alias).is_ok());
}

#[test]
fn conflict_can_keep_the_existing_target_or_replace_it_explicitly() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let existing = global.path().join("review");
    write_skill(&existing, "existing");
    let tools = [tool(global.path())];
    let draft = AssignmentDraft {
        global_tools: vec!["codex".to_string()],
        projects: Vec::new(),
    };

    let preview = preview_skill_assignment(&store, &skill.id, &draft, &tools, true).unwrap();
    assert_eq!(preview[0].outcome, "conflict");
    let skipped =
        apply_skill_assignment(&store, &skill.id, &draft, &tools, &HashSet::new(), false).unwrap();
    assert_eq!(skipped.skipped, 1);
    let blocked = store.list_skill_assignments(&skill.id).unwrap();
    assert!(blocked.is_empty());
    assert_eq!(
        std::fs::read_to_string(existing.join("SKILL.md")).unwrap(),
        "existing"
    );

    let replaced = apply_skill_assignment(
        &store,
        &skill.id,
        &draft,
        &tools,
        &HashSet::from(["global|codex".to_string()]),
        false,
    )
    .unwrap();
    assert_eq!(replaced.succeeded, 1);
    assert_eq!(
        std::fs::read_to_string(existing.join("SKILL.md")).unwrap(),
        "library"
    );
}

#[test]
fn replacement_conflict_keeps_the_skills_existing_assignments_unchanged() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let codex_global = tempfile::tempdir().expect("codex global");
    let cursor_global = tempfile::tempdir().expect("cursor global");
    let codex = tool(codex_global.path());
    let mut cursor = tool(cursor_global.path());
    cursor.key = "cursor".to_string();
    cursor.label = "Cursor".to_string();
    let tools = [codex, cursor];

    let initial = AssignmentDraft {
        global_tools: vec!["codex".to_string()],
        projects: Vec::new(),
    };
    apply_skill_assignment(&store, &skill.id, &initial, &tools, &HashSet::new(), false)
        .expect("apply initial assignment");
    write_skill(&cursor_global.path().join("review"), "unmanaged conflict");

    let replacement = AssignmentDraft {
        global_tools: vec!["cursor".to_string()],
        projects: Vec::new(),
    };
    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &replacement,
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("skip conflicting replacement");

    assert_eq!(result.skipped, 1);
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].tool, "codex");
    assert_eq!(assignments[0].scope, "global");
    assert!(codex_global.path().join("review").exists());
    assert_eq!(
        std::fs::read_to_string(cursor_global.path().join("review/SKILL.md")).unwrap(),
        "unmanaged conflict"
    );
}

#[test]
fn unavailable_replacement_keeps_the_skills_existing_assignments_unchanged() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let codex_global = tempfile::tempdir().expect("codex global");
    let cursor_global = tempfile::tempdir().expect("cursor global");
    let codex = tool(codex_global.path());
    let mut cursor = tool(cursor_global.path());
    cursor.key = "cursor".to_string();
    cursor.label = "Cursor".to_string();
    cursor.global_available = false;
    let tools = [codex, cursor];

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("apply initial assignment");

    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["cursor".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("reject unavailable replacement");

    assert_eq!(result.failed, 1);
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].tool, "codex");
    assert!(codex_global.path().join("review").exists());
}

#[test]
fn failed_replacement_keeps_the_skills_existing_assignments_unchanged() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let codex_global = tempfile::tempdir().expect("codex global");
    let invalid_parent = tempfile::tempdir().expect("invalid parent");
    let invalid_global = invalid_parent.path().join("not-a-directory");
    std::fs::write(&invalid_global, "file").expect("write blocking file");
    let codex = tool(codex_global.path());
    let mut cursor = tool(&invalid_global);
    cursor.key = "cursor".to_string();
    cursor.label = "Cursor".to_string();
    let tools = [codex, cursor];

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("apply initial assignment");

    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["cursor".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("report failed replacement");

    assert_eq!(result.failed, 1);
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert!(assignments
        .iter()
        .any(|assignment| assignment.tool == "codex"));
    assert!(codex_global.path().join("review").exists());
}

#[test]
fn failed_project_replacement_keeps_the_existing_project_assignment() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let project_dir = tempfile::tempdir().expect("project");
    let project = ProjectRecord {
        id: "project-1".to_string(),
        name: "Project".to_string(),
        root_path: project_dir.path().to_string_lossy().to_string(),
        enabled_tool_keys: vec!["codex".to_string(), "cursor".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).expect("insert project");
    let global = tempfile::tempdir().expect("global");
    let codex = tool(global.path());
    let mut cursor = tool(global.path());
    cursor.key = "cursor".to_string();
    cursor.label = "Cursor".to_string();
    cursor.project_skills_dir = ".cursor/skills".to_string();
    let tools = [codex, cursor];

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: Vec::new(),
            projects: vec![ProjectAssignmentDraft {
                project_id: project.id.clone(),
                tool_keys: vec!["codex".to_string()],
            }],
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("apply initial project assignment");
    std::fs::write(project_dir.path().join(".cursor"), "blocking file")
        .expect("write blocking file");

    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: Vec::new(),
            projects: vec![ProjectAssignmentDraft {
                project_id: project.id.clone(),
                tool_keys: vec!["cursor".to_string()],
            }],
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("report failed project replacement");

    assert_eq!(result.failed, 1);
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert_eq!(assignments.len(), 1);
    assert!(assignments
        .iter()
        .any(|assignment| { assignment.scope == "project" && assignment.tool == "codex" }));
    assert!(project_dir.path().join(".agents/skills/review").exists());
}

#[test]
fn project_scoped_changes_do_not_process_other_assignments() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let project_one_dir = tempfile::tempdir().expect("project one");
    let project_two_dir = tempfile::tempdir().expect("project two");
    for (id, root) in [
        ("project-1", project_one_dir.path()),
        ("project-2", project_two_dir.path()),
    ] {
        store
            .insert_project(&ProjectRecord {
                id: id.to_string(),
                name: id.to_string(),
                root_path: root.to_string_lossy().to_string(),
                enabled_tool_keys: vec![
                    "codex".to_string(),
                    "cursor".to_string(),
                    "legacy".to_string(),
                ],
                local_git_exclude_enabled: false,
                created_at: 1,
                updated_at: 1,
                last_scanned_at: None,
            })
            .expect("insert project");
    }
    let codex_global = tempfile::tempdir().expect("codex global");
    let cursor_global = tempfile::tempdir().expect("cursor global");
    let legacy_global = tempfile::tempdir().expect("legacy global");
    let codex = tool(codex_global.path());
    let mut cursor = tool(cursor_global.path());
    cursor.key = "cursor".to_string();
    cursor.label = "Cursor".to_string();
    cursor.project_skills_dir = ".cursor/skills".to_string();
    let mut legacy = tool(legacy_global.path());
    legacy.key = "legacy".to_string();
    legacy.label = "Legacy".to_string();
    legacy.project_skills_dir = ".legacy/skills".to_string();
    let tools = [codex, cursor, legacy];

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["legacy".to_string()],
            projects: vec![
                ProjectAssignmentDraft {
                    project_id: "project-1".to_string(),
                    tool_keys: vec!["codex".to_string()],
                },
                ProjectAssignmentDraft {
                    project_id: "project-2".to_string(),
                    tool_keys: vec!["legacy".to_string()],
                },
            ],
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("apply initial assignments");

    let mut scoped_tools = tools.clone();
    scoped_tools[2].global_available = false;
    let previews = preview_project_skill_assignments(
        &store,
        std::slice::from_ref(&skill.id),
        "project-1",
        &["cursor".to_string()],
        &scoped_tools,
        false,
    )
    .expect("preview scoped replacement");
    assert!(previews[0]
        .items
        .iter()
        .all(|item| item.project_id.as_deref() == Some("project-1")));

    let outcomes = apply_project_skill_assignments(
        &store,
        std::slice::from_ref(&skill.id),
        "project-1",
        &["cursor".to_string()],
        &scoped_tools,
        &HashSet::new(),
        false,
    )
    .expect("apply scoped replacement");
    assert_eq!(outcomes[0].summary.failed, 0);

    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert!(assignments
        .iter()
        .any(|assignment| { assignment.scope == "global" && assignment.tool == "legacy" }));
    assert!(assignments.iter().any(|assignment| {
        assignment.project_id.as_deref() == Some("project-2") && assignment.tool == "legacy"
    }));
    assert!(assignments.iter().any(|assignment| {
        assignment.project_id.as_deref() == Some("project-1") && assignment.tool == "cursor"
    }));
    assert!(legacy_global.path().join("review").exists());
    assert!(project_two_dir
        .path()
        .join(".legacy/skills/review")
        .exists());
    assert!(!project_one_dir
        .path()
        .join(".agents/skills/review")
        .exists());
    assert!(project_one_dir
        .path()
        .join(".cursor/skills/review")
        .exists());

    let added = apply_project_skill_assignments(
        &store,
        std::slice::from_ref(&skill.id),
        "project-1",
        &["codex".to_string()],
        &scoped_tools,
        &HashSet::new(),
        true,
    )
    .expect("add scoped assignment");
    assert_eq!(added[0].summary.failed, 0);
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert!(assignments.iter().any(|assignment| {
        assignment.project_id.as_deref() == Some("project-1") && assignment.tool == "cursor"
    }));
    assert!(assignments.iter().any(|assignment| {
        assignment.project_id.as_deref() == Some("project-1") && assignment.tool == "codex"
    }));
    assert!(assignments
        .iter()
        .any(|assignment| { assignment.scope == "global" && assignment.tool == "legacy" }));
    assert!(assignments.iter().any(|assignment| {
        assignment.project_id.as_deref() == Some("project-2") && assignment.tool == "legacy"
    }));
}

#[test]
fn later_project_failure_restores_an_earlier_project_replacement() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let project_one_dir = tempfile::tempdir().expect("project one");
    let project_two_dir = tempfile::tempdir().expect("project two");
    for (id, root) in [
        ("project-1", project_one_dir.path()),
        ("project-2", project_two_dir.path()),
    ] {
        store
            .insert_project(&ProjectRecord {
                id: id.to_string(),
                name: id.to_string(),
                root_path: root.to_string_lossy().to_string(),
                enabled_tool_keys: vec!["codex".to_string(), "cursor".to_string()],
                local_git_exclude_enabled: false,
                created_at: 1,
                updated_at: 1,
                last_scanned_at: None,
            })
            .expect("insert project");
    }
    let global = tempfile::tempdir().expect("global");
    let codex = tool(global.path());
    let mut cursor = tool(global.path());
    cursor.key = "cursor".to_string();
    cursor.label = "Cursor".to_string();
    cursor.project_skills_dir = ".cursor/skills".to_string();
    let tools = [codex, cursor];
    let initial = AssignmentDraft {
        global_tools: Vec::new(),
        projects: vec![
            ProjectAssignmentDraft {
                project_id: "project-1".to_string(),
                tool_keys: vec!["codex".to_string()],
            },
            ProjectAssignmentDraft {
                project_id: "project-2".to_string(),
                tool_keys: vec!["codex".to_string()],
            },
        ],
    };
    apply_skill_assignment(&store, &skill.id, &initial, &tools, &HashSet::new(), false)
        .expect("apply initial assignments");
    std::fs::write(project_two_dir.path().join(".cursor"), "blocking file")
        .expect("write blocking file");
    let mut replacement_tools = tools.clone();
    replacement_tools[0].global_available = false;

    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: Vec::new(),
            projects: vec![
                ProjectAssignmentDraft {
                    project_id: "project-1".to_string(),
                    tool_keys: vec!["cursor".to_string()],
                },
                ProjectAssignmentDraft {
                    project_id: "project-2".to_string(),
                    tool_keys: vec!["cursor".to_string()],
                },
            ],
        },
        &replacement_tools,
        &HashSet::new(),
        false,
    )
    .expect("report later project failure");

    assert_eq!(result.failed, 1);
    assert!(project_one_dir
        .path()
        .join(".agents/skills/review")
        .exists());
    assert!(!project_one_dir
        .path()
        .join(".cursor/skills/review")
        .exists());
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert_eq!(
        assignments
            .iter()
            .filter(|assignment| assignment.scope == "project")
            .map(|assignment| assignment.tool.as_str())
            .collect::<Vec<_>>(),
        vec!["codex", "codex"]
    );
}

#[test]
fn clearing_assignments_reports_and_removes_owned_targets() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("apply initial assignment");

    let cleared = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft::default(),
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("clear assignments");

    assert_eq!(cleared.succeeded, 1);
    assert!(store.list_skill_assignments(&skill.id).unwrap().is_empty());
    assert!(!global.path().join("review").exists());
}

#[test]
fn disabled_skill_saves_replacement_as_suspended_without_creating_a_target() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    store
        .set_skill_enabled(&skill.id, false)
        .expect("disable skill");
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];

    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("save suspended assignment");

    assert_eq!(result.succeeded, 1);
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].state, "suspended");
    assert!(!global.path().join("review").exists());
}

#[test]
fn clearing_refuses_to_delete_a_target_the_user_modified() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("apply initial assignment");
    let target = global.path().join("review");
    if std::fs::symlink_metadata(&target)
        .expect("target metadata")
        .file_type()
        .is_symlink()
    {
        std::fs::remove_file(&target).expect("remove managed link");
        write_skill(&target, "user replacement");
    } else {
        std::fs::write(target.join("SKILL.md"), "user replacement").expect("modify managed copy");
    }

    let preview =
        preview_skill_assignment(&store, &skill.id, &AssignmentDraft::default(), &tools, true)
            .expect("preview unsafe clear");
    assert_eq!(preview[0].outcome, "unsafe");

    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft::default(),
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("reject unsafe clear");
    assert_eq!(result.failed, 1);
    assert_eq!(store.list_skill_assignments(&skill.id).unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
        "user replacement"
    );
}

#[test]
fn assignment_conflict_keys_roundtrip_as_a_core_domain_value() {
    let key = AssignmentConflictKey::new("skill-1", "project|project-1|codex");
    assert_eq!(AssignmentConflictKey::decode(&key.encode()).unwrap(), key);
    assert!(AssignmentConflictKey::decode("invalid").is_err());
}

#[test]
fn keeping_a_conflicting_project_skill_cancels_the_hub_assignment() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let project_dir = tempfile::tempdir().expect("project");
    let project_skill = project_dir.path().join(".agents/skills/review");
    write_skill(&project_skill, "project");
    let project = ProjectRecord {
        id: "project-1".to_string(),
        name: "Project".to_string(),
        root_path: project_dir.path().to_string_lossy().to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).unwrap();
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    let draft = AssignmentDraft {
        global_tools: Vec::new(),
        projects: vec![ProjectAssignmentDraft {
            project_id: project.id.clone(),
            tool_keys: vec!["codex".to_string()],
        }],
    };

    let result =
        apply_skill_assignment(&store, &skill.id, &draft, &tools, &HashSet::new(), false).unwrap();

    assert_eq!(result.skipped, 1);
    assert!(store.list_skill_assignments(&skill.id).unwrap().is_empty());
    assert!(store
        .get_project_skill_link(&project.id, &skill.id)
        .unwrap()
        .is_none());
    assert_eq!(
        std::fs::read_to_string(project_skill.join("SKILL.md")).unwrap(),
        "project"
    );
}

#[test]
fn project_assignment_scan_includes_builtin_compatibility_directories() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let project_dir = tempfile::tempdir().expect("project");
    write_skill(
        &project_dir.path().join(".codex/skills/legacy-skill"),
        "legacy skill",
    );
    let project = ProjectRecord {
        id: "project-1".to_string(),
        name: "Project".to_string(),
        root_path: project_dir.path().to_string_lossy().to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).expect("insert project");
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: Vec::new(),
            projects: vec![ProjectAssignmentDraft {
                project_id: project.id.clone(),
                tool_keys: vec!["codex".to_string()],
            }],
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .expect("apply project assignment");

    let discovered = store
        .list_project_discovered_skills(&project.id)
        .expect("list discovered skills");
    assert!(
        discovered.iter().any(|item| {
            item.name == "legacy-skill"
                && item.relative_path == ".codex/skills/legacy-skill"
                && item.tool_keys == vec!["codex"]
        }),
        "discovered: {discovered:#?}"
    );
}

#[test]
fn uninstalled_agent_is_unavailable_for_project_preview_and_apply() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let project_dir = tempfile::tempdir().expect("project");
    let project = ProjectRecord {
        id: "project-1".to_string(),
        name: "Project".to_string(),
        root_path: project_dir.path().to_string_lossy().to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).expect("insert project");
    let global = tempfile::tempdir().expect("global");
    let mut unavailable = tool(global.path());
    unavailable.global_available = false;
    let tools = [unavailable];
    let draft = AssignmentDraft {
        global_tools: Vec::new(),
        projects: vec![ProjectAssignmentDraft {
            project_id: project.id.clone(),
            tool_keys: vec!["codex".to_string()],
        }],
    };

    let preview = preview_skill_assignment(&store, &skill.id, &draft, &tools, false)
        .expect("preview assignment");
    assert_eq!(preview[0].outcome, "unavailable");

    let applied = apply_skill_assignment(&store, &skill.id, &draft, &tools, &HashSet::new(), false)
        .expect("apply assignment");
    assert_eq!(applied.failed, 1);
    assert!(!project_dir.path().join(".agents/skills/review").exists());
}

#[test]
fn same_content_global_target_is_taken_over_without_rewriting_it() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "same");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    write_skill(&global.path().join("review"), "same");
    let tools = [tool(global.path())];
    let draft = AssignmentDraft {
        global_tools: vec!["codex".to_string()],
        projects: Vec::new(),
    };

    let applied =
        apply_skill_assignment(&store, &skill.id, &draft, &tools, &HashSet::new(), false).unwrap();

    assert_eq!(applied.succeeded, 1);
    assert_eq!(
        store.list_skill_assignments(&skill.id).unwrap()[0].state,
        "active"
    );
    assert_eq!(
        store
            .get_skill_target(&skill.id, "codex", "global", None)
            .unwrap()
            .unwrap()
            .mode,
        "existing"
    );

    let removal_preview =
        preview_skill_assignment(&store, &skill.id, &AssignmentDraft::default(), &tools, true)
            .unwrap();
    assert_eq!(removal_preview.len(), 1);
    assert_eq!(removal_preview[0].outcome, "remove");

    std::fs::write(global.path().join("review/SKILL.md"), "modified").unwrap();
    let preview = preview_skill_assignment(&store, &skill.id, &draft, &tools, true).unwrap();
    assert_eq!(preview[0].outcome, "conflict");
}

#[test]
fn matching_project_skill_is_recorded_as_native_assignment() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "same");
    let skill = add_skill(&store, central.path());
    let project_dir = tempfile::tempdir().expect("project");
    let project_skill = project_dir.path().join(".agents/skills/review");
    write_skill(&project_skill, "same");
    let project = ProjectRecord {
        id: "project-1".to_string(),
        name: "Project".to_string(),
        root_path: project_dir.path().to_string_lossy().to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).expect("insert project");
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    let draft = AssignmentDraft {
        global_tools: Vec::new(),
        projects: vec![ProjectAssignmentDraft {
            project_id: project.id.clone(),
            tool_keys: vec!["codex".to_string()],
        }],
    };

    let applied =
        apply_skill_assignment(&store, &skill.id, &draft, &tools, &HashSet::new(), false).unwrap();

    assert_eq!(applied.succeeded, 1);
    let assignment = store.list_skill_assignments(&skill.id).unwrap().remove(0);
    assert_eq!(assignment.ownership, "native");
    assert_eq!(assignment.state, "active");
    let projection = project_skill_assignments(&store, &skill, &tools).unwrap();
    assert!(projection[0].valid);
    assert_eq!(projection[0].effective_state, "active");
    assert!(store
        .get_skill_target(&skill.id, "codex", "project", Some(&project.root_path),)
        .unwrap()
        .is_none());
    assert_eq!(
        std::fs::read_to_string(project_skill.join("SKILL.md")).unwrap(),
        "same"
    );
}

#[test]
fn missing_real_target_invalidates_an_active_assignment() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];
    let draft = AssignmentDraft {
        global_tools: vec!["codex".to_string()],
        projects: Vec::new(),
    };
    apply_skill_assignment(&store, &skill.id, &draft, &tools, &HashSet::new(), false)
        .expect("apply assignment");
    assert!(project_skill_assignments(&store, &skill, &tools).unwrap()[0].valid);

    let target = global.path().join("review");
    let metadata = std::fs::symlink_metadata(&target).expect("target metadata");
    if metadata.file_type().is_symlink() {
        std::fs::remove_file(&target).expect("remove symlink");
    } else {
        std::fs::remove_dir_all(&target).expect("remove target");
    }

    let projection = project_skill_assignments(&store, &skill, &tools).unwrap();
    assert!(!projection[0].valid);
    assert_eq!(projection[0].issue_kind.as_deref(), Some("target_missing"));
    assert!(!projection[0].can_ignore);
}

#[test]
fn managed_skill_projection_derives_pending_and_visible_issue_counts() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let tools = [tool(global.path())];

    let pending = managed_skill_projections(&store, &tools).unwrap();
    assert!(pending[0].pending_assignment);
    assert_eq!(pending[0].assignment_issue_count, 0);
    assert!(pending[0].targets.is_empty());

    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &tools,
        &HashSet::new(),
        false,
    )
    .unwrap();
    let assigned = managed_skill_projections(&store, &tools).unwrap();
    assert!(!assigned[0].pending_assignment);
    assert_eq!(assigned[0].assignment_issue_count, 0);
    assert_eq!(assigned[0].targets.len(), 1);

    let target = global.path().join("review");
    let metadata = std::fs::symlink_metadata(&target).unwrap();
    if metadata.file_type().is_symlink() {
        std::fs::remove_file(target).unwrap();
    } else {
        std::fs::remove_dir_all(target).unwrap();
    }
    let broken = managed_skill_projections(&store, &tools).unwrap();
    assert!(broken[0].pending_assignment);
    assert_eq!(broken[0].assignment_issue_count, 1);
}

#[test]
fn unassigned_skill_with_missing_source_exposes_a_skill_level_issue() {
    let (_db, store) = make_store();
    let central_root = tempfile::tempdir().expect("central root");
    let missing = central_root.path().join("missing");
    let skill = SkillRecord {
        id: "missing-skill".to_string(),
        name: "missing".to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: None,
        source_subpath: None,
        source_revision: None,
        central_path: missing.to_string_lossy().to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 1,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&skill).unwrap();

    let projection = managed_skill_projections(&store, &[]).unwrap().remove(0);

    assert_eq!(projection.source_issue.as_deref(), Some("source_missing"));
    assert_eq!(projection.assignment_issue_count, 1);
    assert!(projection.assignments.is_empty());
}

#[test]
fn unassigned_skill_with_invalid_status_exposes_a_skill_level_issue() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let mut skill = add_skill(&store, central.path());
    skill.status = "error".to_string();
    store.upsert_skill(&skill).unwrap();

    let projection = managed_skill_projections(&store, &[]).unwrap().remove(0);

    assert_eq!(projection.source_issue.as_deref(), Some("source_invalid"));
    assert_eq!(projection.assignment_issue_count, 1);
    assert!(projection.assignments.is_empty());
}

#[test]
fn assignment_issue_is_ignorable_only_with_another_valid_assignment() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let codex_dir = tempfile::tempdir().expect("codex global");
    let cursor_dir = tempfile::tempdir().expect("cursor global");
    let codex = tool(codex_dir.path());
    let mut cursor = AssignmentToolSpec {
        key: "cursor".to_string(),
        label: "Cursor".to_string(),
        global_skills_dir: cursor_dir.path().to_path_buf(),
        project_skills_dir: ".cursor/skills".to_string(),
        global_available: false,
        enabled: true,
        supports_project_scope: true,
    };
    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        std::slice::from_ref(&codex),
        &HashSet::new(),
        false,
    )
    .expect("apply valid assignment");
    store
        .upsert_skill_assignment(&SkillAssignmentRecord {
            id: "cursor-assignment".to_string(),
            skill_id: skill.id.clone(),
            tool: cursor.key.clone(),
            scope: "global".to_string(),
            project_id: None,
            ownership: "hub".to_string(),
            state: "error".to_string(),
            last_error: Some("tool is unavailable".to_string()),
            created_at: 1,
            updated_at: 1,
        })
        .expect("insert failed assignment");

    let projections = project_skill_assignments(&store, &skill, &[codex.clone(), cursor.clone()])
        .expect("project assignments");
    let cursor_projection = projections
        .iter()
        .find(|projection| projection.assignment.tool == "cursor")
        .expect("cursor projection");
    assert!(!cursor_projection.valid);
    assert!(cursor_projection.can_ignore);
    let first_fingerprint = cursor_projection.issue_fingerprint.clone().unwrap();
    ignore_assignment_issue(
        &store,
        &skill.id,
        cursor_projection.issue_key.as_deref().unwrap(),
        &first_fingerprint,
        &[codex.clone(), cursor.clone()],
    )
    .expect("ignore issue");
    let ignored = project_skill_assignments(&store, &skill, &[codex.clone(), cursor.clone()])
        .expect("ignored projection");
    assert!(
        ignored
            .iter()
            .find(|projection| projection.assignment.tool == "cursor")
            .unwrap()
            .ignored
    );

    cursor.global_available = true;
    let changed = project_skill_assignments(&store, &skill, &[codex.clone(), cursor.clone()])
        .expect("changed projection");
    let changed_cursor = changed
        .iter()
        .find(|projection| projection.assignment.tool == "cursor")
        .unwrap();
    assert_ne!(
        first_fingerprint,
        changed_cursor.issue_fingerprint.clone().unwrap()
    );
    assert!(!changed_cursor.ignored);

    write_skill(&cursor_dir.path().join("review"), "conflicting content");
    let conflict = project_skill_assignments(&store, &skill, &[codex.clone(), cursor.clone()])
        .expect("conflict projection");
    let conflict_cursor = conflict
        .iter()
        .find(|projection| projection.assignment.tool == "cursor")
        .unwrap();
    assert_eq!(
        conflict_cursor.issue_kind.as_deref(),
        Some("content_conflict")
    );
    assert!(!conflict_cursor.can_ignore);

    let codex_target = codex_dir.path().join("review");
    let metadata = std::fs::symlink_metadata(&codex_target).unwrap();
    if metadata.file_type().is_symlink() {
        std::fs::remove_file(codex_target).unwrap();
    } else {
        std::fs::remove_dir_all(codex_target).unwrap();
    }
    let none_valid = project_skill_assignments(&store, &skill, &[codex, cursor]).unwrap();
    assert!(none_valid.iter().all(|projection| !projection.can_ignore));
}

#[test]
fn disabled_tool_sharing_a_directory_is_not_silently_added() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let global = tempfile::tempdir().expect("global");
    let codex = tool(global.path());
    let disabled = AssignmentToolSpec {
        key: "disabled".to_string(),
        label: "Disabled".to_string(),
        global_skills_dir: global.path().to_path_buf(),
        project_skills_dir: ".disabled/skills".to_string(),
        global_available: false,
        enabled: false,
        supports_project_scope: true,
    };
    let preview = preview_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        &[codex, disabled],
        false,
    )
    .unwrap();
    assert_eq!(preview.len(), 1);
    assert_eq!(preview[0].tool, "codex");
}

#[test]
fn additive_assignment_does_not_retry_existing_targets() {
    let (_db, store) = make_store();
    let central = tempfile::tempdir().expect("central");
    write_skill(central.path(), "library");
    let skill = add_skill(&store, central.path());
    let codex_dir = tempfile::tempdir().expect("codex global");
    let cursor_dir = tempfile::tempdir().expect("cursor global");
    let codex = tool(codex_dir.path());
    let cursor = AssignmentToolSpec {
        key: "cursor".to_string(),
        label: "Cursor".to_string(),
        global_skills_dir: cursor_dir.path().to_path_buf(),
        project_skills_dir: ".cursor/skills".to_string(),
        global_available: true,
        enabled: true,
        supports_project_scope: true,
    };
    apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["codex".to_string()],
            projects: Vec::new(),
        },
        std::slice::from_ref(&codex),
        &HashSet::new(),
        false,
    )
    .unwrap();
    let codex_target = codex_dir.path().join("review");
    let metadata = std::fs::symlink_metadata(&codex_target).unwrap();
    if metadata.file_type().is_symlink() {
        std::fs::remove_file(&codex_target).unwrap();
    } else {
        std::fs::remove_dir_all(&codex_target).unwrap();
    }
    write_skill(&codex_target, "user replacement");

    let result = apply_skill_assignment(
        &store,
        &skill.id,
        &AssignmentDraft {
            global_tools: vec!["cursor".to_string()],
            projects: Vec::new(),
        },
        &[codex, cursor],
        &HashSet::new(),
        true,
    )
    .unwrap();

    assert_eq!(result.succeeded, 1);
    assert_eq!(result.failed, 0);
    assert_eq!(result.skipped, 0);
    assert_eq!(
        std::fs::read_to_string(codex_target.join("SKILL.md")).unwrap(),
        "user replacement"
    );
    let assignments = store.list_skill_assignments(&skill.id).unwrap();
    assert_eq!(assignments.len(), 2);
    assert!(assignments
        .iter()
        .all(|assignment| assignment.state == "active"));
}
