use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use uuid::Uuid;

use super::projects::{
    effective_project_skill_tools, materialize_project_skill_tools_and_scan, remove_project_skill,
    scan_project, ProjectToolSpec,
};
use super::skill_store::{
    ProjectSkillLinkRecord, ProjectSkillOriginRecord, ProjectTemplateBindingRecord,
    ProjectTemplateFailureRecord, ProjectTemplateOverrideRecord, SkillAssignmentRecord,
    SkillRecord, SkillStore, SkillTemplateRecord, SkillTemplateRevisionItemRecord,
    SkillTemplateRevisionRecord,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateSkill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateProjectLink {
    pub project_id: String,
    pub project_name: String,
    pub project_root: String,
    pub applied_revision_id: String,
    pub snapshot_outdated: bool,
    pub sync_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillTemplateView {
    pub template: SkillTemplateRecord,
    pub skills: Vec<TemplateSkill>,
    pub linked_projects: Vec<TemplateProjectLink>,
    pub failure_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillTemplateRevisionView {
    pub revision: SkillTemplateRevisionRecord,
    pub skills: Vec<SkillTemplateRevisionItemRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateDiff {
    pub added: Vec<TemplateSkill>,
    pub removed: Vec<TemplateSkill>,
    pub retained: Vec<TemplateSkill>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateReconcileResult {
    pub project_id: String,
    pub template_id: String,
    pub applied_revision_id: String,
    pub succeeded: usize,
    pub failed: Vec<ProjectTemplateFailureRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateMutationResult {
    pub template: Option<SkillTemplateView>,
    pub membership_changed: bool,
    pub reconciled_projects: usize,
    pub failed_projects: usize,
}

pub fn list_skill_templates(
    store: &SkillStore,
    include_archived: bool,
) -> Result<Vec<SkillTemplateView>> {
    store
        .list_skill_templates(include_archived)?
        .into_iter()
        .map(|template| build_template_view(store, template))
        .collect()
}

pub fn get_skill_template(store: &SkillStore, template_id: &str) -> Result<SkillTemplateView> {
    let template = require_template(store, template_id)?;
    build_template_view(store, template)
}

pub fn list_skill_template_revisions(
    store: &SkillStore,
    template_id: &str,
) -> Result<Vec<SkillTemplateRevisionView>> {
    require_template(store, template_id)?;
    store
        .list_skill_template_revisions(template_id)?
        .into_iter()
        .map(|revision| {
            let skills = store.list_skill_template_revision_items(&revision.id)?;
            Ok(SkillTemplateRevisionView { revision, skills })
        })
        .collect()
}

pub fn create_skill_template(
    store: &SkillStore,
    name: &str,
    description: Option<&str>,
    mode: &str,
    skill_ids: &[String],
) -> Result<TemplateMutationResult> {
    let name = normalize_template_name(name)?;
    let description = normalize_description(description);
    validate_template_mode(mode)?;
    if store.get_skill_template_by_name(&name)?.is_some() {
        anyhow::bail!("skill template name already exists: {name}");
    }
    let skills = validate_template_skills(store, skill_ids, &HashSet::new())?;
    let now = now_ms();
    let template_id = Uuid::new_v4().to_string();
    let revision_id = Uuid::new_v4().to_string();
    let template = SkillTemplateRecord {
        id: template_id.clone(),
        name: name.clone(),
        description: description.clone(),
        mode: mode.to_string(),
        archived: false,
        current_revision_id: revision_id.clone(),
        created_at: now,
        updated_at: now,
    };
    let revision = SkillTemplateRevisionRecord {
        id: revision_id.clone(),
        template_id,
        revision_number: 1,
        name,
        description,
        created_at: now,
    };
    let items = revision_items(&revision_id, &skills);
    store.save_skill_template_revision(&template, &revision, &items)?;
    Ok(TemplateMutationResult {
        template: Some(build_template_view(store, template)?),
        membership_changed: true,
        ..TemplateMutationResult::default()
    })
}

pub fn update_skill_template(
    store: &SkillStore,
    template_id: &str,
    name: &str,
    description: Option<&str>,
    skill_ids: &[String],
    tool_specs: &[ProjectToolSpec],
) -> Result<TemplateMutationResult> {
    let mut template = require_template(store, template_id)?;
    let name = normalize_template_name(name)?;
    if store
        .get_skill_template_by_name(&name)?
        .is_some_and(|other| other.id != template.id)
    {
        anyhow::bail!("skill template name already exists: {name}");
    }
    let previous_ids = store
        .list_skill_template_item_ids(template_id)?
        .into_iter()
        .collect::<HashSet<_>>();
    let skills = validate_template_skills(store, skill_ids, &previous_ids)?;
    let next_ids = skills
        .iter()
        .map(|skill| skill.id.clone())
        .collect::<HashSet<_>>();
    let membership_changed = previous_ids != next_ids;
    let now = now_ms();
    let revision_id = Uuid::new_v4().to_string();
    template.name = name.clone();
    template.description = normalize_description(description);
    template.current_revision_id = revision_id.clone();
    template.updated_at = now;
    let revision = SkillTemplateRevisionRecord {
        id: revision_id.clone(),
        template_id: template.id.clone(),
        revision_number: store.next_skill_template_revision_number(template_id)?,
        name,
        description: template.description.clone(),
        created_at: now,
    };
    store.save_skill_template_revision(
        &template,
        &revision,
        &revision_items(&revision_id, &skills),
    )?;

    let (reconciled_projects, failed_projects) = if membership_changed && template.mode == "live" {
        reconcile_template_projects(store, template_id, tool_specs)
    } else {
        (0, 0)
    };
    Ok(TemplateMutationResult {
        template: Some(build_template_view(store, template)?),
        membership_changed,
        reconciled_projects,
        failed_projects,
    })
}

pub fn rollback_skill_template(
    store: &SkillStore,
    template_id: &str,
    revision_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<TemplateMutationResult> {
    let revision = store
        .get_skill_template_revision(revision_id)?
        .with_context(|| format!("skill template revision not found: {revision_id}"))?;
    if revision.template_id != template_id {
        anyhow::bail!("revision does not belong to skill template");
    }
    let item_ids = store
        .list_skill_template_revision_items(revision_id)?
        .into_iter()
        .map(|item| item.skill_id)
        .collect::<Vec<_>>();
    update_skill_template(
        store,
        template_id,
        &revision.name,
        revision.description.as_deref(),
        &item_ids,
        tool_specs,
    )
}

pub fn set_skill_template_archived(
    store: &SkillStore,
    template_id: &str,
    archived: bool,
) -> Result<SkillTemplateView> {
    store.set_skill_template_archived(template_id, archived)?;
    get_skill_template(store, template_id)
}

pub fn delete_skill_template(store: &SkillStore, template_id: &str) -> Result<()> {
    store.delete_skill_template(template_id)
}

pub fn preview_project_template_change(
    store: &SkillStore,
    project_id: &str,
    template_id: Option<&str>,
) -> Result<TemplateDiff> {
    let current_ids = match store.get_project_template_binding(project_id)? {
        Some(binding) => revision_skill_ids(store, &binding.applied_revision_id)?,
        None => HashSet::new(),
    };
    let next_ids = match template_id {
        Some(template_id) => {
            let template = require_template(store, template_id)?;
            if template.archived {
                anyhow::bail!("archived skill template cannot be newly applied");
            }
            revision_skill_ids(store, &template.current_revision_id)?
        }
        None => HashSet::new(),
    };
    build_template_diff(store, &current_ids, &next_ids)
}

pub fn apply_project_template(
    store: &SkillStore,
    project_id: &str,
    template_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<TemplateReconcileResult> {
    let project = store
        .get_project(project_id)?
        .with_context(|| format!("project not found: {project_id}"))?;
    let template = require_template(store, template_id)?;
    let existing_binding = store.get_project_template_binding(project_id)?;
    if template.archived
        && existing_binding
            .as_ref()
            .map_or(true, |binding| binding.template_id != template.id)
    {
        anyhow::bail!("archived skill template cannot be newly applied");
    }

    let old_template_id = existing_binding
        .as_ref()
        .map(|binding| binding.template_id.clone());
    let old_skill_ids = match &existing_binding {
        Some(binding) => revision_skill_ids(store, &binding.applied_revision_id)?,
        None => HashSet::new(),
    };
    let new_skill_ids = revision_skill_ids(store, &template.current_revision_id)?;
    let now = now_ms();

    for skill_id in &new_skill_ids {
        upsert_template_origin(
            store,
            &project.id,
            skill_id,
            &template.id,
            &project.enabled_tool_keys,
        )?;
    }
    if let Some(old_template_id) = old_template_id.as_deref() {
        if old_template_id != template.id {
            store.delete_project_skill_origins_by_source(
                &project.id,
                "template",
                old_template_id,
            )?;
            store.replace_project_template_failures(&project.id, old_template_id, &[])?;
        } else {
            for skill_id in old_skill_ids.difference(&new_skill_ids) {
                store.delete_project_skill_origin(
                    &project.id,
                    skill_id,
                    "template",
                    &template.id,
                )?;
            }
        }
    }

    store.upsert_project_template_binding(&ProjectTemplateBindingRecord {
        project_id: project.id.clone(),
        template_id: template.id.clone(),
        applied_revision_id: template.current_revision_id.clone(),
        sync_error: existing_binding
            .as_ref()
            .and_then(|binding| binding.sync_error.clone()),
        last_attempt_at: existing_binding
            .as_ref()
            .and_then(|binding| binding.last_attempt_at),
        synced_at: existing_binding
            .as_ref()
            .and_then(|binding| binding.synced_at),
        created_at: existing_binding
            .as_ref()
            .map(|binding| binding.created_at)
            .unwrap_or(now),
        updated_at: now,
    })?;

    let affected = old_skill_ids
        .union(&new_skill_ids)
        .cloned()
        .collect::<HashSet<_>>();
    reconcile_project_binding_skills(store, project_id, tool_specs, &affected, None)
}

pub fn update_snapshot_project_template(
    store: &SkillStore,
    project_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<TemplateReconcileResult> {
    let binding = require_binding(store, project_id)?;
    let template = require_template(store, &binding.template_id)?;
    if template.mode != "snapshot" {
        anyhow::bail!("project template is not in snapshot mode");
    }
    apply_project_template(store, project_id, &template.id, tool_specs)
}

pub fn detach_project_template(
    store: &SkillStore,
    project_id: &str,
    retain_as_manual: bool,
    tool_specs: &[ProjectToolSpec],
) -> Result<()> {
    let binding = require_binding(store, project_id)?;
    let project = store
        .get_project(project_id)?
        .with_context(|| format!("project not found: {project_id}"))?;
    let template_origins = store
        .list_project_skill_origins(project_id)?
        .into_iter()
        .filter(|origin| {
            origin.source_type == "template" && origin.source_id == binding.template_id
        })
        .collect::<Vec<_>>();
    let affected = template_origins
        .iter()
        .map(|origin| origin.skill_id.clone())
        .collect::<HashSet<_>>();
    if retain_as_manual {
        for origin in &template_origins {
            let mut tools = origin.desired_tools.clone();
            if let Some(manual) = store
                .list_project_skill_origins_for_skill(project_id, &origin.skill_id)?
                .into_iter()
                .find(|item| item.source_type == "manual" && item.source_id.is_empty())
            {
                tools.extend(manual.desired_tools);
            }
            let tools = normalized_keys(&tools);
            let now = now_ms();
            store.upsert_project_skill_origin(&ProjectSkillOriginRecord {
                project_id: project.id.clone(),
                skill_id: origin.skill_id.clone(),
                source_type: "manual".to_string(),
                source_id: String::new(),
                desired_tools: tools,
                created_at: now,
                updated_at: now,
            })?;
        }
    }
    store.delete_project_skill_origins_by_source(project_id, "template", &binding.template_id)?;
    store.replace_project_template_failures(project_id, &binding.template_id, &[])?;
    store.delete_project_template_binding(project_id)?;
    for skill_id in affected {
        reconcile_effective_project_skill(store, &project, &skill_id, tool_specs, None)?;
    }
    Ok(())
}

pub fn reconcile_project_template(
    store: &SkillStore,
    project_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<TemplateReconcileResult> {
    let binding = require_binding(store, project_id)?;
    let template = require_template(store, &binding.template_id)?;
    let desired_revision_id = if template.mode == "live" {
        template.current_revision_id.clone()
    } else {
        binding.applied_revision_id.clone()
    };
    let previous_ids = store
        .list_project_skill_origins(project_id)?
        .into_iter()
        .filter(|origin| {
            origin.source_type == "template" && origin.source_id == binding.template_id
        })
        .map(|origin| origin.skill_id)
        .collect::<HashSet<_>>();
    let desired_ids = revision_skill_ids(store, &desired_revision_id)?;
    let project = store
        .get_project(project_id)?
        .with_context(|| format!("project not found: {project_id}"))?;
    for skill_id in &desired_ids {
        upsert_template_origin(
            store,
            project_id,
            skill_id,
            &binding.template_id,
            &project.enabled_tool_keys,
        )?;
    }
    for skill_id in previous_ids.difference(&desired_ids) {
        store.delete_project_skill_origin(
            project_id,
            skill_id,
            "template",
            &binding.template_id,
        )?;
    }
    let mut updated_binding = binding;
    updated_binding.applied_revision_id = desired_revision_id;
    updated_binding.updated_at = now_ms();
    store.upsert_project_template_binding(&updated_binding)?;
    let affected = previous_ids
        .union(&desired_ids)
        .cloned()
        .collect::<HashSet<_>>();
    reconcile_project_binding_skills(store, project_id, tool_specs, &affected, None)
}

pub fn retry_project_template(
    store: &SkillStore,
    project_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<TemplateReconcileResult> {
    reconcile_project_template(store, project_id, tool_specs)
}

pub fn resolve_project_template_conflict(
    store: &SkillStore,
    project_id: &str,
    skill_id: &str,
    use_library_version: bool,
    tool_specs: &[ProjectToolSpec],
) -> Result<TemplateReconcileResult> {
    let binding = require_binding(store, project_id)?;
    let desired = revision_skill_ids(store, &binding.applied_revision_id)?;
    if !desired.contains(skill_id) {
        anyhow::bail!("skill is not managed by the project's template");
    }
    if use_library_version {
        store.delete_project_template_override(project_id, skill_id)?;
    } else {
        let now = now_ms();
        store.upsert_project_template_override(&ProjectTemplateOverrideRecord {
            project_id: project_id.to_string(),
            skill_id: skill_id.to_string(),
            kind: "native".to_string(),
            created_at: now,
            updated_at: now,
        })?;
    }
    reconcile_project_binding_skills(
        store,
        project_id,
        tool_specs,
        &HashSet::from([skill_id.to_string()]),
        use_library_version.then_some(skill_id),
    )
}

pub fn reconcile_all_project_templates(
    store: &SkillStore,
    tool_specs: &[ProjectToolSpec],
) -> (usize, usize) {
    let bindings = match store.list_all_project_template_bindings() {
        Ok(bindings) => bindings,
        Err(err) => {
            log::warn!("list project template bindings for reconciliation: {err:#}");
            return (0, 1);
        }
    };
    let mut reconciled = 0;
    let mut failed = 0;
    for binding in bindings {
        match reconcile_project_template(store, &binding.project_id, tool_specs) {
            Ok(result) => {
                reconciled += 1;
                if !result.failed.is_empty() {
                    failed += 1;
                }
            }
            Err(err) => {
                failed += 1;
                if let Err(record_err) = record_binding_error(store, &binding, &err) {
                    log::warn!("record project template reconciliation failure: {record_err:#}");
                }
            }
        }
    }
    (reconciled, failed)
}

fn reconcile_template_projects(
    store: &SkillStore,
    template_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> (usize, usize) {
    let bindings = match store.list_project_template_bindings(template_id) {
        Ok(bindings) => bindings,
        Err(err) => {
            log::warn!("list template projects for reconciliation: {err:#}");
            return (0, 1);
        }
    };
    let mut reconciled = 0;
    let mut failed = 0;
    for binding in bindings {
        match reconcile_project_template(store, &binding.project_id, tool_specs) {
            Ok(result) => {
                reconciled += 1;
                if !result.failed.is_empty() {
                    failed += 1;
                }
            }
            Err(err) => {
                failed += 1;
                if let Err(record_err) = record_binding_error(store, &binding, &err) {
                    log::warn!("record template project reconciliation failure: {record_err:#}");
                }
            }
        }
    }
    (reconciled, failed)
}

fn reconcile_project_binding_skills(
    store: &SkillStore,
    project_id: &str,
    tool_specs: &[ProjectToolSpec],
    affected_skill_ids: &HashSet<String>,
    overwrite_skill_id: Option<&str>,
) -> Result<TemplateReconcileResult> {
    let mut binding = require_binding(store, project_id)?;
    let project = store
        .get_project(project_id)?
        .with_context(|| format!("project not found: {project_id}"))?;
    let now = now_ms();
    let mut result = TemplateReconcileResult {
        project_id: project.id.clone(),
        template_id: binding.template_id.clone(),
        applied_revision_id: binding.applied_revision_id.clone(),
        ..TemplateReconcileResult::default()
    };

    result.failed.extend(
        store
            .list_project_template_failures(&project.id)?
            .into_iter()
            .filter(|failure| {
                failure.template_id == binding.template_id
                    && !affected_skill_ids.contains(&failure.skill_id)
            }),
    );

    for skill_id in affected_skill_ids {
        let managed_by_current_template = store
            .list_project_skill_origins_for_skill(&project.id, skill_id)?
            .into_iter()
            .any(|origin| {
                origin.source_type == "template" && origin.source_id == binding.template_id
            });
        match reconcile_effective_project_skill(
            store,
            &project,
            skill_id,
            tool_specs,
            (overwrite_skill_id == Some(skill_id.as_str())).then_some(skill_id.as_str()),
        ) {
            Ok(mut skill_failures) => {
                skill_failures.retain(|failure| failure.template_id == binding.template_id);
                if skill_failures.is_empty() {
                    result.succeeded += 1;
                }
                result.failed.append(&mut skill_failures);
            }
            Err(err) => {
                if managed_by_current_template {
                    result.failed.push(ProjectTemplateFailureRecord {
                        project_id: project.id.clone(),
                        template_id: binding.template_id.clone(),
                        skill_id: skill_id.clone(),
                        tool: String::new(),
                        kind: "reconcile_failed".to_string(),
                        error: format!("{err:#}"),
                        updated_at: now,
                    });
                } else {
                    log::warn!(
                        "reconcile remaining non-template project Skill {skill_id}: {err:#}"
                    );
                }
            }
        }
    }

    result.failed.sort_by(|left, right| {
        left.skill_id
            .cmp(&right.skill_id)
            .then_with(|| left.tool.cmp(&right.tool))
            .then_with(|| left.kind.cmp(&right.kind))
    });

    store.replace_project_template_failures(&project.id, &binding.template_id, &result.failed)?;
    binding.last_attempt_at = Some(now);
    binding.updated_at = now;
    if result.failed.is_empty() {
        binding.sync_error = None;
        binding.synced_at = Some(now);
    } else {
        binding.sync_error = Some(
            result
                .failed
                .iter()
                .map(|failure| failure.error.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    store.upsert_project_template_binding(&binding)?;
    Ok(result)
}

fn reconcile_effective_project_skill(
    store: &SkillStore,
    project: &super::skill_store::ProjectRecord,
    skill_id: &str,
    tool_specs: &[ProjectToolSpec],
    overwrite_skill_id: Option<&str>,
) -> Result<Vec<ProjectTemplateFailureRecord>> {
    let origins = store.list_project_skill_origins_for_skill(&project.id, skill_id)?;
    if origins.is_empty() {
        remove_project_skill(store, project, skill_id)?;
        return Ok(Vec::new());
    }
    let effective_tools = effective_project_skill_tools(store, &project.id, skill_id)?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .with_context(|| format!("skill not found: {skill_id}"))?;
    let template_id = origins
        .iter()
        .find(|origin| origin.source_type == "template")
        .map(|origin| origin.source_id.clone())
        .unwrap_or_default();
    let mut failures = Vec::new();

    if effective_tools.is_empty() {
        let existing_targets = store
            .list_project_skill_targets(&project.root_path)?
            .into_iter()
            .any(|target| target.skill_id == skill.id);
        if existing_targets {
            materialize_project_skill_tools_and_scan(
                store,
                &project.id,
                skill_id,
                &[],
                tool_specs,
                &HashSet::new(),
            )?;
        }
        upsert_effective_link(store, project, &skill, &effective_tools, "ok")?;
        return Ok(failures);
    }

    if !skill.enabled {
        remove_project_skill(store, project, skill_id)?;
        upsert_effective_link(store, project, &skill, &effective_tools, "suspended")?;
        for tool in &effective_tools {
            upsert_project_assignment(store, project, &skill, tool, "hub", "suspended", None)?;
        }
        return Ok(failures);
    }

    if !Path::new(&project.root_path).is_dir() {
        for tool in &effective_tools {
            failures.push(template_failure(
                project,
                &template_id,
                &skill,
                tool,
                "project_unavailable",
                "project path is unavailable",
            ));
        }
        upsert_effective_link(store, project, &skill, &effective_tools, "error")?;
        return Ok(failures);
    }

    let _ = scan_project(store, project, tool_specs);
    if store
        .get_project_template_override(&project.id, skill_id)?
        .is_some()
    {
        let native_exists = store
            .list_project_discovered_skills(&project.id)?
            .into_iter()
            .any(|item| {
                item.status != "invalid_missing_skill_md"
                    && item.name.eq_ignore_ascii_case(&skill.name)
            });
        if native_exists {
            for tool in &effective_tools {
                upsert_project_assignment(store, project, &skill, tool, "native", "active", None)?;
            }
            upsert_effective_link(store, project, &skill, &effective_tools, "ok")?;
            return Ok(failures);
        }
    }

    let globally_satisfied = effective_tools
        .iter()
        .filter_map(|tool| {
            global_assignment_satisfies(store, &skill, tool)
                .ok()
                .filter(|satisfied| *satisfied)
                .map(|_| tool.clone())
        })
        .collect::<HashSet<_>>();
    let available_specs = tool_specs
        .iter()
        .filter(|spec| spec.available)
        .map(|spec| spec.key.as_str())
        .collect::<HashSet<_>>();
    let local_tools = effective_tools
        .iter()
        .filter(|tool| {
            !globally_satisfied.contains(*tool) && available_specs.contains(tool.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    for tool in effective_tools.iter().filter(|tool| {
        !globally_satisfied.contains(*tool) && !available_specs.contains(tool.as_str())
    }) {
        failures.push(template_failure(
            project,
            &template_id,
            &skill,
            tool,
            "tool_unavailable",
            "project Agent is unavailable",
        ));
    }

    let overwrite_tools = if overwrite_skill_id == Some(skill_id) {
        local_tools.iter().cloned().collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    match materialize_project_skill_tools_and_scan(
        store,
        &project.id,
        skill_id,
        &local_tools,
        tool_specs,
        &overwrite_tools,
    ) {
        Ok(summary) if summary.failed > 0 => {
            for tool in &local_tools {
                let assignment =
                    store.get_skill_assignment(skill_id, tool, "project", Some(&project.id))?;
                if assignment
                    .as_ref()
                    .is_some_and(|item| item.state == "error")
                {
                    failures.push(template_failure(
                        project,
                        &template_id,
                        &skill,
                        tool,
                        "content_conflict",
                        assignment
                            .as_ref()
                            .and_then(|item| item.last_error.as_deref())
                            .unwrap_or("project Skill conflicts with the library Skill"),
                    ));
                }
            }
        }
        Ok(_) => {}
        Err(err) => {
            let message = format!("{err:#}");
            for tool in &local_tools {
                failures.push(template_failure(
                    project,
                    &template_id,
                    &skill,
                    tool,
                    "materialization_failed",
                    &message,
                ));
            }
        }
    }

    for tool in &globally_satisfied {
        if let Some(assignment) =
            store.get_skill_assignment(skill_id, tool, "project", Some(&project.id))?
        {
            store.delete_skill_assignment(
                skill_id,
                &assignment.tool,
                "project",
                Some(&project.id),
            )?;
        }
    }
    upsert_effective_link(
        store,
        project,
        &skill,
        &effective_tools,
        if failures.is_empty() { "ok" } else { "partial" },
    )?;
    Ok(failures)
}

fn global_assignment_satisfies(
    store: &SkillStore,
    skill: &SkillRecord,
    tool: &str,
) -> Result<bool> {
    let assignment = store.get_skill_assignment(&skill.id, tool, "global", None)?;
    if !assignment
        .as_ref()
        .is_some_and(|item| item.state == "active")
    {
        return Ok(false);
    }
    let target = store.get_skill_target(&skill.id, tool, "global", None)?;
    Ok(target.as_ref().is_some_and(|target| {
        target.status == "ok" && std::fs::symlink_metadata(&target.target_path).is_ok()
    }))
}

fn upsert_effective_link(
    store: &SkillStore,
    project: &super::skill_store::ProjectRecord,
    skill: &SkillRecord,
    desired_tools: &[String],
    status: &str,
) -> Result<()> {
    let now = now_ms();
    let existing = store.get_project_skill_link(&project.id, &skill.id)?;
    store.upsert_project_skill_link(&ProjectSkillLinkRecord {
        project_id: project.id.clone(),
        skill_id: skill.id.clone(),
        enabled: true,
        desired_tools: normalized_keys(desired_tools),
        status: status.to_string(),
        created_at: existing.as_ref().map(|link| link.created_at).unwrap_or(now),
        updated_at: now,
    })
}

fn upsert_project_assignment(
    store: &SkillStore,
    project: &super::skill_store::ProjectRecord,
    skill: &SkillRecord,
    tool: &str,
    ownership: &str,
    state: &str,
    last_error: Option<&str>,
) -> Result<()> {
    let now = now_ms();
    let existing = store.get_skill_assignment(&skill.id, tool, "project", Some(&project.id))?;
    store.upsert_skill_assignment(&SkillAssignmentRecord {
        id: existing
            .as_ref()
            .map(|assignment| assignment.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        skill_id: skill.id.clone(),
        tool: tool.to_string(),
        scope: "project".to_string(),
        project_id: Some(project.id.clone()),
        ownership: ownership.to_string(),
        state: state.to_string(),
        last_error: last_error.map(str::to_string),
        created_at: existing
            .as_ref()
            .map(|assignment| assignment.created_at)
            .unwrap_or(now),
        updated_at: now,
    })
}

fn upsert_template_origin(
    store: &SkillStore,
    project_id: &str,
    skill_id: &str,
    template_id: &str,
    desired_tools: &[String],
) -> Result<()> {
    let now = now_ms();
    let existing = store
        .list_project_skill_origins_for_skill(project_id, skill_id)?
        .into_iter()
        .find(|origin| origin.source_type == "template" && origin.source_id == template_id);
    store.upsert_project_skill_origin(&ProjectSkillOriginRecord {
        project_id: project_id.to_string(),
        skill_id: skill_id.to_string(),
        source_type: "template".to_string(),
        source_id: template_id.to_string(),
        desired_tools: normalized_keys(desired_tools),
        created_at: existing
            .as_ref()
            .map(|origin| origin.created_at)
            .unwrap_or(now),
        updated_at: now,
    })
}

fn template_failure(
    project: &super::skill_store::ProjectRecord,
    template_id: &str,
    skill: &SkillRecord,
    tool: &str,
    kind: &str,
    error: &str,
) -> ProjectTemplateFailureRecord {
    ProjectTemplateFailureRecord {
        project_id: project.id.clone(),
        template_id: template_id.to_string(),
        skill_id: skill.id.clone(),
        tool: tool.to_string(),
        kind: kind.to_string(),
        error: error.to_string(),
        updated_at: now_ms(),
    }
}

fn record_binding_error(
    store: &SkillStore,
    binding: &ProjectTemplateBindingRecord,
    error: &anyhow::Error,
) -> Result<()> {
    let mut binding = binding.clone();
    binding.sync_error = Some(format!("{error:#}"));
    binding.last_attempt_at = Some(now_ms());
    binding.updated_at = now_ms();
    store.upsert_project_template_binding(&binding)
}

fn build_template_view(
    store: &SkillStore,
    template: SkillTemplateRecord,
) -> Result<SkillTemplateView> {
    let library = store
        .list_skills()?
        .into_iter()
        .map(|skill| (skill.id.clone(), skill))
        .collect::<HashMap<_, _>>();
    let mut skills = store
        .list_skill_template_item_ids(&template.id)?
        .into_iter()
        .filter_map(|skill_id| library.get(&skill_id).map(template_skill))
        .collect::<Vec<_>>();
    skills.sort_by_key(|skill| skill.name.to_lowercase());
    let linked_projects = store
        .list_project_template_bindings(&template.id)?
        .into_iter()
        .filter_map(|binding| {
            let project = store.get_project(&binding.project_id).ok().flatten()?;
            Some(TemplateProjectLink {
                project_id: project.id,
                project_name: project.name,
                project_root: project.root_path,
                applied_revision_id: binding.applied_revision_id.clone(),
                snapshot_outdated: template.mode == "snapshot"
                    && binding.applied_revision_id != template.current_revision_id,
                sync_error: binding.sync_error,
            })
        })
        .collect::<Vec<_>>();
    let failure_count = store.list_template_project_failures(&template.id)?.len();
    Ok(SkillTemplateView {
        template,
        skills,
        linked_projects,
        failure_count,
    })
}

fn build_template_diff(
    store: &SkillStore,
    current_ids: &HashSet<String>,
    next_ids: &HashSet<String>,
) -> Result<TemplateDiff> {
    let library = store
        .list_skills()?
        .into_iter()
        .map(|skill| (skill.id.clone(), skill))
        .collect::<HashMap<_, _>>();
    let mut added = next_ids
        .difference(current_ids)
        .filter_map(|id| library.get(id).map(template_skill))
        .collect::<Vec<_>>();
    let mut removed = current_ids
        .difference(next_ids)
        .filter_map(|id| library.get(id).map(template_skill))
        .collect::<Vec<_>>();
    let mut retained = current_ids
        .intersection(next_ids)
        .filter_map(|id| library.get(id).map(template_skill))
        .collect::<Vec<_>>();
    added.sort_by_key(|skill| skill.name.to_lowercase());
    removed.sort_by_key(|skill| skill.name.to_lowercase());
    retained.sort_by_key(|skill| skill.name.to_lowercase());
    Ok(TemplateDiff {
        added,
        removed,
        retained,
    })
}

fn validate_template_skills(
    store: &SkillStore,
    skill_ids: &[String],
    existing_ids: &HashSet<String>,
) -> Result<Vec<SkillRecord>> {
    let skill_ids = normalized_keys(skill_ids);
    if skill_ids.is_empty() {
        anyhow::bail!("skill template must contain at least one skill");
    }
    skill_ids
        .into_iter()
        .map(|skill_id| {
            let skill = store
                .get_skill_by_id(&skill_id)?
                .with_context(|| format!("skill not found: {skill_id}"))?;
            if !skill.enabled && !existing_ids.contains(&skill_id) {
                anyhow::bail!(
                    "disabled skill cannot be added to a template: {}",
                    skill.name
                );
            }
            Ok(skill)
        })
        .collect()
}

fn revision_items(
    revision_id: &str,
    skills: &[SkillRecord],
) -> Vec<SkillTemplateRevisionItemRecord> {
    skills
        .iter()
        .map(|skill| SkillTemplateRevisionItemRecord {
            revision_id: revision_id.to_string(),
            skill_id: skill.id.clone(),
            skill_name: skill.name.clone(),
        })
        .collect()
}

fn revision_skill_ids(store: &SkillStore, revision_id: &str) -> Result<HashSet<String>> {
    let revision = store
        .get_skill_template_revision(revision_id)?
        .with_context(|| format!("skill template revision not found: {revision_id}"))?;
    let mut result = HashSet::new();
    for item in store.list_skill_template_revision_items(&revision.id)? {
        if store.get_skill_by_id(&item.skill_id)?.is_none() {
            anyhow::bail!(
                "skill template revision references a missing Skill: {}",
                item.skill_name
            );
        }
        result.insert(item.skill_id);
    }
    Ok(result)
}

fn require_template(store: &SkillStore, template_id: &str) -> Result<SkillTemplateRecord> {
    store
        .get_skill_template(template_id)?
        .with_context(|| format!("skill template not found: {template_id}"))
}

fn require_binding(store: &SkillStore, project_id: &str) -> Result<ProjectTemplateBindingRecord> {
    store
        .get_project_template_binding(project_id)?
        .with_context(|| format!("project has no skill template: {project_id}"))
}

fn template_skill(skill: &SkillRecord) -> TemplateSkill {
    TemplateSkill {
        id: skill.id.clone(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        enabled: skill.enabled,
    }
}

fn normalize_template_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("skill template name cannot be empty");
    }
    Ok(name.to_string())
}

fn normalize_description(description: Option<&str>) -> Option<String> {
    description
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_template_mode(mode: &str) -> Result<()> {
    if matches!(mode, "live" | "snapshot") {
        Ok(())
    } else {
        anyhow::bail!("invalid skill template mode: {mode}")
    }
}

fn normalized_keys(keys: &[String]) -> Vec<String> {
    let mut keys = keys
        .iter()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

#[cfg(test)]
#[path = "tests/skill_templates.rs"]
mod tests;
