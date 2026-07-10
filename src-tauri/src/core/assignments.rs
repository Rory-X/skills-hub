use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::content_hash::hash_dir;
use super::projects::{
    add_project_skill_tools_and_scan, assign_project_skill_tools_and_scan,
    project_issue_fingerprint, ProjectInjectionSummary, ProjectIssue, ProjectToolSpec,
};
use super::skill_store::{
    IgnoredIssueRecord, ProjectSkillLinkRecord, SkillAssignmentRecord, SkillRecord, SkillStore,
    SkillTargetRecord,
};
use super::sync_engine::{sync_dir_for_tool_with_overwrite, SyncMode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignmentToolSpec {
    pub key: String,
    pub label: String,
    pub global_skills_dir: PathBuf,
    pub project_skills_dir: String,
    pub global_available: bool,
    pub enabled: bool,
    pub supports_project_scope: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssignmentDraft {
    pub global_tools: Vec<String>,
    pub projects: Vec<ProjectAssignmentDraft>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectAssignmentDraft {
    pub project_id: String,
    pub tool_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignmentPreviewItem {
    pub key: String,
    pub tool: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub target_path: Option<String>,
    pub outcome: String,
    pub blocking: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssignmentConflictKey {
    pub skill_id: String,
    pub target_key: String,
}

impl AssignmentConflictKey {
    pub fn new(skill_id: impl Into<String>, target_key: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            target_key: target_key.into(),
        }
    }

    pub fn encode(&self) -> String {
        format!("{}::{}", self.skill_id, self.target_key)
    }

    pub fn decode(value: &str) -> Result<Self> {
        let (skill_id, target_key) = value
            .split_once("::")
            .filter(|(skill_id, target_key)| !skill_id.is_empty() && !target_key.is_empty())
            .ok_or_else(|| anyhow::anyhow!("invalid assignment conflict key"))?;
        Ok(Self::new(skill_id, target_key))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssignmentApplySummary {
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignmentProjection {
    pub assignment: SkillAssignmentRecord,
    pub valid: bool,
    pub effective_state: String,
    pub effective_error: Option<String>,
    pub issue_kind: Option<String>,
    pub issue_key: Option<String>,
    pub issue_fingerprint: Option<String>,
    pub can_ignore: bool,
    pub ignored: bool,
}

#[derive(Clone, Debug)]
pub struct ManagedSkillProjection {
    pub skill: SkillRecord,
    pub targets: Vec<SkillTargetRecord>,
    pub assignments: Vec<AssignmentProjection>,
    pub source_issue: Option<String>,
    pub pending_assignment: bool,
    pub assignment_issue_count: usize,
}

#[derive(Clone, Debug)]
pub struct SkillAssignmentPreview {
    pub skill: SkillRecord,
    pub items: Vec<AssignmentPreviewItem>,
}

#[derive(Clone, Debug)]
pub struct SkillAssignmentApplyOutcome {
    pub skill: SkillRecord,
    pub summary: AssignmentApplySummary,
}

#[derive(Clone, Debug)]
struct AssignmentEvaluation {
    valid: bool,
    issue_kind: Option<&'static str>,
    message: Option<String>,
    project_issue_kind: Option<&'static str>,
    project_issue_path: Option<String>,
}

pub fn preview_skill_assignment(
    store: &SkillStore,
    skill_id: &str,
    draft: &AssignmentDraft,
    tools: &[AssignmentToolSpec],
    include_removals: bool,
) -> Result<Vec<AssignmentPreviewItem>> {
    let skill = require_skill(store, skill_id)?;
    let normalized = normalized_draft(draft, tools);
    let tools_by_key = tools
        .iter()
        .map(|tool| (tool.key.as_str(), tool))
        .collect::<HashMap<_, _>>();
    let mut items = Vec::new();

    for tool_key in &normalized.global_tools {
        let Some(tool) = tools_by_key.get(tool_key.as_str()) else {
            items.push(unavailable_preview(tool_key, "global", None, None));
            continue;
        };
        let target = tool.global_skills_dir.join(&skill.name);
        items.push(preview_target(
            store,
            &skill,
            tool,
            "global",
            None,
            None,
            target,
            tool.global_available,
        )?);
    }

    for project_draft in &normalized.projects {
        let Some(project) = store.get_project(&project_draft.project_id)? else {
            for tool_key in &project_draft.tool_keys {
                items.push(unavailable_preview(
                    tool_key,
                    "project",
                    Some(project_draft.project_id.clone()),
                    None,
                ));
            }
            continue;
        };
        for tool_key in &project_draft.tool_keys {
            let Some(tool) = tools_by_key.get(tool_key.as_str()) else {
                items.push(unavailable_preview(
                    tool_key,
                    "project",
                    Some(project.id.clone()),
                    Some(project.name.clone()),
                ));
                continue;
            };
            let available = tool.enabled
                && tool.supports_project_scope
                && project.enabled_tool_keys.contains(tool_key);
            let target = Path::new(&project.root_path)
                .join(&tool.project_skills_dir)
                .join(&skill.name);
            items.push(preview_target(
                store,
                &skill,
                tool,
                "project",
                Some(&project.id),
                Some(&project.name),
                target,
                available,
            )?);
        }
    }

    if include_removals {
        let desired_keys = draft_target_keys(&normalized);
        for assignment in store.list_skill_assignments(skill_id)? {
            let key = target_key(
                &assignment.scope,
                assignment.project_id.as_deref(),
                &assignment.tool,
            );
            if desired_keys.contains(&key) {
                continue;
            }
            let project = assignment
                .project_id
                .as_deref()
                .and_then(|project_id| store.get_project(project_id).transpose())
                .transpose()?;
            let project_path = project.as_ref().map(|project| project.root_path.as_str());
            let target_path = store
                .get_skill_target(skill_id, &assignment.tool, &assignment.scope, project_path)?
                .map(|target| target.target_path);
            items.push(AssignmentPreviewItem {
                key,
                tool: assignment.tool,
                scope: assignment.scope,
                project_id: assignment.project_id,
                project_name: project.map(|project| project.name),
                target_path,
                outcome: "remove".to_string(),
                blocking: true,
            });
        }
    }

    items.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(items)
}

pub fn project_skill_assignments(
    store: &SkillStore,
    skill: &SkillRecord,
    tools: &[AssignmentToolSpec],
) -> Result<Vec<AssignmentProjection>> {
    let source = Path::new(&skill.central_path);
    let source_hash = if source.is_dir() && source.join("SKILL.md").is_file() {
        hash_dir(source).ok()
    } else {
        None
    };
    let tools_by_key = tools
        .iter()
        .map(|tool| (tool.key.as_str(), tool))
        .collect::<HashMap<_, _>>();
    let assignments = store.list_skill_assignments(&skill.id)?;
    let mut evaluated = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        let evaluation = evaluate_assignment(
            store,
            skill,
            &assignment,
            tools_by_key.get(assignment.tool.as_str()).copied(),
            source_hash.as_deref(),
        )?;
        evaluated.push((assignment, evaluation));
    }
    let valid_count = evaluated
        .iter()
        .filter(|(_, evaluation)| evaluation.valid)
        .count();

    evaluated
        .into_iter()
        .map(|(assignment, evaluation)| {
            let issue_key = (!evaluation.valid).then(|| {
                target_key(
                    &assignment.scope,
                    assignment.project_id.as_deref(),
                    &assignment.tool,
                )
            });
            let issue_fingerprint = issue_key.as_deref().map(|key| {
                assignment_issue_fingerprint(
                    store,
                    skill,
                    &assignment,
                    &evaluation,
                    tools_by_key.get(assignment.tool.as_str()).copied(),
                    source_hash.as_deref(),
                    key,
                )
            });
            let issue_fingerprint = issue_fingerprint.transpose()?;
            let can_ignore = evaluation
                .issue_kind
                .is_some_and(assignment_issue_kind_can_ignore)
                && valid_count > 0;
            let ignored = match (issue_key.as_deref(), issue_fingerprint.as_deref()) {
                (Some(key), Some(fingerprint)) if can_ignore => {
                    store.is_issue_ignored("assignment", Some(&skill.id), key, fingerprint)?
                        || matching_project_issue_ignored(store, skill, &assignment, &evaluation)?
                }
                _ => false,
            };
            let effective_state = if evaluation.valid {
                if skill.enabled {
                    "active"
                } else {
                    "suspended"
                }
            } else {
                "error"
            };
            Ok(AssignmentProjection {
                assignment,
                valid: evaluation.valid,
                effective_state: effective_state.to_string(),
                effective_error: evaluation.message,
                issue_kind: evaluation.issue_kind.map(str::to_string),
                issue_key,
                issue_fingerprint,
                can_ignore,
                ignored,
            })
        })
        .collect()
}

pub fn managed_skill_projections(
    store: &SkillStore,
    tools: &[AssignmentToolSpec],
) -> Result<Vec<ManagedSkillProjection>> {
    store
        .list_skills()?
        .into_iter()
        .map(|skill| {
            let targets = store.list_skill_targets(&skill.id)?;
            let assignments = project_skill_assignments(store, &skill, tools)?;
            let source_issue = managed_skill_source_issue(&skill);
            let pending_assignment = !assignments.iter().any(|assignment| assignment.valid);
            let assignment_issue_count = assignments
                .iter()
                .filter(|assignment| {
                    !assignment.valid
                        && !assignment.ignored
                        && !matches!(
                            assignment.issue_kind.as_deref(),
                            Some("source_missing" | "source_invalid")
                        )
                })
                .count()
                + usize::from(source_issue.is_some());
            Ok(ManagedSkillProjection {
                skill,
                targets,
                assignments,
                source_issue,
                pending_assignment,
                assignment_issue_count,
            })
        })
        .collect()
}

fn managed_skill_source_issue(skill: &SkillRecord) -> Option<String> {
    let source = Path::new(&skill.central_path);
    if !source.is_dir() {
        Some("source_missing".to_string())
    } else if !source.join("SKILL.md").is_file()
        || hash_dir(source).is_err()
        || skill.status != "ok"
    {
        Some("source_invalid".to_string())
    } else {
        None
    }
}

pub fn ignore_assignment_issue(
    store: &SkillStore,
    skill_id: &str,
    issue_key: &str,
    fingerprint: &str,
    tools: &[AssignmentToolSpec],
) -> Result<()> {
    let skill = require_skill(store, skill_id)?;
    let projection = project_skill_assignments(store, &skill, tools)?
        .into_iter()
        .find(|projection| projection.issue_key.as_deref() == Some(issue_key))
        .with_context(|| format!("assignment issue no longer exists: {issue_key}"))?;
    if projection.issue_fingerprint.as_deref() != Some(fingerprint) {
        anyhow::bail!("assignment issue changed; refresh and try again");
    }
    if !projection.can_ignore {
        anyhow::bail!("this assignment issue cannot be ignored");
    }
    store.upsert_ignored_issue(&IgnoredIssueRecord {
        id: Uuid::new_v4().to_string(),
        scope: "assignment".to_string(),
        scope_id: Some(skill.id),
        issue_key: issue_key.to_string(),
        fingerprint: fingerprint.to_string(),
        ignored_at: now_ms(),
    })
}

pub fn ignore_all_assignment_issues(
    store: &SkillStore,
    skill_ids: &[String],
    tools: &[AssignmentToolSpec],
) -> Result<usize> {
    let mut ignored = 0;
    for skill_id in skill_ids {
        let skill = require_skill(store, skill_id)?;
        for projection in project_skill_assignments(store, &skill, tools)? {
            if !projection.can_ignore || projection.ignored {
                continue;
            }
            let (Some(issue_key), Some(fingerprint)) =
                (projection.issue_key, projection.issue_fingerprint)
            else {
                continue;
            };
            store.upsert_ignored_issue(&IgnoredIssueRecord {
                id: Uuid::new_v4().to_string(),
                scope: "assignment".to_string(),
                scope_id: Some(skill.id.clone()),
                issue_key,
                fingerprint,
                ignored_at: now_ms(),
            })?;
            ignored += 1;
        }
    }
    Ok(ignored)
}

pub fn resume_skill_assignment(
    store: &SkillStore,
    skill_id: &str,
    tools: &[AssignmentToolSpec],
) -> Result<AssignmentApplySummary> {
    let assignments = store.list_skill_assignments(skill_id)?;
    let draft = assignment_draft_from_records(&assignments);
    apply_skill_assignment(store, skill_id, &draft, tools, &HashSet::new(), false)
}

pub fn preview_skill_assignments(
    store: &SkillStore,
    skill_ids: &[String],
    draft: &AssignmentDraft,
    tools: &[AssignmentToolSpec],
    additive: bool,
) -> Result<Vec<SkillAssignmentPreview>> {
    skill_ids
        .iter()
        .map(|skill_id| {
            let skill = require_skill(store, skill_id)?;
            let items = preview_skill_assignment(store, skill_id, draft, tools, !additive)?;
            Ok(SkillAssignmentPreview { skill, items })
        })
        .collect()
}

pub fn apply_skill_assignments(
    store: &SkillStore,
    skill_ids: &[String],
    draft: &AssignmentDraft,
    tools: &[AssignmentToolSpec],
    replace_conflict_keys: &HashSet<AssignmentConflictKey>,
    additive: bool,
) -> Result<Vec<SkillAssignmentApplyOutcome>> {
    let mut outcomes = Vec::with_capacity(skill_ids.len());
    for skill_id in skill_ids {
        let skill = require_skill(store, skill_id)?;
        let replacements = replace_conflict_keys
            .iter()
            .filter(|key| key.skill_id == skill.id)
            .map(|key| key.target_key.clone())
            .collect::<HashSet<_>>();
        let summary =
            apply_skill_assignment(store, &skill.id, draft, tools, &replacements, additive)?;
        outcomes.push(SkillAssignmentApplyOutcome { skill, summary });
    }
    Ok(outcomes)
}

pub fn apply_skill_assignment(
    store: &SkillStore,
    skill_id: &str,
    draft: &AssignmentDraft,
    tools: &[AssignmentToolSpec],
    replace_conflicts: &HashSet<String>,
    additive: bool,
) -> Result<AssignmentApplySummary> {
    let skill = require_skill(store, skill_id)?;
    let mut normalized = normalized_draft(draft, tools);
    let requested_keys = draft_target_keys(&normalized);
    if additive {
        merge_existing_assignments(store, skill_id, &mut normalized)?;
    }

    let preview = preview_skill_assignment(store, skill_id, &normalized, tools, false)?;
    let skipped_conflicts = preview
        .iter()
        .filter(|item| {
            item.outcome == "conflict"
                && !replace_conflicts.contains(&item.key)
                && (!additive || requested_keys.contains(&item.key))
        })
        .map(|item| item.key.clone())
        .collect::<HashSet<_>>();
    remove_targets_from_draft(&mut normalized, &skipped_conflicts);

    let desired_keys = draft_target_keys(&normalized);
    let existing = store.list_skill_assignments(skill_id)?;
    let tools_by_key = tools
        .iter()
        .map(|tool| (tool.key.as_str(), tool))
        .collect::<HashMap<_, _>>();
    remove_obsolete_global_assignments(store, &skill, &existing, &desired_keys, &tools_by_key)?;

    let mut summary = AssignmentApplySummary {
        skipped: skipped_conflicts.len(),
        ..AssignmentApplySummary::default()
    };
    let preview_by_key = preview
        .into_iter()
        .map(|item| (item.key.clone(), item))
        .collect::<HashMap<_, _>>();

    for tool_key in &normalized.global_tools {
        let key = target_key("global", None, tool_key);
        if additive && !requested_keys.contains(&key) {
            continue;
        }
        let item = preview_by_key.get(&key);
        if !skill.enabled {
            upsert_assignment(
                store,
                &skill.id,
                tool_key,
                "global",
                None,
                "hub",
                "suspended",
                None,
            )?;
            summary.succeeded += 1;
            continue;
        }
        let Some(tool) = tools_by_key.get(tool_key.as_str()) else {
            persist_assignment_error(
                store,
                &skill.id,
                tool_key,
                "global",
                None,
                "tool is unavailable",
            )?;
            summary.failed += 1;
            summary
                .errors
                .push(format!("{}: tool is unavailable", tool_key));
            continue;
        };
        if !tool.global_available {
            persist_assignment_error(
                store,
                &skill.id,
                tool_key,
                "global",
                None,
                "tool is unavailable",
            )?;
            summary.failed += 1;
            summary
                .errors
                .push(format!("{}: tool is unavailable", tool.label));
            continue;
        }

        let outcome = item.map(|value| value.outcome.as_str()).unwrap_or("ready");
        if outcome == "conflict" && !replace_conflicts.contains(&key) {
            persist_assignment_error(
                store,
                &skill.id,
                tool_key,
                "global",
                None,
                "content conflict blocks assignment",
            )?;
            continue;
        }
        let result = if matches!(outcome, "assigned" | "same_content") {
            take_over_existing_global_target(store, &skill, tool)
        } else {
            sync_global_target(
                store,
                &skill,
                tool,
                outcome == "conflict" && replace_conflicts.contains(&key),
            )
        };
        match result {
            Ok(()) => {
                upsert_assignment(
                    store, &skill.id, tool_key, "global", None, "hub", "active", None,
                )?;
                summary.succeeded += 1;
            }
            Err(err) => {
                let message = format!("{err:#}");
                persist_assignment_error(store, &skill.id, tool_key, "global", None, &message)?;
                summary.failed += 1;
                summary.errors.push(format!("{}: {message}", tool.label));
            }
        }
    }

    apply_project_assignments(
        store,
        &skill,
        &normalized,
        tools,
        replace_conflicts,
        additive.then_some(&requested_keys),
        &mut summary,
    )?;
    remove_obsolete_assignment_records(store, skill_id, &desired_keys)?;
    Ok(summary)
}

fn apply_project_assignments(
    store: &SkillStore,
    skill: &SkillRecord,
    draft: &AssignmentDraft,
    tools: &[AssignmentToolSpec],
    replace_conflicts: &HashSet<String>,
    requested_keys: Option<&HashSet<String>>,
    summary: &mut AssignmentApplySummary,
) -> Result<()> {
    let existing_project_ids = store
        .list_skill_assignments(&skill.id)?
        .into_iter()
        .filter(|item| item.scope == "project")
        .filter_map(|item| item.project_id)
        .collect::<BTreeSet<_>>();
    let requested = draft
        .projects
        .iter()
        .map(|project| (project.project_id.as_str(), project))
        .collect::<HashMap<_, _>>();
    let mut project_ids = existing_project_ids;
    project_ids.extend(
        draft
            .projects
            .iter()
            .map(|project| project.project_id.clone()),
    );
    let tool_specs = tools
        .iter()
        .filter(|tool| tool.supports_project_scope && !tool.project_skills_dir.is_empty())
        .map(|tool| ProjectToolSpec::new(&tool.key, &tool.project_skills_dir))
        .collect::<Vec<_>>();

    for project_id in project_ids {
        let desired = requested
            .get(project_id.as_str())
            .map(|project| project.tool_keys.clone())
            .unwrap_or_default();
        let requested_for_project = desired
            .iter()
            .filter(|tool_key| {
                let Some(keys) = requested_keys else {
                    return true;
                };
                keys.contains(&target_key("project", Some(&project_id), tool_key))
            })
            .cloned()
            .collect::<Vec<_>>();
        if requested_keys.is_some() && requested_for_project.is_empty() {
            continue;
        }
        if !skill.enabled {
            save_suspended_project_assignment(store, skill, &project_id, &desired)?;
            summary.succeeded += requested_for_project.len();
            continue;
        }
        let Some(project) = store.get_project(&project_id)? else {
            for tool_key in &requested_for_project {
                persist_assignment_error(
                    store,
                    &skill.id,
                    tool_key,
                    "project",
                    Some(&project_id),
                    "project is unavailable",
                )?;
                summary.failed += 1;
            }
            if !requested_for_project.is_empty() {
                summary
                    .errors
                    .push(format!("{project_id}: project is unavailable"));
            }
            continue;
        };
        let available = requested_for_project
            .iter()
            .filter(|key| {
                project.enabled_tool_keys.contains(*key)
                    && tools.iter().any(|tool| {
                        tool.key == **key && tool.enabled && tool.supports_project_scope
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        let overwrite_tools = available
            .iter()
            .filter(|tool_key| {
                replace_conflicts.contains(&target_key("project", Some(&project_id), tool_key))
            })
            .cloned()
            .collect::<HashSet<_>>();
        let outcome = if requested_keys.is_some() {
            add_project_skill_tools_and_scan(
                store,
                &project_id,
                &skill.id,
                &desired,
                &available,
                &tool_specs,
                &overwrite_tools,
            )
        } else {
            assign_project_skill_tools_and_scan(
                store,
                &project_id,
                &skill.id,
                &available,
                &tool_specs,
                &overwrite_tools,
            )
        };
        match outcome {
            Ok(project_summary) => {
                persist_project_outcomes(
                    store,
                    skill,
                    &project_id,
                    &available,
                    &project_summary,
                    summary,
                )?;
            }
            Err(err) => {
                let message = format!("{err:#}");
                for tool_key in &available {
                    persist_assignment_error(
                        store,
                        &skill.id,
                        tool_key,
                        "project",
                        Some(&project_id),
                        &message,
                    )?;
                    summary.failed += 1;
                }
                if !available.is_empty() {
                    summary.errors.push(format!("{}: {message}", project.name));
                }
            }
        }
        for tool_key in requested_for_project
            .iter()
            .filter(|key| !available.contains(key))
        {
            persist_assignment_error(
                store,
                &skill.id,
                tool_key,
                "project",
                Some(&project_id),
                "project tool is unavailable",
            )?;
            summary.failed += 1;
            summary.errors.push(format!(
                "{} / {}: project tool is unavailable",
                project.name, tool_key
            ));
        }
    }
    Ok(())
}

fn persist_project_outcomes(
    store: &SkillStore,
    skill: &SkillRecord,
    project_id: &str,
    desired: &[String],
    project_summary: &ProjectInjectionSummary,
    summary: &mut AssignmentApplySummary,
) -> Result<()> {
    let project = store
        .get_project(project_id)?
        .with_context(|| format!("project not found: {project_id}"))?;
    let targets = store.list_project_skill_targets(&project.root_path)?;
    let discovered = store.list_project_discovered_skills(project_id)?;
    let link = store.get_project_skill_link(project_id, &skill.id)?;
    for tool_key in desired {
        let hub = targets.iter().any(|target| {
            target.skill_id == skill.id && target.tool == *tool_key && target.status == "ok"
        });
        let native = discovered.iter().any(|item| {
            item.matched_skill_id.as_deref() == Some(skill.id.as_str())
                && item.tool_keys.contains(tool_key)
        });
        let active = hub || native;
        let error = if active {
            None
        } else {
            Some(
                link.as_ref()
                    .map(|value| value.status.as_str())
                    .unwrap_or("assignment failed"),
            )
        };
        upsert_assignment(
            store,
            &skill.id,
            tool_key,
            "project",
            Some(project_id),
            if hub { "hub" } else { "native" },
            if active { "active" } else { "error" },
            error,
        )?;
        if active {
            summary.succeeded += 1;
        } else {
            summary.failed += 1;
        }
    }
    if project_summary.failed > 0 && desired.is_empty() {
        summary.failed += project_summary.failed;
    }
    Ok(())
}

fn save_suspended_project_assignment(
    store: &SkillStore,
    skill: &SkillRecord,
    project_id: &str,
    desired: &[String],
) -> Result<()> {
    let now = now_ms();
    if desired.is_empty() {
        if let Some(mut link) = store.get_project_skill_link(project_id, &skill.id)? {
            link.enabled = false;
            link.desired_tools.clear();
            link.status = "disabled".to_string();
            link.updated_at = now;
            store.upsert_project_skill_link(&link)?;
        }
        return Ok(());
    }
    let mut link = store
        .get_project_skill_link(project_id, &skill.id)?
        .unwrap_or(ProjectSkillLinkRecord {
            project_id: project_id.to_string(),
            skill_id: skill.id.clone(),
            enabled: true,
            desired_tools: Vec::new(),
            status: "suspended".to_string(),
            created_at: now,
            updated_at: now,
        });
    link.enabled = true;
    link.desired_tools = desired.to_vec();
    link.status = "suspended".to_string();
    link.updated_at = now;
    store.upsert_project_skill_link(&link)?;
    for tool_key in desired {
        upsert_assignment(
            store,
            &skill.id,
            tool_key,
            "project",
            Some(project_id),
            "hub",
            "suspended",
            None,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn preview_target(
    store: &SkillStore,
    skill: &SkillRecord,
    tool: &AssignmentToolSpec,
    scope: &str,
    project_id: Option<&str>,
    project_name: Option<&str>,
    target: PathBuf,
    available: bool,
) -> Result<AssignmentPreviewItem> {
    let key = target_key(scope, project_id, &tool.key);
    if !available {
        return Ok(AssignmentPreviewItem {
            key,
            tool: tool.key.clone(),
            scope: scope.to_string(),
            project_id: project_id.map(str::to_string),
            project_name: project_name.map(str::to_string),
            target_path: Some(target.to_string_lossy().to_string()),
            outcome: "unavailable".to_string(),
            blocking: true,
        });
    }
    let project_path = project_id
        .and_then(|id| store.get_project(id).transpose())
        .transpose()?
        .map(|project| project.root_path);
    let existing = store.get_skill_target(&skill.id, &tool.key, scope, project_path.as_deref())?;
    let target_exists = std::fs::symlink_metadata(&target).is_ok();
    let outcome = if !target_exists {
        "ready"
    } else if target_has_same_content(Path::new(&skill.central_path), &target) {
        if existing.as_ref().is_some_and(|record| {
            record.status == "ok" && record.target_path == target.to_string_lossy()
        }) {
            "assigned"
        } else {
            "same_content"
        }
    } else {
        "conflict"
    };
    Ok(AssignmentPreviewItem {
        key,
        tool: tool.key.clone(),
        scope: scope.to_string(),
        project_id: project_id.map(str::to_string),
        project_name: project_name.map(str::to_string),
        target_path: Some(target.to_string_lossy().to_string()),
        outcome: outcome.to_string(),
        blocking: outcome == "conflict",
    })
}

fn unavailable_preview(
    tool: &str,
    scope: &str,
    project_id: Option<String>,
    project_name: Option<String>,
) -> AssignmentPreviewItem {
    AssignmentPreviewItem {
        key: target_key(scope, project_id.as_deref(), tool),
        tool: tool.to_string(),
        scope: scope.to_string(),
        project_id,
        project_name,
        target_path: None,
        outcome: "unavailable".to_string(),
        blocking: true,
    }
}

fn evaluate_assignment(
    store: &SkillStore,
    skill: &SkillRecord,
    assignment: &SkillAssignmentRecord,
    tool: Option<&AssignmentToolSpec>,
    source_hash: Option<&str>,
) -> Result<AssignmentEvaluation> {
    let source = Path::new(&skill.central_path);
    if !source.is_dir() {
        return Ok(invalid_assignment(
            "source_missing",
            "library skill directory is missing",
            None,
            Some(skill.central_path.clone()),
        ));
    }
    if !source.join("SKILL.md").is_file() || source_hash.is_none() || skill.status != "ok" {
        return Ok(invalid_assignment(
            "source_invalid",
            "library skill is invalid",
            None,
            Some(skill.central_path.clone()),
        ));
    }
    let Some(tool) = tool else {
        return Ok(invalid_assignment(
            "tool_unavailable",
            "agent is no longer configured",
            None,
            None,
        ));
    };
    if !tool.enabled {
        return Ok(invalid_assignment(
            "tool_unavailable",
            "agent is disabled",
            None,
            None,
        ));
    }

    let (project, expected_target, project_path) = match assignment.scope.as_str() {
        "global" if assignment.project_id.is_none() => {
            if !tool.global_available {
                return Ok(invalid_assignment(
                    "tool_unavailable",
                    "agent is not currently available",
                    None,
                    None,
                ));
            }
            (None, tool.global_skills_dir.join(&skill.name), None)
        }
        "project" => {
            let Some(project_id) = assignment.project_id.as_deref() else {
                return Ok(invalid_assignment(
                    "invalid_scope",
                    "project assignment has no project",
                    None,
                    None,
                ));
            };
            let Some(project) = store.get_project(project_id)? else {
                return Ok(invalid_assignment(
                    "project_unavailable",
                    "project is no longer imported",
                    None,
                    None,
                ));
            };
            if !Path::new(&project.root_path).is_dir() {
                return Ok(invalid_assignment(
                    "project_unavailable",
                    "project directory is missing",
                    None,
                    Some(project.root_path),
                ));
            }
            if !tool.supports_project_scope
                || tool.project_skills_dir.trim().is_empty()
                || !project.enabled_tool_keys.contains(&assignment.tool)
            {
                return Ok(invalid_assignment(
                    "project_tool_unavailable",
                    "agent is unavailable for this project",
                    None,
                    None,
                ));
            }
            let relative = Path::new(&tool.project_skills_dir);
            if relative.is_absolute()
                || relative.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir | std::path::Component::RootDir
                    )
                })
            {
                return Ok(invalid_assignment(
                    "unsafe_target",
                    "agent project path is unsafe",
                    None,
                    None,
                ));
            }
            let expected = Path::new(&project.root_path)
                .join(relative)
                .join(&skill.name);
            let project_path = Some(project.root_path.clone());
            (Some(project), expected, project_path)
        }
        _ => {
            return Ok(invalid_assignment(
                "invalid_scope",
                "assignment scope is invalid",
                None,
                None,
            ));
        }
    };

    if !skill.enabled {
        return if assignment.state == "error" {
            Ok(invalid_assignment(
                assignment_error_kind(assignment),
                assignment
                    .last_error
                    .as_deref()
                    .unwrap_or("saved assignment is invalid"),
                None,
                None,
            ))
        } else {
            Ok(valid_assignment())
        };
    }
    if assignment.state == "suspended" {
        return Ok(invalid_assignment(
            "resume_required",
            "assignment must be resumed",
            None,
            None,
        ));
    }

    if assignment.ownership == "native" {
        let Some(project) = project.as_ref() else {
            return Ok(invalid_assignment(
                "unsafe_ownership",
                "native assignment is only valid in a project",
                None,
                None,
            ));
        };
        let matching = store
            .list_project_discovered_skills(&project.id)?
            .into_iter()
            .find(|item| {
                item.matched_skill_id.as_deref() == Some(skill.id.as_str())
                    && item.tool_keys.contains(&assignment.tool)
                    && paths_refer_to_same_location(
                        Path::new(&item.absolute_path),
                        &expected_target,
                    )
                    && item.content_hash.as_deref() == source_hash
                    && Path::new(&item.absolute_path).is_dir()
            });
        return if matching.is_some() {
            Ok(valid_assignment())
        } else if expected_target.is_dir()
            && hash_resolved_dir(&expected_target).ok().as_deref() != source_hash
        {
            Ok(invalid_assignment(
                "content_conflict",
                "project skill content differs from the library",
                None,
                Some(expected_target.to_string_lossy().to_string()),
            ))
        } else {
            Ok(invalid_assignment(
                "native_missing",
                "matching project skill is missing",
                Some("tool_added_pending_sync"),
                Some(expected_target.to_string_lossy().to_string()),
            ))
        };
    }
    if assignment.ownership != "hub" {
        return Ok(invalid_assignment(
            "unsafe_ownership",
            "assignment ownership is invalid",
            None,
            None,
        ));
    }

    let target = store.get_skill_target(
        &skill.id,
        &assignment.tool,
        &assignment.scope,
        project_path.as_deref(),
    )?;
    let Some(target) = target else {
        if expected_target.is_dir()
            && hash_resolved_dir(&expected_target).ok().as_deref() != source_hash
        {
            return Ok(invalid_assignment(
                "content_conflict",
                "target content differs from the library",
                None,
                Some(expected_target.to_string_lossy().to_string()),
            ));
        }
        return Ok(invalid_assignment(
            "target_missing",
            "managed target is missing",
            (assignment.scope == "project").then_some("tool_added_pending_sync"),
            Some(expected_target.to_string_lossy().to_string()),
        ));
    };
    if !paths_refer_to_same_location(Path::new(&target.target_path), &expected_target) {
        return Ok(invalid_assignment(
            "unsafe_target",
            "managed target path does not match the assignment",
            None,
            Some(target.target_path),
        ));
    }
    if std::fs::symlink_metadata(&expected_target).is_err() || !expected_target.is_dir() {
        return Ok(invalid_assignment(
            "target_missing",
            "managed target is missing",
            (assignment.scope == "project").then_some("target_missing"),
            Some(target.target_path),
        ));
    }
    let target_hash = hash_resolved_dir(&expected_target).ok();
    if target_hash.as_deref() != source_hash {
        return Ok(invalid_assignment(
            "content_conflict",
            "managed target content differs from the library",
            None,
            Some(target.target_path),
        ));
    }
    if target.status == "error" {
        return Ok(invalid_assignment(
            "target_error",
            target
                .last_error
                .as_deref()
                .or(assignment.last_error.as_deref())
                .unwrap_or("managed target failed"),
            (assignment.scope == "project").then_some("injection_failed"),
            Some(target.target_path),
        ));
    }
    Ok(valid_assignment())
}

fn valid_assignment() -> AssignmentEvaluation {
    AssignmentEvaluation {
        valid: true,
        issue_kind: None,
        message: None,
        project_issue_kind: None,
        project_issue_path: None,
    }
}

fn invalid_assignment(
    kind: &'static str,
    message: &str,
    project_issue_kind: Option<&'static str>,
    project_issue_path: Option<String>,
) -> AssignmentEvaluation {
    AssignmentEvaluation {
        valid: false,
        issue_kind: Some(kind),
        message: Some(message.to_string()),
        project_issue_kind,
        project_issue_path,
    }
}

fn assignment_error_kind(assignment: &SkillAssignmentRecord) -> &'static str {
    let error = assignment
        .last_error
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    if error.contains("conflict") || error.contains("different content") {
        "content_conflict"
    } else if error.contains("unsafe") || error.contains("outside") {
        "unsafe_target"
    } else {
        "target_error"
    }
}

fn assignment_issue_kind_can_ignore(kind: &str) -> bool {
    matches!(
        kind,
        "tool_unavailable"
            | "project_unavailable"
            | "project_tool_unavailable"
            | "target_missing"
            | "target_error"
            | "native_missing"
            | "resume_required"
    )
}

#[allow(clippy::too_many_arguments)]
fn assignment_issue_fingerprint(
    store: &SkillStore,
    skill: &SkillRecord,
    assignment: &SkillAssignmentRecord,
    evaluation: &AssignmentEvaluation,
    tool: Option<&AssignmentToolSpec>,
    source_hash: Option<&str>,
    issue_key: &str,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(issue_key.as_bytes());
    hasher.update(evaluation.issue_kind.unwrap_or_default().as_bytes());
    hasher.update(evaluation.message.as_deref().unwrap_or_default().as_bytes());
    hasher.update(skill.status.as_bytes());
    hasher.update([skill.enabled as u8]);
    hasher.update(source_hash.unwrap_or_default().as_bytes());
    hasher.update(assignment.ownership.as_bytes());
    hasher.update(assignment.state.as_bytes());
    hasher.update(
        assignment
            .last_error
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    if let Some(tool) = tool {
        hasher.update([tool.enabled as u8, tool.global_available as u8]);
        hasher.update(tool.global_skills_dir.to_string_lossy().as_bytes());
        hasher.update(tool.project_skills_dir.as_bytes());
    }
    if let Some(project_id) = assignment.project_id.as_deref() {
        if let Some(project) = store.get_project(project_id)? {
            hasher.update(project.root_path.as_bytes());
            for key in project.enabled_tool_keys {
                hasher.update(key.as_bytes());
                hasher.update([0]);
            }
        }
    }
    let project_path = assignment
        .project_id
        .as_deref()
        .and_then(|project_id| store.get_project(project_id).transpose())
        .transpose()?
        .map(|project| project.root_path);
    if let Some(target) = store.get_skill_target(
        &skill.id,
        &assignment.tool,
        &assignment.scope,
        project_path.as_deref(),
    )? {
        hasher.update(target.target_path.as_bytes());
        hasher.update(target.status.as_bytes());
        hasher.update(target.last_error.unwrap_or_default().as_bytes());
        if let Ok(hash) = hash_resolved_dir(Path::new(&target.target_path)) {
            hasher.update(hash.as_bytes());
        }
    }
    if assignment.ownership == "native" {
        if let Some(project_id) = assignment.project_id.as_deref() {
            for discovered in store.list_project_discovered_skills(project_id)? {
                if discovered.matched_skill_id.as_deref() == Some(skill.id.as_str())
                    && discovered.tool_keys.contains(&assignment.tool)
                {
                    hasher.update(discovered.absolute_path.as_bytes());
                    hasher.update(discovered.status.as_bytes());
                    hasher.update(discovered.content_hash.unwrap_or_default().as_bytes());
                }
            }
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn matching_project_issue_ignored(
    store: &SkillStore,
    skill: &SkillRecord,
    assignment: &SkillAssignmentRecord,
    evaluation: &AssignmentEvaluation,
) -> Result<bool> {
    let (Some(project_id), Some(kind)) = (
        assignment.project_id.as_deref(),
        evaluation.project_issue_kind,
    ) else {
        return Ok(false);
    };
    let Some(project) = store.get_project(project_id)? else {
        return Ok(false);
    };
    let id = format!("{kind}:{}:{}", skill.id, assignment.tool);
    let issue = ProjectIssue {
        id: id.clone(),
        kind: kind.to_string(),
        skill_id: Some(skill.id.clone()),
        skill_name: Some(skill.name.clone()),
        tool_key: Some(assignment.tool.clone()),
        path: evaluation.project_issue_path.clone(),
        actions: Vec::new(),
    };
    let fingerprint = project_issue_fingerprint(store, &project, &issue)?;
    store.is_issue_ignored("project", Some(&project.id), &id, &fingerprint)
}

fn assignment_draft_from_records(assignments: &[SkillAssignmentRecord]) -> AssignmentDraft {
    let mut global_tools = Vec::new();
    let mut projects = BTreeMap::<String, Vec<String>>::new();
    for assignment in assignments {
        if assignment.scope == "global" {
            global_tools.push(assignment.tool.clone());
        } else if let Some(project_id) = assignment.project_id.as_deref() {
            projects
                .entry(project_id.to_string())
                .or_default()
                .push(assignment.tool.clone());
        }
    }
    AssignmentDraft {
        global_tools,
        projects: projects
            .into_iter()
            .map(|(project_id, tool_keys)| ProjectAssignmentDraft {
                project_id,
                tool_keys,
            })
            .collect(),
    }
}

fn normalized_draft(draft: &AssignmentDraft, tools: &[AssignmentToolSpec]) -> AssignmentDraft {
    let mut global_tools = normalized_keys(&draft.global_tools);
    let selected_paths = global_tools
        .iter()
        .filter_map(|key| tools.iter().find(|tool| tool.key == *key))
        .map(|tool| tool.global_skills_dir.clone())
        .collect::<HashSet<_>>();
    for tool in tools {
        if tool.global_available && selected_paths.contains(&tool.global_skills_dir) {
            global_tools.push(tool.key.clone());
        }
    }
    global_tools = normalized_keys(&global_tools);

    let mut projects: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for project in &draft.projects {
        projects
            .entry(project.project_id.trim().to_string())
            .or_default()
            .extend(project.tool_keys.iter().cloned());
    }
    AssignmentDraft {
        global_tools,
        projects: projects
            .into_iter()
            .filter(|(project_id, _)| !project_id.is_empty())
            .map(|(project_id, tool_keys)| ProjectAssignmentDraft {
                project_id,
                tool_keys: normalized_keys(&tool_keys),
            })
            .collect(),
    }
}

fn merge_existing_assignments(
    store: &SkillStore,
    skill_id: &str,
    draft: &mut AssignmentDraft,
) -> Result<()> {
    let mut global = draft.global_tools.iter().cloned().collect::<BTreeSet<_>>();
    let mut projects = draft
        .projects
        .iter()
        .map(|project| {
            (
                project.project_id.clone(),
                project.tool_keys.iter().cloned().collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for assignment in store.list_skill_assignments(skill_id)? {
        if assignment.scope == "global" {
            global.insert(assignment.tool);
        } else if let Some(project_id) = assignment.project_id {
            projects
                .entry(project_id)
                .or_default()
                .insert(assignment.tool);
        }
    }
    draft.global_tools = global.into_iter().collect();
    draft.projects = projects
        .into_iter()
        .map(|(project_id, tools)| ProjectAssignmentDraft {
            project_id,
            tool_keys: tools.into_iter().collect(),
        })
        .collect();
    Ok(())
}

fn draft_target_keys(draft: &AssignmentDraft) -> HashSet<String> {
    let mut keys = draft
        .global_tools
        .iter()
        .map(|tool| target_key("global", None, tool))
        .collect::<HashSet<_>>();
    for project in &draft.projects {
        keys.extend(
            project
                .tool_keys
                .iter()
                .map(|tool| target_key("project", Some(&project.project_id), tool)),
        );
    }
    keys
}

fn remove_targets_from_draft(draft: &mut AssignmentDraft, targets: &HashSet<String>) {
    draft
        .global_tools
        .retain(|tool| !targets.contains(&target_key("global", None, tool)));
    for project in &mut draft.projects {
        project.tool_keys.retain(|tool| {
            !targets.contains(&target_key("project", Some(&project.project_id), tool))
        });
    }
}

fn remove_obsolete_global_assignments(
    store: &SkillStore,
    skill: &SkillRecord,
    existing: &[SkillAssignmentRecord],
    desired_keys: &HashSet<String>,
    tools: &HashMap<&str, &AssignmentToolSpec>,
) -> Result<()> {
    let desired_paths = desired_keys
        .iter()
        .filter_map(|key| key.strip_prefix("global|"))
        .filter_map(|tool| tools.get(tool))
        .map(|tool| tool.global_skills_dir.join(&skill.name))
        .collect::<HashSet<_>>();
    let mut removed_paths = HashSet::new();
    for assignment in existing.iter().filter(|assignment| {
        assignment.scope == "global"
            && !desired_keys.contains(&target_key("global", None, &assignment.tool))
    }) {
        if let Some(target) = store.get_skill_target(&skill.id, &assignment.tool, "global", None)? {
            let path = PathBuf::from(&target.target_path);
            if !desired_paths.contains(&path) && removed_paths.insert(path.clone()) {
                remove_global_target_if_owned(skill, &path)?;
            }
            store.delete_skill_target(&skill.id, &assignment.tool, "global", None)?;
        }
    }
    Ok(())
}

fn remove_obsolete_assignment_records(
    store: &SkillStore,
    skill_id: &str,
    desired_keys: &HashSet<String>,
) -> Result<()> {
    for assignment in store.list_skill_assignments(skill_id)? {
        let key = target_key(
            &assignment.scope,
            assignment.project_id.as_deref(),
            &assignment.tool,
        );
        if !desired_keys.contains(&key) {
            store.delete_skill_assignment(
                skill_id,
                &assignment.tool,
                &assignment.scope,
                assignment.project_id.as_deref(),
            )?;
        }
    }
    Ok(())
}

fn take_over_existing_global_target(
    store: &SkillStore,
    skill: &SkillRecord,
    tool: &AssignmentToolSpec,
) -> Result<()> {
    let target = tool.global_skills_dir.join(&skill.name);
    store.upsert_skill_target(&SkillTargetRecord {
        id: Uuid::new_v4().to_string(),
        skill_id: skill.id.clone(),
        tool: tool.key.clone(),
        scope: "global".to_string(),
        project_path: None,
        target_path: target.to_string_lossy().to_string(),
        mode: "existing".to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: Some(now_ms()),
    })
}

fn sync_global_target(
    store: &SkillStore,
    skill: &SkillRecord,
    tool: &AssignmentToolSpec,
    overwrite: bool,
) -> Result<()> {
    std::fs::create_dir_all(&tool.global_skills_dir)
        .with_context(|| format!("create skills directory {:?}", tool.global_skills_dir))?;
    let target = tool.global_skills_dir.join(&skill.name);
    let outcome = sync_dir_for_tool_with_overwrite(
        &tool.key,
        Path::new(&skill.central_path),
        &target,
        overwrite,
    )?;
    store.upsert_skill_target(&SkillTargetRecord {
        id: Uuid::new_v4().to_string(),
        skill_id: skill.id.clone(),
        tool: tool.key.clone(),
        scope: "global".to_string(),
        project_path: None,
        target_path: outcome.target_path.to_string_lossy().to_string(),
        mode: sync_mode_name(&outcome.mode_used).to_string(),
        status: "ok".to_string(),
        last_error: None,
        synced_at: Some(now_ms()),
    })
}

fn remove_global_target_if_owned(skill: &SkillRecord, target: &Path) -> Result<()> {
    let Ok(metadata) = std::fs::symlink_metadata(target) else {
        return Ok(());
    };
    let source = Path::new(&skill.central_path);
    let owned = if metadata.file_type().is_symlink() {
        std::fs::canonicalize(target).ok() == std::fs::canonicalize(source).ok()
    } else {
        target_has_same_content(source, target)
    };
    if !owned {
        return Ok(());
    }
    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(target)?;
    } else {
        std::fs::remove_dir_all(target)?;
    }
    Ok(())
}

fn persist_assignment_error(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
    scope: &str,
    project_id: Option<&str>,
    message: &str,
) -> Result<()> {
    upsert_assignment(
        store,
        skill_id,
        tool,
        scope,
        project_id,
        "hub",
        "error",
        Some(message),
    )
}

#[allow(clippy::too_many_arguments)]
fn upsert_assignment(
    store: &SkillStore,
    skill_id: &str,
    tool: &str,
    scope: &str,
    project_id: Option<&str>,
    ownership: &str,
    state: &str,
    last_error: Option<&str>,
) -> Result<()> {
    let now = now_ms();
    let existing = store.get_skill_assignment(skill_id, tool, scope, project_id)?;
    store.upsert_skill_assignment(&SkillAssignmentRecord {
        id: existing
            .as_ref()
            .map(|item| item.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        scope: scope.to_string(),
        project_id: project_id.map(str::to_string),
        ownership: ownership.to_string(),
        state: state.to_string(),
        last_error: last_error.map(str::to_string),
        created_at: existing.as_ref().map(|item| item.created_at).unwrap_or(now),
        updated_at: now,
    })
}

fn target_has_same_content(source: &Path, target: &Path) -> bool {
    if !source.is_dir() || !target.is_dir() {
        return false;
    }
    match (hash_resolved_dir(source), hash_resolved_dir(target)) {
        (Ok(source_hash), Ok(target_hash)) => source_hash == target_hash,
        _ => false,
    }
}

fn hash_resolved_dir(path: &Path) -> Result<String> {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    hash_dir(&resolved)
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> PathBuf {
        let Some(parent) = path.parent() else {
            return path.to_path_buf();
        };
        let parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
        path.file_name()
            .map(|name| parent.join(name))
            .unwrap_or(parent)
    }
    normalized(left) == normalized(right)
}

fn require_skill(store: &SkillStore, skill_id: &str) -> Result<SkillRecord> {
    store
        .get_skill_by_id(skill_id)?
        .with_context(|| format!("skill not found: {skill_id}"))
}

fn target_key(scope: &str, project_id: Option<&str>, tool: &str) -> String {
    match project_id {
        Some(project_id) => format!("{scope}|{project_id}|{tool}"),
        None => format!("{scope}|{tool}"),
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

fn sync_mode_name(mode: &SyncMode) -> &'static str {
    match mode {
        SyncMode::Auto => "auto",
        SyncMode::Symlink => "symlink",
        SyncMode::Junction => "junction",
        SyncMode::Copy => "copy",
    }
}

fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

#[cfg(test)]
#[path = "tests/assignments.rs"]
mod tests;
