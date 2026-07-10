use std::collections::HashSet;
use std::path::Path;

use crate::core::skill_store::{ProjectRecord, SkillAssignmentRecord, SkillRecord, SkillStore};

use super::{
    apply_skill_assignment, ignore_assignment_issue, managed_skill_projections,
    preview_skill_assignment, project_skill_assignments, AssignmentConflictKey, AssignmentDraft,
    AssignmentToolSpec, ProjectAssignmentDraft,
};

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
