use std::path::Path;

use crate::core::projects::{create_or_get_project, ProjectToolSpec};
use crate::core::skill_store::{SkillRecord, SkillStore};

use super::*;

fn make_store() -> (tempfile::TempDir, SkillStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SkillStore::new(dir.path().join("test.db"));
    store.ensure_schema().expect("schema");
    (dir, store)
}

fn add_skill(store: &SkillStore, root: &Path, id: &str, name: &str) -> SkillRecord {
    let central = root.join(id);
    std::fs::create_dir_all(&central).unwrap();
    std::fs::write(
        central.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: test\n---\n"),
    )
    .unwrap();
    let skill = SkillRecord {
        id: id.to_string(),
        name: name.to_string(),
        description: Some("test".to_string()),
        source_type: "local".to_string(),
        source_ref: None,
        source_subpath: None,
        source_revision: None,
        central_path: central.to_string_lossy().to_string(),
        content_hash: None,
        created_at: 1,
        updated_at: 1,
        last_sync_at: None,
        last_seen_at: 1,
        enabled: true,
        status: "ok".to_string(),
    };
    store.upsert_skill(&skill).unwrap();
    skill
}

fn template_id(result: &TemplateMutationResult) -> String {
    result
        .template
        .as_ref()
        .expect("template")
        .template
        .id
        .clone()
}

#[test]
fn template_names_are_case_insensitively_unique_and_mode_is_immutable() {
    let (dir, store) = make_store();
    let skill = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let created = create_skill_template(
        &store,
        "Review",
        Some("Review flow"),
        "live",
        std::slice::from_ref(&skill.id),
    )
    .unwrap();
    let template = created.template.unwrap().template;

    let duplicate = create_skill_template(
        &store,
        "review",
        None,
        "snapshot",
        std::slice::from_ref(&skill.id),
    )
    .unwrap_err();
    assert!(duplicate.to_string().contains("already exists"));

    let mut changed_mode = template.clone();
    changed_mode.mode = "snapshot".to_string();
    changed_mode.current_revision_id = "revision-2".to_string();
    let revision = SkillTemplateRevisionRecord {
        id: "revision-2".to_string(),
        template_id: template.id,
        revision_number: 2,
        name: template.name,
        description: template.description,
        created_at: 2,
    };
    let error = store
        .save_skill_template_revision(
            &changed_mode,
            &revision,
            &[SkillTemplateRevisionItemRecord {
                revision_id: revision.id.clone(),
                skill_id: skill.id,
                skill_name: skill.name,
            }],
        )
        .unwrap_err();
    assert!(error.to_string().contains("mode cannot be changed"));
}

#[test]
fn every_save_creates_a_revision_and_historical_items_do_not_block_skill_deletion() {
    let (dir, store) = make_store();
    let first = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let second = add_skill(&store, dir.path(), "skill-b", "Beta");
    let created = create_skill_template(
        &store,
        "Workflow",
        None,
        "snapshot",
        std::slice::from_ref(&first.id),
    )
    .unwrap();
    let id = template_id(&created);

    update_skill_template(
        &store,
        &id,
        "Workflow",
        Some("updated"),
        std::slice::from_ref(&second.id),
        &[],
    )
    .unwrap();
    update_skill_template(
        &store,
        &id,
        "Workflow renamed",
        Some("metadata only"),
        std::slice::from_ref(&second.id),
        &[],
    )
    .unwrap();

    assert_eq!(store.list_skill_template_revisions(&id).unwrap().len(), 3);
    store.delete_skill(&first.id).unwrap();
    assert!(store.get_skill_by_id(&first.id).unwrap().is_none());
    assert_eq!(
        store.list_skill_template_item_ids(&id).unwrap(),
        vec![second.id]
    );
}

#[test]
fn live_projects_follow_membership_while_snapshot_projects_keep_the_applied_revision() {
    let (dir, store) = make_store();
    let first = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let second = add_skill(&store, dir.path(), "skill-b", "Beta");
    let live_project_dir = tempfile::tempdir().unwrap();
    let snapshot_project_dir = tempfile::tempdir().unwrap();
    let live_project = create_or_get_project(&store, live_project_dir.path(), &[]).unwrap();
    let snapshot_project = create_or_get_project(&store, snapshot_project_dir.path(), &[]).unwrap();
    let live = create_skill_template(
        &store,
        "Live",
        None,
        "live",
        std::slice::from_ref(&first.id),
    )
    .unwrap();
    let snapshot = create_skill_template(
        &store,
        "Snapshot",
        None,
        "snapshot",
        std::slice::from_ref(&first.id),
    )
    .unwrap();
    let live_id = template_id(&live);
    let snapshot_id = template_id(&snapshot);
    apply_project_template(&store, &live_project.id, &live_id, &[]).unwrap();
    apply_project_template(&store, &snapshot_project.id, &snapshot_id, &[]).unwrap();

    update_skill_template(
        &store,
        &live_id,
        "Live",
        None,
        std::slice::from_ref(&second.id),
        &[],
    )
    .unwrap();
    update_skill_template(
        &store,
        &snapshot_id,
        "Snapshot",
        None,
        std::slice::from_ref(&second.id),
        &[],
    )
    .unwrap();

    let live_origins = store.list_project_skill_origins(&live_project.id).unwrap();
    assert_eq!(live_origins.len(), 1);
    assert_eq!(live_origins[0].skill_id, second.id);
    let snapshot_origins = store
        .list_project_skill_origins(&snapshot_project.id)
        .unwrap();
    assert_eq!(snapshot_origins.len(), 1);
    assert_eq!(snapshot_origins[0].skill_id, first.id);

    update_snapshot_project_template(&store, &snapshot_project.id, &[]).unwrap();
    let snapshot_origins = store
        .list_project_skill_origins(&snapshot_project.id)
        .unwrap();
    assert_eq!(snapshot_origins.len(), 1);
    assert_eq!(snapshot_origins[0].skill_id, second.id);
}

