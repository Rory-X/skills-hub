use std::path::PathBuf;

use crate::core::skill_store::{
    IgnoredIssueRecord, ProjectRecord, ProjectSkillLinkRecord, ProjectSkillOriginRecord,
    SkillAssignmentRecord, SkillRecord, SkillStore, SkillTargetRecord, SkillTemplateRecord,
    SkillTemplateRevisionItemRecord, SkillTemplateRevisionRecord,
};
use rusqlite::Connection;

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let store = SkillStore::new(db);
    store.ensure_schema().expect("ensure_schema");
    (dir, store)
}

fn make_skill(id: &str, name: &str, central_path: &str, updated_at: i64) -> SkillRecord {
    SkillRecord {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: Some("/tmp/source".to_string()),
        source_subpath: None,
        source_revision: None,
        central_path: central_path.to_string(),
        content_hash: None,
        created_at: 1,
        updated_at,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    }
}

#[test]
fn schema_is_idempotent() {
    let (_dir, store) = make_store();
    store.ensure_schema().expect("ensure_schema again");
}

#[test]
fn deleting_project_cascades_project_records_without_deleting_skill() {
    let (_dir, store) = make_store();
    let skill = make_skill("skill", "Skill", "/central/skill", 1);
    store.upsert_skill(&skill).unwrap();
    let project = ProjectRecord {
        id: "project".to_string(),
        name: "Project".to_string(),
        root_path: "/projects/project".to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).unwrap();
    store
        .upsert_project_skill_link(&ProjectSkillLinkRecord {
            project_id: project.id.clone(),
            skill_id: skill.id.clone(),
            enabled: true,
            desired_tools: vec!["codex".to_string()],
            status: "ok".to_string(),
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    store
        .upsert_skill_assignment(&SkillAssignmentRecord {
            id: "assignment".to_string(),
            skill_id: skill.id.clone(),
            tool: "codex".to_string(),
            scope: "project".to_string(),
            project_id: Some(project.id.clone()),
            ownership: "hub".to_string(),
            state: "active".to_string(),
            last_error: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    store
        .upsert_ignored_issue(&IgnoredIssueRecord {
            id: "ignored".to_string(),
            scope: "project".to_string(),
            scope_id: Some(project.id.clone()),
            issue_key: "issue".to_string(),
            fingerprint: "fingerprint".to_string(),
            ignored_at: 1,
        })
        .unwrap();

    store.delete_project(&project.id).unwrap();

    assert!(store.get_project(&project.id).unwrap().is_none());
    assert!(store
        .list_project_skill_links(&project.id)
        .unwrap()
        .is_empty());
    assert!(store.list_skill_assignments(&skill.id).unwrap().is_empty());
    assert!(!store
        .is_issue_ignored("project", Some(&project.id), "issue", "fingerprint",)
        .unwrap());
    assert!(store.get_skill_by_id(&skill.id).unwrap().is_some());
}

#[test]
fn migrates_v9_project_links_into_manual_origins() {
    let (_dir, store) = make_store();
    let skill = make_skill("origin-skill", "Origin", "/central/origin", 1);
    store.upsert_skill(&skill).unwrap();
    let project = ProjectRecord {
        id: "origin-project".to_string(),
        name: "Origin project".to_string(),
        root_path: "/project/origin".to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).unwrap();
    store
        .upsert_project_skill_link(&ProjectSkillLinkRecord {
            project_id: project.id.clone(),
            skill_id: skill.id.clone(),
            enabled: true,
            desired_tools: vec!["codex".to_string()],
            status: "ok".to_string(),
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

    let conn = Connection::open(store.db_path()).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         DROP TABLE project_template_failures;
         DROP TABLE project_template_overrides;
         DROP TABLE project_skill_origins;
         DROP TABLE project_template_bindings;
         DROP TABLE skill_template_revision_items;
         DROP TABLE skill_template_revisions;
         DROP TABLE skill_template_items;
         DROP TABLE skill_templates;
         PRAGMA user_version = 9;",
    )
    .unwrap();
    drop(conn);

    store.ensure_schema().unwrap();
    assert_eq!(
        store.list_project_skill_origins(&project.id).unwrap(),
        vec![ProjectSkillOriginRecord {
            project_id: project.id,
            skill_id: skill.id,
            source_type: "manual".to_string(),
            source_id: String::new(),
            desired_tools: vec!["codex".to_string()],
            created_at: 1,
            updated_at: 1,
        }]
    );
}

#[test]
fn current_template_and_project_origins_block_skill_deletion() {
    let (_dir, store) = make_store();
    let skill = make_skill("protected", "Protected", "/central/protected", 1);
    store.upsert_skill(&skill).unwrap();
    let template = SkillTemplateRecord {
        id: "template".to_string(),
        name: "Template".to_string(),
        description: None,
        mode: "live".to_string(),
        archived: false,
        current_revision_id: "revision".to_string(),
        created_at: 1,
        updated_at: 1,
    };
    let revision = SkillTemplateRevisionRecord {
        id: "revision".to_string(),
        template_id: template.id.clone(),
        revision_number: 1,
        name: template.name.clone(),
        description: None,
        created_at: 1,
    };
    store
        .save_skill_template_revision(
            &template,
            &revision,
            &[SkillTemplateRevisionItemRecord {
                revision_id: revision.id.clone(),
                skill_id: skill.id.clone(),
                skill_name: skill.name.clone(),
            }],
        )
        .unwrap();
    assert!(store.delete_skill(&skill.id).is_err());
    assert_eq!(
        store
            .list_skill_template_names_for_skill(&skill.id)
            .unwrap(),
        vec!["Template"]
    );

    store.delete_skill_template(&template.id).unwrap();
    let project = ProjectRecord {
        id: "protected-project".to_string(),
        name: "Protected project".to_string(),
        root_path: "/project/protected".to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).unwrap();
    store
        .upsert_project_skill_origin(&ProjectSkillOriginRecord {
            project_id: project.id.clone(),
            skill_id: skill.id.clone(),
            source_type: "manual".to_string(),
            source_id: String::new(),
            desired_tools: vec!["codex".to_string()],
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
    assert!(store.delete_skill(&skill.id).is_err());
    assert_eq!(
        store
            .list_project_names_for_skill_origin(&skill.id)
            .unwrap(),
        vec!["Protected project"]
    );
}

#[test]
fn migrates_v3_targets_to_global_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("test.db");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TABLE skills (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          description TEXT NULL,
          source_type TEXT NOT NULL,
          source_ref TEXT NULL,
          source_subpath TEXT NULL,
          source_revision TEXT NULL,
          central_path TEXT NOT NULL UNIQUE,
          content_hash TEXT NULL,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          last_sync_at INTEGER NULL,
          last_seen_at INTEGER NOT NULL,
          status TEXT NOT NULL
        );
        CREATE TABLE skill_targets (
          id TEXT PRIMARY KEY,
          skill_id TEXT NOT NULL,
          tool TEXT NOT NULL,
          target_path TEXT NOT NULL,
          mode TEXT NOT NULL,
          status TEXT NOT NULL,
          last_error TEXT NULL,
          synced_at INTEGER NULL,
          UNIQUE(skill_id, tool),
          FOREIGN KEY(skill_id) REFERENCES skills(id) ON DELETE CASCADE
        );
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE discovered_skills (
          id TEXT PRIMARY KEY,
          tool TEXT NOT NULL,
          found_path TEXT NOT NULL,
          name_guess TEXT NULL,
          fingerprint TEXT NULL,
          found_at INTEGER NOT NULL,
          imported_skill_id TEXT NULL,
          FOREIGN KEY(imported_skill_id) REFERENCES skills(id) ON DELETE SET NULL
        );
        INSERT INTO skills (
          id, name, description, source_type, source_ref, source_subpath, source_revision,
          central_path, content_hash, created_at, updated_at, last_sync_at, last_seen_at, status
        ) VALUES (
          's1', 'S1', NULL, 'local', NULL, NULL, NULL,
          '/central/s1', NULL, 1, 2, NULL, 1, 'ok'
        );
        INSERT INTO skill_targets (
          id, skill_id, tool, target_path, mode, status, last_error, synced_at
        ) VALUES (
          't1', 's1', 'cursor', '/target/s1', 'copy', 'ok', NULL, 3
        );
        PRAGMA user_version = 3;",
    )
    .unwrap();
    drop(conn);

    let store = SkillStore::new(db);
    store.ensure_schema().unwrap();

    let target = store
        .get_skill_target("s1", "cursor", "global", None)
        .unwrap()
        .unwrap();
    assert_eq!(target.target_path, "/target/s1");
    assert_eq!(target.scope, "global");
    assert!(target.project_path.is_none());
}

#[test]
fn settings_roundtrip_and_update() {
    let (_dir, store) = make_store();

    assert_eq!(store.get_setting("missing").unwrap(), None);
    store.set_setting("k", "v1").unwrap();
    assert_eq!(store.get_setting("k").unwrap().as_deref(), Some("v1"));
    store.set_setting("k", "v2").unwrap();
    assert_eq!(store.get_setting("k").unwrap().as_deref(), Some("v2"));

    store.set_onboarding_completed(true).unwrap();
    assert_eq!(
        store
            .get_setting("onboarding_completed")
            .unwrap()
            .as_deref(),
        Some("true")
    );
    store.set_onboarding_completed(false).unwrap();
    assert_eq!(
        store
            .get_setting("onboarding_completed")
            .unwrap()
            .as_deref(),
        Some("false")
    );
}

#[test]
fn skills_upsert_list_get_delete() {
    let (_dir, store) = make_store();

    let a = make_skill("a", "A", "/central/a", 10);
    let b = make_skill("b", "B", "/central/b", 20);
    store.upsert_skill(&a).unwrap();
    store.upsert_skill(&b).unwrap();

    let listed = store.list_skills().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, "b");
    assert_eq!(listed[1].id, "a");

    let got = store.get_skill_by_id("a").unwrap().unwrap();
    assert_eq!(got.name, "A");

    let mut a2 = a.clone();
    a2.name = "A2".to_string();
    a2.updated_at = 30;
    store.upsert_skill(&a2).unwrap();
    assert_eq!(store.get_skill_by_id("a").unwrap().unwrap().name, "A2");
    assert_eq!(store.list_skills().unwrap()[0].id, "a");

    store.delete_skill("a").unwrap();
    assert!(store.get_skill_by_id("a").unwrap().is_none());
}

#[test]
fn skill_targets_upsert_unique_constraint_and_list_order() {
    let (_dir, store) = make_store();
    let skill = make_skill("s1", "S1", "/central/s1", 1);
    store.upsert_skill(&skill).unwrap();

    let t1 = SkillTargetRecord {
        id: "t1".to_string(),
        skill_id: "s1".to_string(),
        tool: "cursor".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: "/target/1".to_string(),
        mode: "copy".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: None,
    };
    store.upsert_skill_target(&t1).unwrap();
    assert_eq!(
        store
            .get_skill_target("s1", "cursor", "global", None)
            .unwrap()
            .unwrap()
            .target_path,
        "/target/1"
    );

    let mut t1b = t1.clone();
    t1b.id = "t2".to_string();
    t1b.target_path = "/target/2".to_string();
    store.upsert_skill_target(&t1b).unwrap();
    assert_eq!(
        store
            .get_skill_target("s1", "cursor", "global", None)
            .unwrap()
            .unwrap()
            .id,
        "t1",
        "unique(skill_id, tool) 冲突时应更新现有行而不是替换 id"
    );
    assert_eq!(
        store
            .get_skill_target("s1", "cursor", "global", None)
            .unwrap()
            .unwrap()
            .target_path,
        "/target/2"
    );

    let t2 = SkillTargetRecord {
        id: "t3".to_string(),
        skill_id: "s1".to_string(),
        tool: "claude_code".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: "/target/cc".to_string(),
        mode: "copy".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: None,
    };
    store.upsert_skill_target(&t2).unwrap();

    let targets = store.list_skill_targets("s1").unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].tool, "claude_code");
    assert_eq!(targets[1].tool, "cursor");

    store
        .delete_skill_target("s1", "cursor", "global", None)
        .unwrap();
    assert!(store
        .get_skill_target("s1", "cursor", "global", None)
        .unwrap()
        .is_none());
}

#[test]
fn project_targets_coexist_by_project_path_and_delete_precisely() {
    let (_dir, store) = make_store();
    let skill = make_skill("s1", "S1", "/central/s1", 1);
    store.upsert_skill(&skill).unwrap();

    let global = SkillTargetRecord {
        id: "global".to_string(),
        skill_id: "s1".to_string(),
        tool: "cursor".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: "/global/cursor/s1".to_string(),
        mode: "copy".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: Some(1),
    };
    let project_a = SkillTargetRecord {
        id: "project-a".to_string(),
        skill_id: "s1".to_string(),
        tool: "cursor".to_string(),
        scope: "project".to_string(),
        project_path: Some("/projects/a".to_string()),
        target_path: "/projects/a/.agents/skills/s1".to_string(),
        mode: "symlink".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: Some(2),
    };
    let project_b = SkillTargetRecord {
        id: "project-b".to_string(),
        skill_id: "s1".to_string(),
        tool: "cursor".to_string(),
        scope: "project".to_string(),
        project_path: Some("/projects/b".to_string()),
        target_path: "/projects/b/.agents/skills/s1".to_string(),
        mode: "symlink".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: Some(3),
    };

    store.upsert_skill_target(&global).unwrap();
    store.upsert_skill_target(&project_a).unwrap();
    store.upsert_skill_target(&project_b).unwrap();

    assert_eq!(store.list_skill_targets("s1").unwrap().len(), 3);
    assert_eq!(
        store
            .get_skill_target("s1", "cursor", "global", None)
            .unwrap()
            .unwrap()
            .target_path,
        "/global/cursor/s1"
    );
    assert_eq!(
        store
            .get_skill_target("s1", "cursor", "project", Some("/projects/a"))
            .unwrap()
            .unwrap()
            .target_path,
        "/projects/a/.agents/skills/s1"
    );
    assert_eq!(
        store
            .get_skill_target("s1", "cursor", "project", Some("/projects/b"))
            .unwrap()
            .unwrap()
            .target_path,
        "/projects/b/.agents/skills/s1"
    );

    let mut updated_project_a = project_a.clone();
    updated_project_a.id = "project-a-new-id".to_string();
    updated_project_a.target_path = "/projects/a/.agents/skills/s1-updated".to_string();
    store.upsert_skill_target(&updated_project_a).unwrap();

    let got_project_a = store
        .get_skill_target("s1", "cursor", "project", Some("/projects/a"))
        .unwrap()
        .unwrap();
    assert_eq!(got_project_a.id, "project-a");
    assert_eq!(
        got_project_a.target_path,
        "/projects/a/.agents/skills/s1-updated"
    );
    assert_eq!(store.list_skill_targets("s1").unwrap().len(), 3);

    store
        .delete_skill_target("s1", "cursor", "project", Some("/projects/a"))
        .unwrap();

    assert!(store
        .get_skill_target("s1", "cursor", "project", Some("/projects/a"))
        .unwrap()
        .is_none());
    assert!(store
        .get_skill_target("s1", "cursor", "project", Some("/projects/b"))
        .unwrap()
        .is_some());
    assert!(store
        .get_skill_target("s1", "cursor", "global", None)
        .unwrap()
        .is_some());
}

#[test]
fn deleting_skill_cascades_targets() {
    let (_dir, store) = make_store();
    let skill = make_skill("s1", "S1", "/central/s1", 1);
    store.upsert_skill(&skill).unwrap();

    let t = SkillTargetRecord {
        id: "t1".to_string(),
        skill_id: "s1".to_string(),
        tool: "cursor".to_string(),
        scope: "global".to_string(),
        project_path: None,
        target_path: "/target/1".to_string(),
        mode: "copy".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: None,
    };
    store.upsert_skill_target(&t).unwrap();
    assert_eq!(store.list_skill_targets("s1").unwrap().len(), 1);

    store.delete_skill("s1").unwrap();
    assert_eq!(store.list_skill_targets("s1").unwrap().len(), 0);
}

#[test]
fn skill_assignments_upsert_update_and_delete_precisely() {
    let (_dir, store) = make_store();
    store
        .upsert_skill(&make_skill("s1", "S1", "/central/s1", 1))
        .unwrap();

    let global = SkillAssignmentRecord {
        id: "a-global".to_string(),
        skill_id: "s1".to_string(),
        tool: "codex".to_string(),
        scope: "global".to_string(),
        project_id: None,
        ownership: "hub".to_string(),
        state: "active".to_string(),
        last_error: None,
        created_at: 1,
        updated_at: 1,
    };
    store.upsert_skill_assignment(&global).unwrap();

    let mut updated = global.clone();
    updated.id = "replacement-id".to_string();
    updated.state = "error".to_string();
    updated.last_error = Some("disk full".to_string());
    updated.updated_at = 2;
    store.upsert_skill_assignment(&updated).unwrap();

    let stored = store
        .get_skill_assignment("s1", "codex", "global", None)
        .unwrap()
        .unwrap();
    assert_eq!(stored.id, "a-global");
    assert_eq!(stored.state, "error");
    assert_eq!(stored.last_error.as_deref(), Some("disk full"));

    let project = ProjectRecord {
        id: "p1".to_string(),
        name: "Project".to_string(),
        root_path: "/projects/one".to_string(),
        enabled_tool_keys: vec!["codex".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 1,
        last_scanned_at: None,
    };
    store.insert_project(&project).unwrap();
    store
        .upsert_skill_assignment(&SkillAssignmentRecord {
            id: "a-project".to_string(),
            skill_id: "s1".to_string(),
            tool: "codex".to_string(),
            scope: "project".to_string(),
            project_id: Some(project.id.clone()),
            ownership: "native".to_string(),
            state: "active".to_string(),
            last_error: None,
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();

    assert_eq!(store.list_skill_assignments("s1").unwrap().len(), 2);
    store
        .delete_skill_assignment("s1", "codex", "project", Some("p1"))
        .unwrap();
    assert_eq!(store.list_skill_assignments("s1").unwrap(), vec![stored]);
}

#[test]
fn migrates_v7_targets_and_project_links_into_assignments() {
    let (dir, store) = make_store();
    let skill = make_skill("s1", "S1", "/central/s1", 1);
    store.upsert_skill(&skill).unwrap();
    let project = ProjectRecord {
        id: "p1".to_string(),
        name: "Project".to_string(),
        root_path: "/projects/one".to_string(),
        enabled_tool_keys: vec!["codex".to_string(), "cursor".to_string()],
        local_git_exclude_enabled: false,
        created_at: 1,
        updated_at: 2,
        last_scanned_at: None,
    };
    store.insert_project(&project).unwrap();
    store
        .upsert_skill_target(&SkillTargetRecord {
            id: "target-global".to_string(),
            skill_id: skill.id.clone(),
            tool: "codex".to_string(),
            scope: "global".to_string(),
            project_path: None,
            target_path: "/target/s1".to_string(),
            mode: "copy".to_string(),
            status: "ok".to_string(),
            last_error: None,
            synced_at: Some(3),
        })
        .unwrap();
    store
        .upsert_project_skill_link(&ProjectSkillLinkRecord {
            project_id: project.id.clone(),
            skill_id: skill.id.clone(),
            enabled: true,
            desired_tools: vec!["codex".to_string(), "cursor".to_string()],
            status: "ok".to_string(),
            created_at: 1,
            updated_at: 2,
        })
        .unwrap();

    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    conn.execute_batch(
        "DROP TABLE skill_assignments;
         DROP TABLE ignored_issues;
         PRAGMA user_version = 7;",
    )
    .unwrap();
    drop(conn);

    store.ensure_schema().unwrap();
    let assignments = store.list_skill_assignments("s1").unwrap();
    assert_eq!(assignments.len(), 3);
    assert!(assignments
        .iter()
        .any(|item| { item.scope == "global" && item.tool == "codex" && item.ownership == "hub" }));
    assert!(assignments.iter().any(|item| {
        item.scope == "project"
            && item.tool == "codex"
            && item.project_id.as_deref() == Some("p1")
            && item.ownership == "native"
    }));
    assert!(assignments.iter().any(|item| {
        item.scope == "project"
            && item.tool == "cursor"
            && item.project_id.as_deref() == Some("p1")
            && item.ownership == "native"
    }));
}

#[test]
fn ignored_issues_match_only_the_same_problem_instance() {
    let (_dir, store) = make_store();
    let ignored = IgnoredIssueRecord {
        id: "ignored-1".to_string(),
        scope: "project".to_string(),
        scope_id: Some("p1".to_string()),
        issue_key: "target_missing:s1:codex".to_string(),
        fingerprint: "fingerprint-a".to_string(),
        ignored_at: 10,
    };
    store.upsert_ignored_issue(&ignored).unwrap();

    assert!(store
        .is_issue_ignored(
            "project",
            Some("p1"),
            "target_missing:s1:codex",
            "fingerprint-a",
        )
        .unwrap());
    assert!(!store
        .is_issue_ignored(
            "project",
            Some("p1"),
            "target_missing:s1:codex",
            "fingerprint-b",
        )
        .unwrap());
    assert!(!store
        .is_issue_ignored(
            "project",
            Some("p2"),
            "target_missing:s1:codex",
            "fingerprint-a",
        )
        .unwrap());
}

#[test]
fn tags_can_be_created_renamed_linked_and_deleted() {
    let (_dir, store) = make_store();
    let skill = make_skill("s1", "S1", "/central/s1", 1);
    store.upsert_skill(&skill).unwrap();

    let frontend = store.create_tag(" Frontend ").unwrap();
    assert_eq!(frontend.name, "Frontend");
    assert!(store.create_tag("frontend").is_err());

    let docs = store.create_tag("Docs").unwrap();
    store.set_skill_tags("s1", &[frontend.id, docs.id]).unwrap();
    store
        .set_skill_tags("s1", &[frontend.id, frontend.id, docs.id])
        .unwrap();

    let linked = store.get_skill_tags("s1").unwrap();
    assert_eq!(linked.len(), 2);
    assert_eq!(linked[0].name, "Docs");
    assert_eq!(linked[1].name, "Frontend");

    let renamed = store.rename_tag(frontend.id, "UI").unwrap();
    assert_eq!(renamed.name, "UI");
    assert!(store.rename_tag(renamed.id, "docs").is_err());

    let tags = store.list_tags_with_counts().unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].name, "Docs");
    assert_eq!(tags[0].skill_count, 1);
    assert_eq!(tags[1].name, "UI");
    assert_eq!(tags[1].skill_count, 1);

    store.delete_tag(docs.id).unwrap();
    let linked = store.get_skill_tags("s1").unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].name, "UI");
}

#[test]
fn tag_links_are_removed_when_skill_is_deleted_and_untagged_is_counted() {
    let (_dir, store) = make_store();
    let tagged = make_skill("tagged", "Tagged", "/central/tagged", 2);
    let untagged = make_skill("untagged", "Untagged", "/central/untagged", 1);
    store.upsert_skill(&tagged).unwrap();
    store.upsert_skill(&untagged).unwrap();

    let tag = store.create_tag("Frontend").unwrap();
    store.set_skill_tags("tagged", &[tag.id]).unwrap();

    assert_eq!(store.list_untagged_skill_ids().unwrap(), vec!["untagged"]);

    store.delete_skill("tagged").unwrap();
    assert!(store.get_skill_tags("tagged").unwrap().is_empty());
    assert_eq!(store.list_tags_with_counts().unwrap()[0].skill_count, 0);
}

#[test]
fn description_stored_and_retrieved() {
    let (_dir, store) = make_store();
    let mut skill = make_skill("d1", "D1", "/central/d1", 1);
    skill.description = Some("A test skill description".to_string());
    store.upsert_skill(&skill).unwrap();

    let got = store.get_skill_by_id("d1").unwrap().unwrap();
    assert_eq!(got.description.as_deref(), Some("A test skill description"));
}

#[test]
fn description_null_by_default() {
    let (_dir, store) = make_store();
    let skill = make_skill("d2", "D2", "/central/d2", 1);
    store.upsert_skill(&skill).unwrap();

    let got = store.get_skill_by_id("d2").unwrap().unwrap();
    assert!(got.description.is_none());
}

#[test]
fn update_skill_description_backfills() {
    let (_dir, store) = make_store();
    let skill = make_skill("d3", "D3", "/central/d3", 1);
    store.upsert_skill(&skill).unwrap();

    assert!(store
        .get_skill_by_id("d3")
        .unwrap()
        .unwrap()
        .description
        .is_none());

    store
        .update_skill_description("d3", Some("backfilled"))
        .unwrap();
    assert_eq!(
        store
            .get_skill_by_id("d3")
            .unwrap()
            .unwrap()
            .description
            .as_deref(),
        Some("backfilled")
    );
}

#[test]
fn error_context_includes_db_path() {
    let store = SkillStore::new(PathBuf::from("/this/path/should/not/exist/test.db"));
    let err = store.ensure_schema().unwrap_err();
    let msg = format!("{:#}", err);
    assert!(msg.contains("failed to open db at"), "{msg}");
}
