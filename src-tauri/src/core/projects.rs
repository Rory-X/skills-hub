use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::content_hash::hash_dir;
use super::installer::{backfill_skill_content_hashes, install_local_skill};
use super::skill_store::{
    IgnoredIssueRecord, ProjectDiscoveredSkillRecord, ProjectRecord, ProjectSkillLinkRecord,
    SkillAssignmentRecord, SkillRecord, SkillStore, SkillTargetRecord,
};
use super::sync_engine::{
    sync_dir_copy_with_overwrite, sync_dir_for_tool_with_overwrite, SyncMode,
};

const GIT_EXCLUDE_BEGIN: &str = "# Skills Hub: begin";
const GIT_EXCLUDE_END: &str = "# Skills Hub: end";

enum ProjectConflictPolicy<'a> {
    Preserve,
    ReplaceAll,
    ReplaceTools(&'a HashSet<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectToolSpec {
    pub key: String,
    pub relative_skills_dir: String,
}

impl ProjectToolSpec {
    pub fn new(key: impl Into<String>, relative_skills_dir: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            relative_skills_dir: relative_skills_dir.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectIssue {
    pub id: String,
    pub kind: String,
    pub skill_id: Option<String>,
    pub skill_name: Option<String>,
    pub tool_key: Option<String>,
    pub path: Option<String>,
    pub actions: Vec<ProjectIssueAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectIssueAction {
    pub kind: String,
    pub value: Option<String>,
    pub label: Option<String>,
    pub confirmation_required: bool,
    pub destructive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectIssueProjection {
    pub issue: ProjectIssue,
    pub fingerprint: String,
    pub can_ignore: bool,
    pub ignored: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectInjectionSummary {
    pub injected: usize,
    pub matched: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSummaryData {
    pub skill_count: usize,
    pub issues: Vec<ProjectIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSkillPathView {
    pub discovered_skill_id: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub tool_keys: Vec<String>,
    pub content_hash: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSkillView {
    pub name: String,
    pub description: Option<String>,
    pub source: String,
    pub status: String,
    pub linked_skill_id: Option<String>,
    pub matched_skill_id: Option<String>,
    pub tool_keys: Vec<String>,
    pub paths: Vec<ProjectSkillPathView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectLibrarySkillView {
    pub skill_id: String,
    pub name: String,
    pub description: Option<String>,
    pub availability: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectDetailData {
    pub current_skills: Vec<ProjectSkillView>,
    pub library_skills: Vec<ProjectLibrarySkillView>,
    pub issues: Vec<ProjectIssue>,
}

pub fn create_or_get_project(
    store: &SkillStore,
    root: &Path,
    default_tool_keys: &[String],
) -> Result<ProjectRecord> {
    if !root.is_absolute() {
        anyhow::bail!("project path must be absolute: {:?}", root);
    }
    if !root.is_dir() {
        anyhow::bail!("project path must be an existing directory: {:?}", root);
    }
    let normalized = std::fs::canonicalize(root)
        .with_context(|| format!("normalize project path {:?}", root))?;
    let root_path = normalized.to_string_lossy().to_string();
    if let Some(project) = store.get_project_by_root(&root_path)? {
        return Ok(project);
    }

    let name = normalized
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| root_path.clone());
    let now = now_ms();
    let project = ProjectRecord {
        id: Uuid::new_v4().to_string(),
        name,
        root_path,
        enabled_tool_keys: normalized_keys(default_tool_keys),
        local_git_exclude_enabled: false,
        created_at: now,
        updated_at: now,
        last_scanned_at: None,
    };
    store.insert_project(&project)?;
    Ok(project)
}

pub fn update_project_configuration(
    store: &SkillStore,
    project_id: &str,
    enabled_tool_keys: &[String],
    local_git_exclude_enabled: bool,
) -> Result<ProjectRecord> {
    let mut project = require_project(store, project_id)?;
    project.enabled_tool_keys = normalized_keys(enabled_tool_keys);
    project.local_git_exclude_enabled = local_git_exclude_enabled;
    project.updated_at = now_ms();
    store.update_project(&project)?;
    Ok(project)
}

pub fn import_project_and_scan(
    store: &SkillStore,
    root: &Path,
    default_tool_keys: &[String],
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    validate_enabled_tool_specs(default_tool_keys, tool_specs)?;
    let project = create_or_get_project(store, root, default_tool_keys)?;
    scan_project(store, &project, tool_specs)?;
    require_project(store, &project.id)
}

pub fn refresh_project(
    store: &SkillStore,
    project_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let project = require_project(store, project_id)?;
    validate_enabled_tool_specs(&project.enabled_tool_keys, tool_specs)?;
    scan_project(store, &project, tool_specs)?;
    require_project(store, project_id)
}

pub fn update_project_and_scan(
    store: &SkillStore,
    project_id: &str,
    enabled_tool_keys: &[String],
    local_git_exclude_enabled: bool,
    available_tool_keys: &[String],
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let available = available_tool_keys.iter().collect::<HashSet<_>>();
    if let Some(unknown) = enabled_tool_keys
        .iter()
        .find(|key| !available.contains(key))
    {
        anyhow::bail!("project tool is not available: {unknown}");
    }
    validate_enabled_tool_specs(enabled_tool_keys, tool_specs)?;
    let project = update_project_configuration(
        store,
        project_id,
        enabled_tool_keys,
        local_git_exclude_enabled,
    )?;
    scan_project(store, &project, tool_specs)?;
    require_project(store, project_id)
}

pub fn inject_project_skill_and_scan(
    store: &SkillStore,
    project_id: &str,
    skill_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let project = require_project(store, project_id)?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .with_context(|| format!("skill not found: {skill_id}"))?;
    inject_project_skill(store, &project, &skill, tool_specs)?;
    scan_project(store, &project, tool_specs)?;
    require_project(store, project_id)
}

pub fn remove_project_skill_and_scan(
    store: &SkillStore,
    project_id: &str,
    skill_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let project = require_project(store, project_id)?;
    remove_project_skill(store, &project, skill_id)?;
    scan_project(store, &project, tool_specs)?;
    require_project(store, project_id)
}

pub fn repair_project_issue_and_scan(
    store: &SkillStore,
    project_id: &str,
    issue_id: &str,
    action_kind: &str,
    action_value: Option<&str>,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let project = require_project(store, project_id)?;
    scan_project(store, &project, tool_specs)?;
    let project = require_project(store, project_id)?;
    repair_project_issue(
        store,
        &project,
        tool_specs,
        issue_id,
        action_kind,
        action_value,
    )?;
    require_project(store, project_id)
}

pub fn collect_project_skill_and_scan<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    project_id: &str,
    discovered_skill_id: &str,
    library_name: Option<&str>,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let project = require_project(store, project_id)?;
    collect_project_skill(app, store, project_id, discovered_skill_id, library_name)?;
    scan_project(store, &project, tool_specs)?;
    require_project(store, project_id)
}

pub fn collect_project_skill<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
    project_id: &str,
    discovered_skill_id: &str,
    library_name: Option<&str>,
) -> Result<()> {
    let discovered = store
        .list_project_discovered_skills(project_id)?
        .into_iter()
        .find(|item| item.id == discovered_skill_id)
        .with_context(|| format!("project skill not found: {discovered_skill_id}"))?;
    if discovered.status == "invalid_missing_skill_md" {
        anyhow::bail!("project skill is invalid: {}", discovered.absolute_path);
    }
    if let Some(skill_id) = discovered.matched_skill_id.clone() {
        record_collected_project_skill(store, &discovered, &skill_id)?;
        return Ok(());
    }

    backfill_skill_content_hashes(store);
    let library = store.list_skills()?;
    let same_name = library
        .iter()
        .filter(|skill| skill.name.eq_ignore_ascii_case(&discovered.name))
        .collect::<Vec<_>>();
    let already_matched = same_name.iter().find(|skill| {
        skill.content_hash.is_some()
            && skill.content_hash.as_ref() == discovered.content_hash.as_ref()
    });
    if let Some(skill) = already_matched {
        record_collected_project_skill(store, &discovered, &skill.id)?;
        return Ok(());
    }

    let requested_name = library_name
        .map(validate_project_library_name)
        .transpose()?;
    if !same_name.is_empty() && requested_name.is_none() {
        anyhow::bail!("PROJECT_LIBRARY_NAME_REQUIRED|{}", discovered.name);
    }
    let installed = install_local_skill(
        app,
        store,
        Path::new(&discovered.absolute_path),
        requested_name,
    )?;
    record_collected_project_skill(store, &discovered, &installed.skill_id)?;
    Ok(())
}

fn record_collected_project_skill(
    store: &SkillStore,
    discovered: &ProjectDiscoveredSkillRecord,
    skill_id: &str,
) -> Result<()> {
    let now = now_ms();
    let mut desired_tools = store
        .list_project_discovered_skills(&discovered.project_id)?
        .into_iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(&discovered.name))
        .filter(|candidate| {
            discovered.content_hash.is_some() && candidate.content_hash == discovered.content_hash
        })
        .flat_map(|candidate| candidate.tool_keys)
        .collect::<Vec<_>>();
    let existing = store.get_project_skill_link(&discovered.project_id, skill_id)?;
    if let Some(link) = &existing {
        desired_tools.extend(link.desired_tools.iter().cloned());
    }
    let desired_tools = normalized_keys(&desired_tools);
    store.upsert_project_skill_link(&ProjectSkillLinkRecord {
        project_id: discovered.project_id.clone(),
        skill_id: skill_id.to_string(),
        enabled: true,
        desired_tools: desired_tools.clone(),
        status: "ok".to_string(),
        created_at: existing.as_ref().map(|link| link.created_at).unwrap_or(now),
        updated_at: now,
    })?;
    for tool_key in desired_tools {
        upsert_project_assignment(
            store,
            skill_id,
            &discovered.project_id,
            &tool_key,
            "native",
            "active",
            None,
        )?;
    }
    Ok(())
}

pub fn scan_project(
    store: &SkillStore,
    project: &ProjectRecord,
    tool_specs: &[ProjectToolSpec],
) -> Result<Vec<ProjectDiscoveredSkillRecord>> {
    let root = PathBuf::from(&project.root_path);
    if !root.is_dir() {
        anyhow::bail!("project path is no longer available: {}", project.root_path);
    }
    let enabled = project
        .enabled_tool_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut dirs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for spec in tool_specs {
        if !enabled.contains(spec.key.as_str()) || spec.relative_skills_dir.trim().is_empty() {
            continue;
        }
        validate_project_skills_dir(&spec.relative_skills_dir)?;
        dirs.entry(spec.relative_skills_dir.clone())
            .or_default()
            .push(spec.key.clone());
    }

    backfill_skill_content_hashes(store);
    let library = store.list_skills()?;
    let mut library_by_identity: HashMap<(String, String), String> = HashMap::new();
    for skill in library {
        if let Some(hash) = skill.content_hash {
            library_by_identity
                .entry((skill.name.to_lowercase(), hash))
                .or_insert(skill.id);
        }
    }

    let scanned_at = std::cmp::max(
        now_ms(),
        project
            .last_scanned_at
            .unwrap_or_default()
            .saturating_add(1),
    );
    let mut found: BTreeMap<String, ProjectDiscoveredSkillRecord> = BTreeMap::new();
    for (relative_dir, mut tool_keys) in dirs {
        tool_keys.sort();
        tool_keys.dedup();
        let skills_dir = resolve_project_skills_dir(project, &relative_dir)?;
        if !skills_dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&skills_dir)
            .with_context(|| format!("read project skills directory {:?}", skills_dir))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if !(file_type.is_dir() || (file_type.is_symlink() && path.is_dir())) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".system" {
                continue;
            }
            let absolute_path = path.to_string_lossy().to_string();
            if let Some(existing) = found.get_mut(&absolute_path) {
                existing.tool_keys.extend(tool_keys.iter().cloned());
                existing.tool_keys.sort();
                existing.tool_keys.dedup();
                continue;
            }
            let relative_path = slash_path(path.strip_prefix(&root).unwrap_or(&path));
            let valid = path.join("SKILL.md").is_file();
            let content_hash = if valid {
                let hash_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                Some(hash_dir(&hash_path).with_context(|| format!("hash skill {:?}", path))?)
            } else {
                None
            };
            let matched_skill_id = content_hash.as_ref().and_then(|hash| {
                library_by_identity
                    .get(&(name.to_lowercase(), hash.clone()))
                    .cloned()
            });
            found.insert(
                absolute_path.clone(),
                ProjectDiscoveredSkillRecord {
                    id: Uuid::new_v4().to_string(),
                    project_id: project.id.clone(),
                    name,
                    relative_path,
                    absolute_path,
                    content_hash,
                    tool_keys: tool_keys.clone(),
                    status: if valid {
                        if matched_skill_id.is_some() {
                            "matched".to_string()
                        } else {
                            "valid".to_string()
                        }
                    } else {
                        "invalid_missing_skill_md".to_string()
                    },
                    matched_skill_id,
                    first_seen_at: scanned_at,
                    last_seen_at: scanned_at,
                },
            );
        }
    }
    let records = found.into_values().collect::<Vec<_>>();
    store.replace_project_discovered_skills(&project.id, scanned_at, &records)?;
    let records = store.list_project_discovered_skills(&project.id)?;
    reconcile_matched_native_assignments(store, project, &records)?;
    Ok(records)
}

fn reconcile_matched_native_assignments(
    store: &SkillStore,
    project: &ProjectRecord,
    records: &[ProjectDiscoveredSkillRecord],
) -> Result<()> {
    let mut tools_by_skill = BTreeMap::<String, BTreeSet<String>>::new();
    for record in records {
        let Some(skill_id) = record.matched_skill_id.as_deref() else {
            continue;
        };
        tools_by_skill
            .entry(skill_id.to_string())
            .or_default()
            .extend(record.tool_keys.iter().cloned());
    }
    for (skill_id, discovered_tools) in tools_by_skill {
        let existing_link = store.get_project_skill_link(&project.id, &skill_id)?;
        if existing_link.as_ref().is_some_and(|link| !link.enabled) {
            continue;
        }
        let desired_tools = existing_link
            .as_ref()
            .map(|link| link.desired_tools.clone())
            .unwrap_or_else(|| discovered_tools.iter().cloned().collect());
        if existing_link.is_none() {
            let now = now_ms();
            store.upsert_project_skill_link(&ProjectSkillLinkRecord {
                project_id: project.id.clone(),
                skill_id: skill_id.clone(),
                enabled: true,
                desired_tools: desired_tools.clone(),
                status: "ok".to_string(),
                created_at: now,
                updated_at: now,
            })?;
        }
        for tool_key in desired_tools
            .iter()
            .filter(|tool| discovered_tools.contains(*tool))
        {
            let existing =
                store.get_skill_assignment(&skill_id, tool_key, "project", Some(&project.id))?;
            if existing
                .as_ref()
                .is_some_and(|assignment| assignment.ownership == "hub")
            {
                let target = store.get_skill_target(
                    &skill_id,
                    tool_key,
                    "project",
                    Some(&project.root_path),
                )?;
                if target.is_some() {
                    continue;
                }
            }
            upsert_project_assignment(
                store,
                &skill_id,
                &project.id,
                tool_key,
                "native",
                "active",
                None,
            )?;
        }
    }
    Ok(())
}

pub fn diagnose_project(
    store: &SkillStore,
    project: &ProjectRecord,
    tool_specs: &[ProjectToolSpec],
) -> Result<Vec<ProjectIssue>> {
    let discovered = store.list_project_discovered_skills(&project.id)?;
    let links = store
        .list_project_skill_links(&project.id)?
        .into_iter()
        .filter(|link| link.enabled)
        .collect::<Vec<_>>();
    let library = store.list_skills()?;
    let targets = store.list_project_skill_targets(&project.root_path)?;
    let mut issues = Vec::new();
    let live_hashes = discovered
        .iter()
        .map(|item| (item.id.clone(), current_project_skill_hash(item)))
        .collect::<HashMap<_, _>>();

    let library_by_name = group_library_by_name(&library);
    let linked_skill_ids = links
        .iter()
        .map(|link| link.skill_id.as_str())
        .collect::<HashSet<_>>();
    let mut discovered_by_name: BTreeMap<String, Vec<&ProjectDiscoveredSkillRecord>> =
        BTreeMap::new();
    for item in &discovered {
        discovered_by_name
            .entry(item.name.to_lowercase())
            .or_default()
            .push(item);
        if item.status == "invalid_missing_skill_md" {
            issues.push(ProjectIssue {
                id: format!("invalid_skill:{}", item.id),
                kind: "invalid_skill".to_string(),
                skill_id: None,
                skill_name: Some(item.name.clone()),
                tool_key: None,
                path: Some(item.absolute_path.clone()),
                actions: vec![project_issue_action(
                    "delete_invalid_skill",
                    None,
                    None,
                    true,
                    true,
                )],
            });
        }
    }
    for (normalized_name, variants) in &discovered_by_name {
        let hashes = variants
            .iter()
            .filter_map(|item| live_hashes.get(&item.id).and_then(Option::as_deref))
            .collect::<BTreeSet<_>>();
        if hashes.len() > 1 {
            let mut seen_hashes = HashSet::new();
            let actions = variants
                .iter()
                .filter_map(|item| {
                    let hash = live_hashes.get(&item.id)?.as_ref()?;
                    if !seen_hashes.insert(hash) {
                        return None;
                    }
                    Some(project_issue_action(
                        "keep_project_variant",
                        Some(item.id.clone()),
                        Some(item.relative_path.clone()),
                        true,
                        true,
                    ))
                })
                .collect();
            issues.push(ProjectIssue {
                id: format!("same_project_name_content_mismatch:{normalized_name}"),
                kind: "same_project_name_content_mismatch".to_string(),
                skill_id: None,
                skill_name: variants.first().map(|item| item.name.clone()),
                tool_key: None,
                path: None,
                actions,
            });
        }
        if let Some(library_variants) = library_by_name.get(normalized_name) {
            let linked_library_variants = library_variants
                .iter()
                .copied()
                .filter(|skill| linked_skill_ids.contains(skill.id.as_str()))
                .collect::<Vec<_>>();
            let all_library_hashes_known = !linked_library_variants.is_empty()
                && linked_library_variants
                    .iter()
                    .all(|skill| skill.content_hash.is_some());
            let any_match = variants.iter().any(|item| {
                live_hashes.get(&item.id).is_some_and(|live_hash| {
                    live_hash.as_ref().is_some_and(|hash| {
                        linked_library_variants
                            .iter()
                            .any(|skill| skill.content_hash.as_ref() == Some(hash))
                    })
                })
            });
            if all_library_hashes_known
                && !any_match
                && variants
                    .iter()
                    .any(|item| live_hashes.get(&item.id).is_some_and(Option::is_some))
            {
                issues.push(ProjectIssue {
                    id: format!("same_name_content_conflict:{normalized_name}"),
                    kind: "same_name_content_conflict".to_string(),
                    skill_id: linked_library_variants
                        .first()
                        .map(|skill| skill.id.clone()),
                    skill_name: variants.first().map(|item| item.name.clone()),
                    tool_key: None,
                    path: variants.first().map(|item| item.absolute_path.clone()),
                    actions: vec![
                        project_issue_action("replace_with_library", None, None, true, true),
                        project_issue_action("keep_project_version", None, None, true, true),
                    ],
                });
            }
        }
    }

    let tools = tool_specs
        .iter()
        .map(|tool| (tool.key.as_str(), tool))
        .collect::<HashMap<_, _>>();
    let enabled_project_tools = project
        .enabled_tool_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let target_by_skill_tool = targets
        .iter()
        .map(|target| ((target.skill_id.as_str(), target.tool.as_str()), target))
        .collect::<HashMap<_, _>>();
    let skills_by_id = library
        .iter()
        .map(|skill| (skill.id.as_str(), skill))
        .collect::<HashMap<_, _>>();
    for link in &links {
        let Some(skill) = skills_by_id.get(link.skill_id.as_str()) else {
            continue;
        };
        let skill_name = Some(skill.name.clone());
        if !skill.enabled || !Path::new(&skill.central_path).is_dir() {
            let mut actions = Vec::new();
            if !skill.enabled && Path::new(&skill.central_path).is_dir() {
                actions.push(project_issue_action(
                    "enable_library_skill",
                    None,
                    None,
                    false,
                    false,
                ));
            }
            actions.push(project_issue_action(
                "remove_unavailable_library_skill",
                None,
                None,
                true,
                true,
            ));
            issues.push(ProjectIssue {
                id: format!("library_skill_unavailable:{}", link.skill_id),
                kind: "library_skill_unavailable".to_string(),
                skill_id: Some(link.skill_id.clone()),
                skill_name,
                tool_key: None,
                path: Some(skill.central_path.clone()),
                actions,
            });
            continue;
        }
        for tool_key in &link.desired_tools {
            if !tools.contains_key(tool_key.as_str())
                || !enabled_project_tools.contains(tool_key.as_str())
            {
                continue;
            }
            match target_by_skill_tool.get(&(link.skill_id.as_str(), tool_key.as_str())) {
                None if discovered.iter().any(|item| {
                    item.matched_skill_id.as_deref() == Some(link.skill_id.as_str())
                        && item.tool_keys.contains(tool_key)
                        && !targets.iter().any(|target| {
                            target.skill_id == link.skill_id
                                && target.target_path == item.absolute_path
                        })
                }) => {}
                None => issues.push(ProjectIssue {
                    id: format!("tool_added_pending_sync:{}:{}", link.skill_id, tool_key),
                    kind: "tool_added_pending_sync".to_string(),
                    skill_id: Some(link.skill_id.clone()),
                    skill_name: skill_name.clone(),
                    tool_key: Some(tool_key.clone()),
                    path: None,
                    actions: vec![project_issue_action(
                        "retry_injection",
                        None,
                        None,
                        false,
                        false,
                    )],
                }),
                Some(target) if target.status == "error" => issues.push(ProjectIssue {
                    id: format!("injection_failed:{}:{}", link.skill_id, tool_key),
                    kind: "injection_failed".to_string(),
                    skill_id: Some(link.skill_id.clone()),
                    skill_name: skill_name.clone(),
                    tool_key: Some(tool_key.clone()),
                    path: Some(target.target_path.clone()),
                    actions: vec![project_issue_action(
                        "retry_injection",
                        None,
                        None,
                        false,
                        false,
                    )],
                }),
                Some(target) if std::fs::symlink_metadata(&target.target_path).is_err() => {
                    issues.push(ProjectIssue {
                        id: format!("target_missing:{}:{}", link.skill_id, tool_key),
                        kind: "target_missing".to_string(),
                        skill_id: Some(link.skill_id.clone()),
                        skill_name: skill_name.clone(),
                        tool_key: Some(tool_key.clone()),
                        path: Some(target.target_path.clone()),
                        actions: vec![project_issue_action(
                            "retry_injection",
                            None,
                            None,
                            false,
                            false,
                        )],
                    });
                }
                _ => {}
            }
        }
    }

    let desired_by_skill = links
        .iter()
        .map(|link| {
            (
                link.skill_id.as_str(),
                link.desired_tools
                    .iter()
                    .filter(|tool| enabled_project_tools.contains(tool.as_str()))
                    .map(String::as_str)
                    .collect::<HashSet<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    for target in &targets {
        let is_desired = desired_by_skill
            .get(target.skill_id.as_str())
            .is_some_and(|tools| tools.contains(target.tool.as_str()));
        if !is_desired {
            issues.push(ProjectIssue {
                id: format!(
                    "tool_removed_pending_unsync:{}:{}",
                    target.skill_id, target.tool
                ),
                kind: "tool_removed_pending_unsync".to_string(),
                skill_id: Some(target.skill_id.clone()),
                skill_name: skills_by_id
                    .get(target.skill_id.as_str())
                    .map(|skill| skill.name.clone()),
                tool_key: Some(target.tool.clone()),
                path: Some(target.target_path.clone()),
                actions: vec![project_issue_action(
                    "remove_stale_target",
                    None,
                    None,
                    true,
                    true,
                )],
            });
        }
    }

    match git_exclude_state(project, &targets)? {
        GitExcludeState::Clean => {}
        GitExcludeState::Drift(path) => issues.push(ProjectIssue {
            id: "git_exclude_drift".to_string(),
            kind: "git_exclude_drift".to_string(),
            skill_id: None,
            skill_name: None,
            tool_key: None,
            path: Some(path.to_string_lossy().to_string()),
            actions: vec![project_issue_action(
                "repair_git_exclude",
                None,
                None,
                false,
                false,
            )],
        }),
        GitExcludeState::Unavailable { can_initialize } => {
            let mut actions = vec![project_issue_action(
                "disable_git_exclude",
                None,
                None,
                true,
                false,
            )];
            if can_initialize {
                actions.push(project_issue_action(
                    "initialize_git_repository",
                    None,
                    None,
                    true,
                    true,
                ));
            }
            issues.push(ProjectIssue {
                id: "git_exclude_unavailable".to_string(),
                kind: "git_exclude_unavailable".to_string(),
                skill_id: None,
                skill_name: None,
                tool_key: None,
                path: None,
                actions,
            });
        }
    }

    issues.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.id.cmp(&b.id)));
    Ok(issues)
}

pub fn project_issue_projections(
    store: &SkillStore,
    project: &ProjectRecord,
    issues: Vec<ProjectIssue>,
) -> Result<Vec<ProjectIssueProjection>> {
    issues
        .into_iter()
        .map(|issue| {
            let fingerprint = project_issue_fingerprint(store, project, &issue)?;
            let can_ignore = can_ignore_project_issue(store, project, &issue)?;
            let ignored = can_ignore
                && store.is_issue_ignored("project", Some(&project.id), &issue.id, &fingerprint)?;
            Ok(ProjectIssueProjection {
                issue,
                fingerprint,
                can_ignore,
                ignored,
            })
        })
        .collect()
}

pub fn visible_project_issue_projections(
    store: &SkillStore,
    project: &ProjectRecord,
    issues: Vec<ProjectIssue>,
) -> Result<Vec<ProjectIssueProjection>> {
    Ok(project_issue_projections(store, project, issues)?
        .into_iter()
        .filter(|projection| !projection.ignored)
        .collect())
}

pub fn ignore_project_issue_and_scan(
    store: &SkillStore,
    project_id: &str,
    issue_id: &str,
    fingerprint: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let project = refresh_project(store, project_id, tool_specs)?;
    let projection = project_issue_projections(
        store,
        &project,
        diagnose_project(store, &project, tool_specs)?,
    )?
    .into_iter()
    .find(|projection| projection.issue.id == issue_id)
    .with_context(|| format!("project issue no longer exists: {issue_id}"))?;
    if projection.fingerprint != fingerprint {
        anyhow::bail!("project issue changed; refresh and try again");
    }
    if !projection.can_ignore {
        anyhow::bail!("this project issue cannot be ignored");
    }
    store.upsert_ignored_issue(&IgnoredIssueRecord {
        id: Uuid::new_v4().to_string(),
        scope: "project".to_string(),
        scope_id: Some(project.id.clone()),
        issue_key: projection.issue.id,
        fingerprint: fingerprint.to_string(),
        ignored_at: now_ms(),
    })?;
    Ok(project)
}

pub fn ignore_all_project_issues_and_scan(
    store: &SkillStore,
    project_id: &str,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectRecord> {
    let project = refresh_project(store, project_id, tool_specs)?;
    for projection in project_issue_projections(
        store,
        &project,
        diagnose_project(store, &project, tool_specs)?,
    )? {
        if !projection.can_ignore {
            continue;
        }
        store.upsert_ignored_issue(&IgnoredIssueRecord {
            id: Uuid::new_v4().to_string(),
            scope: "project".to_string(),
            scope_id: Some(project.id.clone()),
            issue_key: projection.issue.id,
            fingerprint: projection.fingerprint,
            ignored_at: now_ms(),
        })?;
    }
    Ok(project)
}

pub fn can_ignore_project_issue(
    store: &SkillStore,
    project: &ProjectRecord,
    issue: &ProjectIssue,
) -> Result<bool> {
    if matches!(
        issue.kind.as_str(),
        "tool_removed_pending_unsync" | "git_exclude_drift" | "git_exclude_unavailable"
    ) {
        return Ok(true);
    }
    if !matches!(
        issue.kind.as_str(),
        "target_missing" | "tool_added_pending_sync" | "injection_failed"
    ) {
        return Ok(false);
    }
    let (Some(skill_id), Some(tool_key)) = (issue.skill_id.as_deref(), issue.tool_key.as_deref())
    else {
        return Ok(false);
    };
    let Some(skill) = store.get_skill_by_id(skill_id)? else {
        return Ok(false);
    };
    let source_hash = if Path::new(&skill.central_path).is_dir() {
        hash_dir(Path::new(&skill.central_path)).ok()
    } else {
        None
    };
    let Some(source_hash) = source_hash else {
        return Ok(false);
    };
    for assignment in store.list_skill_assignments(skill_id)? {
        if assignment.state != "active"
            || (assignment.scope == "project"
                && assignment.project_id.as_deref() == Some(project.id.as_str())
                && assignment.tool == tool_key)
        {
            continue;
        }
        if project_assignment_exists(store, &assignment, &source_hash)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn project_assignment_exists(
    store: &SkillStore,
    assignment: &SkillAssignmentRecord,
    source_hash: &str,
) -> Result<bool> {
    let project = assignment
        .project_id
        .as_deref()
        .and_then(|project_id| store.get_project(project_id).transpose())
        .transpose()?;
    if let Some(project) = project.as_ref() {
        if !Path::new(&project.root_path).is_dir()
            || !project.enabled_tool_keys.contains(&assignment.tool)
        {
            return Ok(false);
        }
    }
    if assignment.ownership == "native" {
        let Some(project) = project else {
            return Ok(false);
        };
        return Ok(store
            .list_project_discovered_skills(&project.id)?
            .into_iter()
            .any(|item| {
                item.matched_skill_id.as_deref() == Some(assignment.skill_id.as_str())
                    && item.tool_keys.contains(&assignment.tool)
                    && item.content_hash.as_deref() == Some(source_hash)
                    && Path::new(&item.absolute_path).is_dir()
            }));
    }
    let project_path = project.as_ref().map(|project| project.root_path.as_str());
    let Some(target) = store.get_skill_target(
        &assignment.skill_id,
        &assignment.tool,
        &assignment.scope,
        project_path,
    )?
    else {
        return Ok(false);
    };
    if target.status != "ok" || !Path::new(&target.target_path).is_dir() {
        return Ok(false);
    }
    let target_path = Path::new(&target.target_path);
    let hash_path =
        std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
    Ok(hash_dir(&hash_path)
        .map(|hash| hash == source_hash)
        .unwrap_or(false))
}

pub fn project_issue_fingerprint(
    store: &SkillStore,
    project: &ProjectRecord,
    issue: &ProjectIssue,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(issue.id.as_bytes());
    hasher.update(issue.kind.as_bytes());
    hasher.update([project.local_git_exclude_enabled as u8]);
    for tool in &project.enabled_tool_keys {
        hasher.update(tool.as_bytes());
        hasher.update([0]);
    }
    if let Some(skill_id) = issue.skill_id.as_deref() {
        hasher.update(skill_id.as_bytes());
        if let Some(skill) = store.get_skill_by_id(skill_id)? {
            hasher.update(skill.content_hash.unwrap_or_default().as_bytes());
            hasher.update(skill.status.as_bytes());
            hasher.update([skill.enabled as u8]);
        }
        for target in store
            .list_project_skill_targets(&project.root_path)?
            .into_iter()
            .filter(|target| {
                target.skill_id == skill_id
                    && issue
                        .tool_key
                        .as_deref()
                        .map_or(true, |tool| target.tool == tool)
            })
        {
            hasher.update(target.tool.as_bytes());
            hasher.update(target.status.as_bytes());
            hasher.update(target.last_error.unwrap_or_default().as_bytes());
            hasher.update(target.target_path.as_bytes());
        }
        for assignment in store.list_skill_assignments(skill_id)? {
            hasher.update(assignment.tool.as_bytes());
            hasher.update(assignment.scope.as_bytes());
            hasher.update(assignment.project_id.unwrap_or_default().as_bytes());
            hasher.update(assignment.ownership.as_bytes());
            hasher.update(assignment.state.as_bytes());
            hasher.update(assignment.last_error.unwrap_or_default().as_bytes());
        }
    }
    if let Some(path) = issue.path.as_deref() {
        let path = Path::new(path);
        if path.is_dir() {
            if let Ok(hash) = hash_dir(path) {
                hasher.update(hash.as_bytes());
            }
        } else if let Ok(bytes) = std::fs::read(path) {
            hasher.update(bytes);
        }
    }
    for action in &issue.actions {
        hasher.update(action.kind.as_bytes());
        hasher.update(action.value.as_deref().unwrap_or_default().as_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn build_project_summary_data(
    store: &SkillStore,
    project: &ProjectRecord,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectSummaryData> {
    let discovered = store.list_project_discovered_skills(&project.id)?;
    let links = store.list_project_skill_links(&project.id)?;
    let skills = store.list_skills()?;
    let skill_names = skills
        .iter()
        .map(|skill| (skill.id.as_str(), skill.name.to_lowercase()))
        .collect::<HashMap<_, _>>();
    let mut names = discovered
        .iter()
        .map(|item| item.name.to_lowercase())
        .collect::<HashSet<_>>();
    for link in links.iter().filter(|link| link.enabled) {
        if let Some(name) = skill_names.get(link.skill_id.as_str()) {
            names.insert(name.clone());
        }
    }
    Ok(ProjectSummaryData {
        skill_count: names.len(),
        issues: diagnose_project(store, project, tool_specs)?,
    })
}

pub fn build_project_detail_data(
    store: &SkillStore,
    project: &ProjectRecord,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectDetailData> {
    let discovered = store.list_project_discovered_skills(&project.id)?;
    let links = store.list_project_skill_links(&project.id)?;
    let library = store.list_skills()?;
    let issues = diagnose_project(store, project, tool_specs)?;
    Ok(ProjectDetailData {
        current_skills: build_current_project_skills(&discovered, &links, &library, &issues),
        library_skills: build_project_library_skills(&discovered, &links, &library, &issues),
        issues,
    })
}

fn build_current_project_skills(
    discovered: &[ProjectDiscoveredSkillRecord],
    links: &[ProjectSkillLinkRecord],
    library: &[SkillRecord],
    issues: &[ProjectIssue],
) -> Vec<ProjectSkillView> {
    #[derive(Default)]
    struct CurrentBuilder {
        name: String,
        discovered: Vec<ProjectDiscoveredSkillRecord>,
        link: Option<ProjectSkillLinkRecord>,
    }

    let library_by_id = library
        .iter()
        .map(|skill| (skill.id.as_str(), skill))
        .collect::<HashMap<_, _>>();
    let mut grouped: BTreeMap<String, CurrentBuilder> = BTreeMap::new();
    for item in discovered {
        let entry = grouped.entry(item.name.to_lowercase()).or_default();
        entry.name = item.name.clone();
        entry.discovered.push(item.clone());
    }
    for link in links.iter().filter(|link| link.enabled) {
        let Some(skill) = library_by_id.get(link.skill_id.as_str()) else {
            continue;
        };
        let entry = grouped.entry(skill.name.to_lowercase()).or_default();
        entry.name = skill.name.clone();
        entry.link = Some(link.clone());
    }

    grouped
        .into_values()
        .map(|builder| {
            let linked_skill = builder
                .link
                .as_ref()
                .and_then(|link| library_by_id.get(link.skill_id.as_str()).copied());
            let matched_skill_id = builder
                .discovered
                .iter()
                .find_map(|item| item.matched_skill_id.clone());
            let matched_skill = matched_skill_id
                .as_deref()
                .and_then(|id| library_by_id.get(id).copied());
            let mut tool_keys = builder
                .discovered
                .iter()
                .flat_map(|item| item.tool_keys.iter().cloned())
                .collect::<BTreeSet<_>>();
            if let Some(link) = &builder.link {
                tool_keys.extend(link.desired_tools.iter().cloned());
            }
            let has_issue = issues.iter().any(|issue| {
                issue
                    .skill_name
                    .as_ref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&builder.name))
                    || builder.link.as_ref().is_some_and(|link| {
                        issue.skill_id.as_deref() == Some(link.skill_id.as_str())
                    })
            });
            let source = if builder.link.is_some() {
                "library"
            } else if matched_skill_id.is_some() {
                "matched_library"
            } else {
                "project"
            };
            ProjectSkillView {
                name: builder.name,
                description: linked_skill
                    .or(matched_skill)
                    .and_then(|skill| skill.description.clone()),
                source: source.to_string(),
                status: if has_issue {
                    "needs_attention".to_string()
                } else {
                    builder
                        .link
                        .as_ref()
                        .map(|link| link.status.clone())
                        .unwrap_or_else(|| "healthy".to_string())
                },
                linked_skill_id: builder.link.map(|link| link.skill_id),
                matched_skill_id,
                tool_keys: tool_keys.into_iter().collect(),
                paths: builder
                    .discovered
                    .into_iter()
                    .map(|item| ProjectSkillPathView {
                        discovered_skill_id: item.id,
                        relative_path: item.relative_path,
                        absolute_path: item.absolute_path,
                        tool_keys: item.tool_keys,
                        content_hash: item.content_hash,
                        status: item.status,
                    })
                    .collect(),
            }
        })
        .collect()
}

fn build_project_library_skills(
    discovered: &[ProjectDiscoveredSkillRecord],
    links: &[ProjectSkillLinkRecord],
    library: &[SkillRecord],
    issues: &[ProjectIssue],
) -> Vec<ProjectLibrarySkillView> {
    let links_by_skill = links
        .iter()
        .filter(|link| link.enabled)
        .map(|link| (link.skill_id.as_str(), link))
        .collect::<HashMap<_, _>>();
    let mut result = library
        .iter()
        .map(|skill| {
            let same_name = discovered
                .iter()
                .filter(|item| item.name.eq_ignore_ascii_case(&skill.name))
                .collect::<Vec<_>>();
            let availability = if !skill.enabled {
                "disabled"
            } else if let Some(link) = links_by_skill.get(skill.id.as_str()) {
                let needs_repair = link.status != "ok"
                    || issues
                        .iter()
                        .any(|issue| issue.skill_id.as_deref() == Some(skill.id.as_str()));
                if needs_repair {
                    "needs_repair"
                } else {
                    "enabled"
                }
            } else if same_name.iter().any(|item| {
                item.content_hash.is_some()
                    && item.content_hash.as_ref() == skill.content_hash.as_ref()
            }) {
                "project_same"
            } else if !same_name.is_empty() {
                "conflict"
            } else {
                "addable"
            };
            ProjectLibrarySkillView {
                skill_id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                availability: availability.to_string(),
            }
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|skill| skill.name.to_lowercase());
    result
}

pub fn inject_project_skill(
    store: &SkillStore,
    project: &ProjectRecord,
    skill: &SkillRecord,
    tool_specs: &[ProjectToolSpec],
) -> Result<ProjectInjectionSummary> {
    inject_project_skill_for_tools(
        store,
        project,
        skill,
        tool_specs,
        None,
        None,
        ProjectConflictPolicy::Preserve,
    )
}

pub fn assign_project_skill_tools_and_scan(
    store: &SkillStore,
    project_id: &str,
    skill_id: &str,
    desired_tools: &[String],
    tool_specs: &[ProjectToolSpec],
    overwrite_conflict_tools: &HashSet<String>,
) -> Result<ProjectInjectionSummary> {
    let project = require_project(store, project_id)?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .with_context(|| format!("skill not found: {skill_id}"))?;
    let desired_tools = normalized_keys(desired_tools);
    let enabled = project
        .enabled_tool_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if let Some(unavailable) = desired_tools
        .iter()
        .find(|tool| !enabled.contains(tool.as_str()))
    {
        anyhow::bail!("project tool is not enabled: {unavailable}");
    }
    validate_enabled_tool_specs(&desired_tools, tool_specs)?;

    let desired = desired_tools
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let targets = store
        .list_project_skill_targets(&project.root_path)?
        .into_iter()
        .filter(|target| target.skill_id == skill_id)
        .collect::<Vec<_>>();
    let mut removed_paths = HashSet::new();
    for target in targets
        .iter()
        .filter(|target| !desired.contains(target.tool.as_str()))
    {
        let path_still_desired = targets.iter().any(|other| {
            other.target_path == target.target_path && desired.contains(other.tool.as_str())
        });
        if !path_still_desired && removed_paths.insert(target.target_path.clone()) {
            remove_target_if_still_owned(store, target)?;
        }
        store.delete_skill_target(skill_id, &target.tool, "project", Some(&project.root_path))?;
    }

    if desired_tools.is_empty() {
        if let Some(mut link) = store.get_project_skill_link(&project.id, skill_id)? {
            link.enabled = false;
            link.desired_tools.clear();
            link.status = "disabled".to_string();
            link.updated_at = now_ms();
            store.upsert_project_skill_link(&link)?;
        }
        scan_project(store, &project, tool_specs)?;
        rebuild_project_git_exclude(store, &project)?;
        return Ok(ProjectInjectionSummary::default());
    }

    let summary = inject_project_skill_for_tools(
        store,
        &project,
        &skill,
        tool_specs,
        Some(&desired_tools),
        None,
        ProjectConflictPolicy::ReplaceTools(overwrite_conflict_tools),
    )?;
    scan_project(store, &project, tool_specs)?;
    rebuild_project_git_exclude(store, &project)?;
    Ok(summary)
}

pub fn add_project_skill_tools_and_scan(
    store: &SkillStore,
    project_id: &str,
    skill_id: &str,
    desired_tools: &[String],
    added_tools: &[String],
    tool_specs: &[ProjectToolSpec],
    overwrite_conflict_tools: &HashSet<String>,
) -> Result<ProjectInjectionSummary> {
    let project = require_project(store, project_id)?;
    let skill = store
        .get_skill_by_id(skill_id)?
        .with_context(|| format!("skill not found: {skill_id}"))?;
    let desired_tools = normalized_keys(desired_tools);
    let added_tools = normalized_keys(added_tools);
    let enabled = project
        .enabled_tool_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if let Some(unavailable) = added_tools
        .iter()
        .find(|tool| !enabled.contains(tool.as_str()))
    {
        anyhow::bail!("project tool is not enabled: {unavailable}");
    }
    validate_enabled_tool_specs(&added_tools, tool_specs)?;
    let summary = inject_project_skill_for_tools(
        store,
        &project,
        &skill,
        tool_specs,
        Some(&desired_tools),
        Some(&added_tools),
        ProjectConflictPolicy::ReplaceTools(overwrite_conflict_tools),
    )?;
    scan_project(store, &project, tool_specs)?;
    rebuild_project_git_exclude(store, &project)?;
    Ok(summary)
}

pub fn remove_project_skill(
    store: &SkillStore,
    project: &ProjectRecord,
    skill_id: &str,
) -> Result<()> {
    let targets = store
        .list_project_skill_targets(&project.root_path)?
        .into_iter()
        .filter(|target| target.skill_id == skill_id)
        .collect::<Vec<_>>();
    let mut removed_paths = HashSet::new();
    for target in &targets {
        if removed_paths.insert(target.target_path.clone()) {
            remove_target_if_still_owned(store, target)?;
        }
        store.delete_skill_target(skill_id, &target.tool, "project", Some(&project.root_path))?;
    }
    if let Some(mut link) = store.get_project_skill_link(&project.id, skill_id)? {
        link.enabled = false;
        link.desired_tools.clear();
        link.status = "disabled".to_string();
        link.updated_at = now_ms();
        store.upsert_project_skill_link(&link)?;
    }
    for assignment in store
        .list_skill_assignments(skill_id)?
        .into_iter()
        .filter(|assignment| assignment.project_id.as_deref() == Some(project.id.as_str()))
    {
        store.delete_skill_assignment(skill_id, &assignment.tool, "project", Some(&project.id))?;
    }
    Ok(())
}

pub fn repair_project_issue(
    store: &SkillStore,
    project: &ProjectRecord,
    tool_specs: &[ProjectToolSpec],
    issue_id: &str,
    action_kind: &str,
    action_value: Option<&str>,
) -> Result<()> {
    let issue = diagnose_project(store, project, tool_specs)?
        .into_iter()
        .find(|item| item.id == issue_id)
        .with_context(|| format!("project issue no longer exists: {issue_id}"))?;
    issue
        .actions
        .iter()
        .find(|action| action.kind == action_kind && action.value.as_deref() == action_value)
        .with_context(|| format!("project issue action is no longer available: {action_kind}"))?;
    match action_kind {
        "delete_invalid_skill" => {
            let path = PathBuf::from(issue.path.context("issue is missing path")?);
            ensure_project_child_path(project, &path)?;
            if path.join("SKILL.md").is_file() {
                anyhow::bail!("project skill was repaired after the last scan: {:?}", path);
            }
            remove_path_any(&path)?;
        }
        "keep_project_variant" => {
            let discovered_skill_id = action_value.context("action is missing variant id")?;
            replace_project_variants(store, project, &issue, discovered_skill_id)?;
        }
        "replace_with_library" => {
            let skill_id = issue.skill_id.context("issue is missing skill id")?;
            let skill = store
                .get_skill_by_id(&skill_id)?
                .with_context(|| format!("skill not found: {skill_id}"))?;
            inject_project_skill_for_tools(
                store,
                project,
                &skill,
                tool_specs,
                None,
                None,
                ProjectConflictPolicy::ReplaceAll,
            )?;
        }
        "keep_project_version" => {
            let skill_id = issue.skill_id.context("issue is missing skill id")?;
            remove_project_skill(store, project, &skill_id)?;
        }
        "retry_injection" => {
            let skill_id = issue.skill_id.context("issue is missing skill id")?;
            let tool_key = issue.tool_key.context("issue is missing tool key")?;
            let skill = store
                .get_skill_by_id(&skill_id)?
                .with_context(|| format!("skill not found: {skill_id}"))?;
            inject_project_skill_for_tools(
                store,
                project,
                &skill,
                tool_specs,
                None,
                Some(&[tool_key]),
                ProjectConflictPolicy::Preserve,
            )?;
        }
        "remove_stale_target" => {
            let skill_id = issue.skill_id.context("issue is missing skill id")?;
            let tool_key = issue.tool_key.context("issue is missing tool key")?;
            let targets = store.list_project_skill_targets(&project.root_path)?;
            if let Some(target) = targets
                .iter()
                .find(|target| target.skill_id == skill_id && target.tool == tool_key)
            {
                let path_still_desired = targets.iter().any(|other| {
                    other.id != target.id
                        && other.target_path == target.target_path
                        && project.enabled_tool_keys.contains(&other.tool)
                });
                if !path_still_desired {
                    remove_target_if_still_owned(store, target)?;
                }
                store.delete_skill_target(
                    &skill_id,
                    &tool_key,
                    "project",
                    Some(&project.root_path),
                )?;
            }
        }
        "repair_git_exclude" => rebuild_project_git_exclude(store, project)?,
        "disable_git_exclude" => {
            let mut updated = project.clone();
            updated.local_git_exclude_enabled = false;
            updated.updated_at = now_ms();
            store.update_project(&updated)?;
        }
        "initialize_git_repository" => {
            git2::Repository::init(&project.root_path)
                .with_context(|| format!("initialize Git repository at {}", project.root_path))?;
            rebuild_project_git_exclude(store, project)?;
        }
        "enable_library_skill" => {
            let skill_id = issue.skill_id.context("issue is missing skill id")?;
            store.set_skill_enabled(&skill_id, true)?;
            let skill = store
                .get_skill_by_id(&skill_id)?
                .with_context(|| format!("skill not found: {skill_id}"))?;
            inject_project_skill_for_tools(
                store,
                project,
                &skill,
                tool_specs,
                None,
                None,
                ProjectConflictPolicy::Preserve,
            )?;
        }
        "remove_unavailable_library_skill" => {
            let skill_id = issue.skill_id.context("issue is missing skill id")?;
            remove_project_skill(store, project, &skill_id)?;
        }
        _ => anyhow::bail!("unsupported project issue action: {action_kind}"),
    }
    scan_project(store, project, tool_specs)?;
    let active_link_ids = store
        .list_project_skill_links(&project.id)?
        .into_iter()
        .filter(|link| link.enabled)
        .map(|link| link.skill_id)
        .collect::<Vec<_>>();
    for skill_id in active_link_ids {
        reconcile_project_link_status(store, project, tool_specs, &skill_id)?;
    }
    Ok(())
}

fn replace_project_variants(
    store: &SkillStore,
    project: &ProjectRecord,
    issue: &ProjectIssue,
    chosen_id: &str,
) -> Result<()> {
    let skill_name = issue
        .skill_name
        .as_deref()
        .context("issue is missing skill name")?;
    let variants = store
        .list_project_discovered_skills(&project.id)?
        .into_iter()
        .filter(|item| {
            item.name.eq_ignore_ascii_case(skill_name) && item.status != "invalid_missing_skill_md"
        })
        .collect::<Vec<_>>();
    if variants
        .iter()
        .any(|item| current_project_skill_hash(item) != item.content_hash)
    {
        anyhow::bail!("project skill variants changed after the last scan");
    }
    let chosen = variants
        .iter()
        .find(|item| item.id == chosen_id)
        .with_context(|| format!("project skill variant no longer exists: {chosen_id}"))?;
    let chosen_path = PathBuf::from(&chosen.absolute_path);
    ensure_project_child_path(project, &chosen_path)?;
    let source = std::fs::canonicalize(&chosen_path)
        .with_context(|| format!("resolve chosen project skill {:?}", chosen_path))?;

    for variant in variants.iter().filter(|item| item.id != chosen_id) {
        let target = PathBuf::from(&variant.absolute_path);
        ensure_project_child_path(project, &target)?;
        sync_dir_copy_with_overwrite(&source, &target, true)
            .with_context(|| format!("replace project skill variant {:?}", target))?;
        forget_project_targets_at_path(store, project, &target)?;
    }
    Ok(())
}

fn forget_project_targets_at_path(
    store: &SkillStore,
    project: &ProjectRecord,
    path: &Path,
) -> Result<()> {
    let target_path = path.to_string_lossy();
    for target in store
        .list_project_skill_targets(&project.root_path)?
        .into_iter()
        .filter(|target| target.target_path == target_path)
    {
        store.delete_skill_target(
            &target.skill_id,
            &target.tool,
            "project",
            Some(&project.root_path),
        )?;
    }
    Ok(())
}

fn ensure_project_child_path(project: &ProjectRecord, path: &Path) -> Result<()> {
    if !path.is_absolute() {
        anyhow::bail!("project issue path must be absolute: {:?}", path);
    }
    let project_root = std::fs::canonicalize(&project.root_path)
        .with_context(|| format!("resolve project root {}", project.root_path))?;
    let parent = path
        .parent()
        .with_context(|| format!("project issue path has no parent: {:?}", path))?;
    let normalized_parent = std::fs::canonicalize(parent)
        .with_context(|| format!("resolve project issue parent {:?}", parent))?;
    if !normalized_parent.starts_with(&project_root) || path == project_root {
        anyhow::bail!("project issue path escapes project root: {:?}", path);
    }
    Ok(())
}

pub fn rebuild_project_git_exclude(store: &SkillStore, project: &ProjectRecord) -> Result<()> {
    let targets = store.list_project_skill_targets(&project.root_path)?;
    let repository = match git2::Repository::discover(&project.root_path) {
        Ok(repository) => repository,
        Err(_) if !project.local_git_exclude_enabled => return Ok(()),
        Err(err) => return Err(err).context("project is not inside a Git repository"),
    };
    let exclude_path = repository.path().join("info/exclude");
    let patterns = expected_git_exclude_patterns(project, &repository, &targets);
    let current = read_optional_string(&exclude_path)?;
    let rendered = render_git_exclude(&current, &patterns);
    if rendered == normalize_newlines(&current) {
        return Ok(());
    }
    if let Some(parent) = exclude_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create Git exclude directory {:?}", parent))?;
    }
    std::fs::write(&exclude_path, rendered)
        .with_context(|| format!("write Git exclude file {:?}", exclude_path))?;
    Ok(())
}

fn inject_project_skill_for_tools(
    store: &SkillStore,
    project: &ProjectRecord,
    skill: &SkillRecord,
    tool_specs: &[ProjectToolSpec],
    desired_tools: Option<&[String]>,
    only_tools: Option<&[String]>,
    conflict_policy: ProjectConflictPolicy<'_>,
) -> Result<ProjectInjectionSummary> {
    let now = now_ms();
    let existing_link = store.get_project_skill_link(&project.id, &skill.id)?;
    let next_desired_tools = desired_tools
        .map(ToOwned::to_owned)
        .or_else(|| {
            existing_link
                .as_ref()
                .map(|link| link.desired_tools.clone())
        })
        .unwrap_or_else(|| project.enabled_tool_keys.clone());
    let mut link = existing_link.unwrap_or(ProjectSkillLinkRecord {
        project_id: project.id.clone(),
        skill_id: skill.id.clone(),
        enabled: true,
        desired_tools: next_desired_tools.clone(),
        status: "pending".to_string(),
        created_at: now,
        updated_at: now,
    });
    link.enabled = true;
    link.desired_tools = next_desired_tools;
    link.status = "pending".to_string();
    link.updated_at = now;
    store.upsert_project_skill_link(&link)?;

    if !skill.enabled {
        link.status = "error".to_string();
        store.upsert_project_skill_link(&link)?;
        for tool_key in &link.desired_tools {
            upsert_project_assignment(
                store,
                &skill.id,
                &project.id,
                tool_key,
                "hub",
                "suspended",
                None,
            )?;
        }
        anyhow::bail!("skill is disabled: {}", skill.name);
    }
    let source = PathBuf::from(&skill.central_path);
    if !source.is_dir() {
        link.status = "error".to_string();
        store.upsert_project_skill_link(&link)?;
        for tool_key in &link.desired_tools {
            upsert_project_assignment(
                store,
                &skill.id,
                &project.id,
                tool_key,
                "hub",
                "error",
                Some("central skill path not found"),
            )?;
        }
        anyhow::bail!("central skill path not found: {}", skill.central_path);
    }
    let target_name = match validate_project_library_name(&skill.name) {
        Ok(name) => name,
        Err(err) => {
            link.status = "error".to_string();
            store.upsert_project_skill_link(&link)?;
            return Err(err);
        }
    };

    let desired = link.desired_tools.iter().collect::<HashSet<_>>();
    let selected = only_tools.map(|items| items.iter().collect::<HashSet<_>>());
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for spec in tool_specs {
        if !desired.contains(&spec.key) {
            continue;
        }
        if selected
            .as_ref()
            .is_some_and(|selected| !selected.contains(&spec.key))
        {
            continue;
        }
        validate_project_skills_dir(&spec.relative_skills_dir)?;
        groups
            .entry(spec.relative_skills_dir.clone())
            .or_default()
            .push(spec.key.clone());
    }

    let expected_hash = match &skill.content_hash {
        Some(hash) => hash.clone(),
        None => hash_dir(&source)?,
    };
    let existing_targets = store.list_project_skill_targets(&project.root_path)?;
    let mut summary = ProjectInjectionSummary::default();
    let mut had_conflict = false;
    for (relative_dir, mut tool_keys) in groups {
        tool_keys.sort();
        tool_keys.dedup();
        let target = resolve_project_skills_dir(project, &relative_dir)?.join(&target_name);
        let overwrite_conflicts = match &conflict_policy {
            ProjectConflictPolicy::Preserve => false,
            ProjectConflictPolicy::ReplaceAll => true,
            ProjectConflictPolicy::ReplaceTools(tools) => {
                tool_keys.iter().all(|tool| tools.contains(tool))
            }
        };
        let target_exists = std::fs::symlink_metadata(&target).is_ok();
        if target_exists {
            let hash_path = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
            let same_content = hash_dir(&hash_path)
                .map(|hash| hash == expected_hash)
                .unwrap_or(false);
            if !same_content {
                if !overwrite_conflicts {
                    for tool_key in &tool_keys {
                        upsert_project_assignment(
                            store,
                            &skill.id,
                            &project.id,
                            tool_key,
                            "native",
                            "error",
                            Some("same name has different content"),
                        )?;
                    }
                    summary.failed += tool_keys.len();
                    had_conflict = true;
                    continue;
                }
            } else {
                let managed = existing_targets.iter().any(|existing| {
                    existing.skill_id == skill.id
                        && existing.target_path == target.to_string_lossy()
                });
                if managed {
                    for tool_key in &tool_keys {
                        store.upsert_skill_target(&SkillTargetRecord {
                            id: Uuid::new_v4().to_string(),
                            skill_id: skill.id.clone(),
                            tool: tool_key.clone(),
                            scope: "project".to_string(),
                            project_path: Some(project.root_path.clone()),
                            target_path: target.to_string_lossy().to_string(),
                            mode: "existing".to_string(),
                            status: "ok".to_string(),
                            last_error: None,
                            synced_at: Some(now_ms()),
                        })?;
                        upsert_project_assignment(
                            store,
                            &skill.id,
                            &project.id,
                            tool_key,
                            "hub",
                            "active",
                            None,
                        )?;
                    }
                    summary.injected += tool_keys.len();
                } else {
                    for tool_key in &tool_keys {
                        upsert_project_assignment(
                            store,
                            &skill.id,
                            &project.id,
                            tool_key,
                            "native",
                            "active",
                            None,
                        )?;
                    }
                    summary.matched += tool_keys.len();
                }
                continue;
            }
        }

        match sync_dir_for_tool_with_overwrite(&tool_keys[0], &source, &target, overwrite_conflicts)
        {
            Ok(outcome) => {
                let mode = sync_mode_name(&outcome.mode_used);
                for tool_key in &tool_keys {
                    store.upsert_skill_target(&SkillTargetRecord {
                        id: Uuid::new_v4().to_string(),
                        skill_id: skill.id.clone(),
                        tool: tool_key.clone(),
                        scope: "project".to_string(),
                        project_path: Some(project.root_path.clone()),
                        target_path: target.to_string_lossy().to_string(),
                        mode: mode.to_string(),
                        status: "ok".to_string(),
                        last_error: None,
                        synced_at: Some(now_ms()),
                    })?;
                    upsert_project_assignment(
                        store,
                        &skill.id,
                        &project.id,
                        tool_key,
                        "hub",
                        "active",
                        None,
                    )?;
                }
                summary.injected += tool_keys.len();
            }
            Err(err) => {
                let message = format!("{err:#}");
                for tool_key in &tool_keys {
                    store.upsert_skill_target(&SkillTargetRecord {
                        id: Uuid::new_v4().to_string(),
                        skill_id: skill.id.clone(),
                        tool: tool_key.clone(),
                        scope: "project".to_string(),
                        project_path: Some(project.root_path.clone()),
                        target_path: target.to_string_lossy().to_string(),
                        mode: "pending".to_string(),
                        status: "error".to_string(),
                        last_error: Some(message.clone()),
                        synced_at: None,
                    })?;
                    upsert_project_assignment(
                        store,
                        &skill.id,
                        &project.id,
                        tool_key,
                        "hub",
                        "error",
                        Some(&message),
                    )?;
                }
                summary.failed += tool_keys.len();
            }
        }
    }

    link.status = if summary.failed == 0 {
        "ok"
    } else if had_conflict {
        "conflict"
    } else if summary.injected + summary.matched > 0 {
        "partial"
    } else {
        "error"
    }
    .to_string();
    link.updated_at = now_ms();
    store.upsert_project_skill_link(&link)?;
    Ok(summary)
}

fn git_exclude_state(
    project: &ProjectRecord,
    targets: &[SkillTargetRecord],
) -> Result<GitExcludeState> {
    let active_targets = targets
        .iter()
        .filter(|target| target.status == "ok")
        .collect::<Vec<_>>();
    let repository = match git2::Repository::discover(&project.root_path) {
        Ok(repository) => repository,
        Err(_) if !project.local_git_exclude_enabled || active_targets.is_empty() => {
            return Ok(GitExcludeState::Clean)
        }
        Err(err) => {
            let can_initialize = !has_git_marker(Path::new(&project.root_path));
            if !can_initialize {
                log::warn!(
                    "failed to discover Git repository for {}: {err:#}",
                    project.root_path
                );
            }
            return Ok(GitExcludeState::Unavailable { can_initialize });
        }
    };
    let exclude_path = repository.path().join("info/exclude");
    let current = read_optional_string(&exclude_path)?;
    let patterns = expected_git_exclude_patterns(project, &repository, targets);
    let rendered = render_git_exclude(&current, &patterns);
    if rendered == normalize_newlines(&current) {
        Ok(GitExcludeState::Clean)
    } else {
        Ok(GitExcludeState::Drift(exclude_path))
    }
}

fn has_git_marker(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        let marker = ancestor.join(".git");
        match std::fs::symlink_metadata(&marker) {
            Ok(_) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                ancestor.join("HEAD").is_file() && ancestor.join("objects").is_dir()
            }
            Err(_) => true,
        }
    })
}

fn expected_git_exclude_patterns(
    project: &ProjectRecord,
    repository: &git2::Repository,
    targets: &[SkillTargetRecord],
) -> Vec<String> {
    if !project.local_git_exclude_enabled {
        return Vec::new();
    }
    let Some(workdir) = repository.workdir() else {
        return Vec::new();
    };
    let project_root = Path::new(&project.root_path);
    let normalized_project_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let normalized_workdir =
        std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let mut patterns = targets
        .iter()
        .filter(|target| target.status == "ok")
        .filter_map(|target| {
            let target_path = Path::new(&target.target_path);
            let normalized_target = target_path
                .parent()
                .and_then(|parent| std::fs::canonicalize(parent).ok())
                .and_then(|parent| target_path.file_name().map(|name| parent.join(name)))
                .unwrap_or_else(|| target_path.to_path_buf());
            if !normalized_target.starts_with(&normalized_project_root) {
                return None;
            }
            let relative = normalized_target.strip_prefix(&normalized_workdir).ok()?;
            Some(format!("/{}/", slash_path(relative).trim_matches('/')))
        })
        .collect::<Vec<_>>();
    patterns.sort();
    patterns.dedup();
    patterns
}

fn render_git_exclude(current: &str, patterns: &[String]) -> String {
    let normalized = normalize_newlines(current);
    let mut outside = Vec::new();
    let lines = normalized.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        if lines[index] == GIT_EXCLUDE_BEGIN {
            let next_marker = lines[index + 1..]
                .iter()
                .position(|line| *line == GIT_EXCLUDE_BEGIN || *line == GIT_EXCLUDE_END);
            if let Some(offset) = next_marker {
                let marker_index = index + 1 + offset;
                if lines[marker_index] == GIT_EXCLUDE_END {
                    index = marker_index + 1;
                    continue;
                }
            }
            index += 1;
            continue;
        }
        if lines[index] == GIT_EXCLUDE_END {
            index += 1;
            continue;
        }
        outside.push(lines[index].to_string());
        index += 1;
    }
    while outside.last().is_some_and(|line| line.is_empty()) {
        outside.pop();
    }
    if !patterns.is_empty() {
        if !outside.is_empty() {
            outside.push(String::new());
        }
        outside.push(GIT_EXCLUDE_BEGIN.to_string());
        outside.extend(patterns.iter().cloned());
        outside.push(GIT_EXCLUDE_END.to_string());
    }
    if outside.is_empty() {
        String::new()
    } else {
        format!("{}\n", outside.join("\n"))
    }
}

fn require_project(store: &SkillStore, project_id: &str) -> Result<ProjectRecord> {
    store
        .get_project(project_id)?
        .with_context(|| format!("project not found: {project_id}"))
}

#[allow(clippy::too_many_arguments)]
fn upsert_project_assignment(
    store: &SkillStore,
    skill_id: &str,
    project_id: &str,
    tool: &str,
    ownership: &str,
    state: &str,
    last_error: Option<&str>,
) -> Result<()> {
    let now = now_ms();
    let existing = store.get_skill_assignment(skill_id, tool, "project", Some(project_id))?;
    store.upsert_skill_assignment(&SkillAssignmentRecord {
        id: existing
            .as_ref()
            .map(|assignment| assignment.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        scope: "project".to_string(),
        project_id: Some(project_id.to_string()),
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

fn group_library_by_name(skills: &[SkillRecord]) -> HashMap<String, Vec<&SkillRecord>> {
    let mut grouped: HashMap<String, Vec<&SkillRecord>> = HashMap::new();
    for skill in skills {
        grouped
            .entry(skill.name.to_lowercase())
            .or_default()
            .push(skill);
    }
    grouped
}

fn current_project_skill_hash(item: &ProjectDiscoveredSkillRecord) -> Option<String> {
    if item.status == "invalid_missing_skill_md" {
        return None;
    }
    let path = PathBuf::from(&item.absolute_path);
    let hash_path = std::fs::canonicalize(&path).unwrap_or(path);
    hash_dir(&hash_path).ok()
}

fn project_issue_action(
    kind: &str,
    value: Option<String>,
    label: Option<String>,
    confirmation_required: bool,
    destructive: bool,
) -> ProjectIssueAction {
    ProjectIssueAction {
        kind: kind.to_string(),
        value,
        label,
        confirmation_required,
        destructive,
    }
}

fn validate_project_skills_dir(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.trim().is_empty() || path.is_absolute() {
        anyhow::bail!("project skills directory must be relative: {value}");
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        anyhow::bail!("project skills directory escapes project root: {value}");
    }
    Ok(())
}

fn validate_enabled_tool_specs(
    enabled_tool_keys: &[String],
    tool_specs: &[ProjectToolSpec],
) -> Result<()> {
    let enabled = enabled_tool_keys.iter().collect::<HashSet<_>>();
    for spec in tool_specs {
        if enabled.contains(&spec.key) {
            validate_project_skills_dir(&spec.relative_skills_dir)?;
        }
    }
    Ok(())
}

fn resolve_project_skills_dir(project: &ProjectRecord, relative_dir: &str) -> Result<PathBuf> {
    validate_project_skills_dir(relative_dir)?;
    let project_root = std::fs::canonicalize(&project.root_path)
        .with_context(|| format!("resolve project root {}", project.root_path))?;
    let candidate = project_root.join(relative_dir);
    let mut ancestor = candidate.as_path();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let resolved = std::fs::canonicalize(ancestor)
                    .with_context(|| format!("resolve project path {:?}", ancestor))?;
                if !resolved.starts_with(&project_root) {
                    anyhow::bail!("project skills directory escapes project root: {relative_dir}");
                }
                break;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .context("project skills directory has no parent")?;
            }
            Err(err) => {
                return Err(err).with_context(|| format!("stat project path {:?}", ancestor));
            }
        }
    }
    Ok(candidate)
}

fn validate_project_library_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
    {
        anyhow::bail!("library skill name must be a single directory name");
    }
    Ok(value.to_string())
}

fn reconcile_project_link_status(
    store: &SkillStore,
    project: &ProjectRecord,
    tool_specs: &[ProjectToolSpec],
    skill_id: &str,
) -> Result<()> {
    let Some(mut link) = store.get_project_skill_link(&project.id, skill_id)? else {
        return Ok(());
    };
    if !link.enabled {
        return Ok(());
    }
    let related = diagnose_project(store, project, tool_specs)?
        .into_iter()
        .filter(|issue| issue.skill_id.as_deref() == Some(skill_id))
        .collect::<Vec<_>>();
    link.status = if related
        .iter()
        .any(|issue| issue.kind == "same_name_content_conflict")
    {
        "conflict"
    } else if related.is_empty() {
        "ok"
    } else {
        "pending"
    }
    .to_string();
    link.updated_at = now_ms();
    store.upsert_project_skill_link(&link)
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn sync_mode_name(mode: &SyncMode) -> &'static str {
    match mode {
        SyncMode::Auto => "auto",
        SyncMode::Symlink => "symlink",
        SyncMode::Junction => "junction",
        SyncMode::Copy => "copy",
    }
}

fn remove_target_if_still_owned(store: &SkillStore, target: &SkillTargetRecord) -> Result<bool> {
    let path = Path::new(&target.target_path);
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(err).with_context(|| format!("stat target {:?}", path)),
    };
    let Some(skill) = store.get_skill_by_id(&target.skill_id)? else {
        return Ok(false);
    };
    let central_path = PathBuf::from(&skill.central_path);
    let still_owned = if metadata.file_type().is_symlink() {
        std::fs::read_link(path)
            .ok()
            .map(|link| {
                let resolved = if link.is_absolute() {
                    link
                } else {
                    path.parent().unwrap_or_else(|| Path::new(".")).join(link)
                };
                resolved == central_path
                    || std::fs::canonicalize(resolved).ok()
                        == std::fs::canonicalize(&central_path).ok()
            })
            .unwrap_or(false)
    } else {
        let expected_hash = skill.content_hash.or_else(|| hash_dir(&central_path).ok());
        expected_hash.is_some_and(|expected| {
            hash_dir(path)
                .map(|actual| actual == expected)
                .unwrap_or(false)
        })
    };
    if still_owned {
        remove_path_any(path)?;
    }
    Ok(still_owned)
}

fn remove_path_any(path: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("stat target {:?}", path)),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(path).with_context(|| format!("remove target {:?}", path))?;
    } else {
        std::fs::remove_dir_all(path).with_context(|| format!("remove target {:?}", path))?;
    }
    Ok(())
}

fn read_optional_string(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err).with_context(|| format!("read {:?}", path)),
    }
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

enum GitExcludeState {
    Clean,
    Drift(PathBuf),
    Unavailable { can_initialize: bool },
}

#[cfg(test)]
#[path = "tests/projects.rs"]
mod tests;