#[test]
fn detaching_can_retain_template_skills_as_manual_or_remove_them() {
    let (dir, store) = make_store();
    let skill = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let project_dir = tempfile::tempdir().unwrap();
    let project = create_or_get_project(&store, project_dir.path(), &[]).unwrap();
    let template = create_skill_template(
        &store,
        "Workflow",
        None,
        "live",
        std::slice::from_ref(&skill.id),
    )
    .unwrap();
    let template_id = template_id(&template);
    apply_project_template(&store, &project.id, &template_id, &[]).unwrap();

    detach_project_template(&store, &project.id, true, &[]).unwrap();
    let origins = store.list_project_skill_origins(&project.id).unwrap();
    assert_eq!(origins.len(), 1);
    assert_eq!(origins[0].source_type, "manual");
    assert!(store
        .get_project_template_binding(&project.id)
        .unwrap()
        .is_none());

    apply_project_template(&store, &project.id, &template_id, &[]).unwrap();
    detach_project_template(&store, &project.id, false, &[]).unwrap();
    let origins = store.list_project_skill_origins(&project.id).unwrap();
    assert_eq!(origins.len(), 1, "the pre-existing manual source remains");
}

#[test]
fn one_conflicting_skill_does_not_block_other_template_skills() {
    let (dir, store) = make_store();
    let first = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let second = add_skill(&store, dir.path(), "skill-b", "Beta");
    let project_dir = tempfile::tempdir().unwrap();
    let project =
        create_or_get_project(&store, project_dir.path(), &["codex".to_string()]).unwrap();
    let conflict = project_dir.path().join(".agents/skills/Alpha");
    std::fs::create_dir_all(&conflict).unwrap();
    std::fs::write(
        conflict.join("SKILL.md"),
        "---\nname: Alpha\ndescription: project\n---\ndifferent",
    )
    .unwrap();
    let template = create_skill_template(
        &store,
        "Workflow",
        None,
        "live",
        &[first.id.clone(), second.id.clone()],
    )
    .unwrap();
    let result = apply_project_template(
        &store,
        &project.id,
        &template_id(&template),
        &[ProjectToolSpec::new("codex", ".agents/skills")],
    )
    .unwrap();

    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].skill_id, first.id);
    assert!(project_dir.path().join(".agents/skills/Beta").exists());
}

#[test]
fn disabling_a_conflicting_template_skill_clears_its_sync_failure() {
    let (dir, store) = make_store();
    let skill = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let project_dir = tempfile::tempdir().unwrap();
    let project =
        create_or_get_project(&store, project_dir.path(), &["codex".to_string()]).unwrap();
    let conflict = project_dir.path().join(".agents/skills/Alpha");
    std::fs::create_dir_all(&conflict).unwrap();
    std::fs::write(
        conflict.join("SKILL.md"),
        "---\nname: Alpha\ndescription: project\n---\ndifferent",
    )
    .unwrap();
    let template = create_skill_template(
        &store,
        "Workflow",
        None,
        "live",
        std::slice::from_ref(&skill.id),
    )
    .unwrap();
    let template_id = template_id(&template);
    let specs = [ProjectToolSpec::new("codex", ".agents/skills")];
    let failed = apply_project_template(&store, &project.id, &template_id, &specs).unwrap();
    assert_eq!(failed.failed.len(), 1);
    assert!(store
        .get_project_template_binding(&project.id)
        .unwrap()
        .unwrap()
        .sync_error
        .is_some());

    store.set_skill_enabled(&skill.id, false).unwrap();
    let reconciled = reconcile_project_template(&store, &project.id, &specs).unwrap();

    assert!(reconciled.failed.is_empty());
    assert!(store
        .get_project_template_binding(&project.id)
        .unwrap()
        .unwrap()
        .sync_error
        .is_none());
    assert!(store
        .list_project_template_failures(&project.id)
        .unwrap()
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(conflict.join("SKILL.md")).unwrap(),
        "---\nname: Alpha\ndescription: project\n---\ndifferent"
    );
}

#[test]
fn resolving_one_template_conflict_keeps_other_failures_visible() {
    let (dir, store) = make_store();
    let first = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let second = add_skill(&store, dir.path(), "skill-b", "Beta");
    let project_dir = tempfile::tempdir().unwrap();
    let project =
        create_or_get_project(&store, project_dir.path(), &["codex".to_string()]).unwrap();
    for name in [&first.name, &second.name] {
        let conflict = project_dir.path().join(".agents/skills").join(name);
        std::fs::create_dir_all(&conflict).unwrap();
        std::fs::write(
            conflict.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: project\n---\ndifferent"),
        )
        .unwrap();
    }
    let template = create_skill_template(
        &store,
        "Workflow",
        None,
        "live",
        &[first.id.clone(), second.id.clone()],
    )
    .unwrap();
    let specs = [ProjectToolSpec::new("codex", ".agents/skills")];
    let applied =
        apply_project_template(&store, &project.id, &template_id(&template), &specs).unwrap();
    assert_eq!(applied.failed.len(), 2);

    let resolved =
        resolve_project_template_conflict(&store, &project.id, &first.id, false, &specs).unwrap();

    assert_eq!(resolved.failed.len(), 1);
    assert_eq!(resolved.failed[0].skill_id, second.id);
    let persisted = store.list_project_template_failures(&project.id).unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].skill_id, second.id);
    assert!(store
        .get_project_template_binding(&project.id)
        .unwrap()
        .unwrap()
        .sync_error
        .is_some());
}

#[test]
fn switching_templates_clears_old_origins_and_failures_but_keeps_manual_sources() {
    let (dir, store) = make_store();
    let first = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let second = add_skill(&store, dir.path(), "skill-b", "Beta");
    let project_dir = tempfile::tempdir().unwrap();
    let project =
        create_or_get_project(&store, project_dir.path(), &["codex".to_string()]).unwrap();
    let conflict = project_dir.path().join(".agents/skills/Alpha");
    std::fs::create_dir_all(&conflict).unwrap();
    std::fs::write(
        conflict.join("SKILL.md"),
        "---\nname: Alpha\ndescription: project\n---\ndifferent",
    )
    .unwrap();
    super::super::projects::set_manual_project_skill_origin(
        &store,
        &project,
        &first.id,
        &["codex".to_string()],
    )
    .unwrap();
    let first_template = create_skill_template(
        &store,
        "First",
        None,
        "live",
        std::slice::from_ref(&first.id),
    )
    .unwrap();
    let second_template = create_skill_template(
        &store,
        "Second",
        None,
        "live",
        std::slice::from_ref(&second.id),
    )
    .unwrap();
    let first_template_id = template_id(&first_template);
    let second_template_id = template_id(&second_template);
    let specs = [ProjectToolSpec::new("codex", ".agents/skills")];
    let failed = apply_project_template(&store, &project.id, &first_template_id, &specs).unwrap();
    assert_eq!(failed.failed.len(), 1);

    let switched =
        apply_project_template(&store, &project.id, &second_template_id, &specs).unwrap();

    assert!(switched.failed.is_empty());
    assert!(store
        .list_project_template_failures(&project.id)
        .unwrap()
        .is_empty());
    let binding = store
        .get_project_template_binding(&project.id)
        .unwrap()
        .unwrap();
    assert_eq!(binding.template_id, second_template_id);
    let origins = store.list_project_skill_origins(&project.id).unwrap();
    assert!(origins
        .iter()
        .any(|origin| { origin.skill_id == first.id && origin.source_type == "manual" }));
    assert!(!origins
        .iter()
        .any(|origin| { origin.skill_id == first.id && origin.source_type == "template" }));
    assert!(origins.iter().any(|origin| {
        origin.skill_id == second.id
            && origin.source_type == "template"
            && origin.source_id == second_template_id
    }));
}

#[test]
fn archived_templates_serve_existing_projects_but_cannot_be_reapplied_or_deleted() {
    let (dir, store) = make_store();
    let skill = add_skill(&store, dir.path(), "skill-a", "Alpha");
    let first_project_dir = tempfile::tempdir().unwrap();
    let second_project_dir = tempfile::tempdir().unwrap();
    let first_project = create_or_get_project(&store, first_project_dir.path(), &[]).unwrap();
    let second_project = create_or_get_project(&store, second_project_dir.path(), &[]).unwrap();
    let template = create_skill_template(
        &store,
        "Workflow",
        None,
        "live",
        std::slice::from_ref(&skill.id),
    )
    .unwrap();
    let template_id = template_id(&template);
    apply_project_template(&store, &first_project.id, &template_id, &[]).unwrap();

    let archived = set_skill_template_archived(&store, &template_id, true).unwrap();
    assert!(archived.template.archived);
    reconcile_project_template(&store, &first_project.id, &[]).unwrap();
    assert!(apply_project_template(&store, &second_project.id, &template_id, &[]).is_err());
    assert!(delete_skill_template(&store, &template_id).is_err());

    detach_project_template(&store, &first_project.id, false, &[]).unwrap();
    delete_skill_template(&store, &template_id).unwrap();
    assert!(store.get_skill_template(&template_id).unwrap().is_none());
}
