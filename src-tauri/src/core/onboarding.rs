use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::central_repo::resolve_central_repo_path;
use super::content_hash::hash_dir;
use super::skill_store::{IgnoredIssueRecord, SkillRecord, SkillStore};
use super::tool_adapters::{default_tool_adapters, scan_tool_dir, DetectedSkill};

const ONBOARDING_IGNORE_SCOPE: &str = "onboarding";
const ONBOARDING_IGNORE_ISSUE_KEY: &str = "discovered_skill";

#[derive(Clone, Debug, Serialize)]
pub struct OnboardingVariant {
    pub tool: String,
    pub name: String,
    pub path: PathBuf,
    pub fingerprint: Option<String>,
    pub is_link: bool,
    pub link_target: Option<PathBuf>,
    pub plugin_name: Option<String>,
    pub plugin_version: Option<String>,
    pub plugin_scope: Option<String>,
    pub hub_relation: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OnboardingHubSkill {
    pub id: String,
    pub name: String,
    pub central_path: PathBuf,
    pub fingerprint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OnboardingGroup {
    pub name: String,
    pub variants: Vec<OnboardingVariant>,
    pub has_conflict: bool,
    pub hub_skill: Option<OnboardingHubSkill>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OnboardingPlan {
    pub total_tools_scanned: usize,
    pub total_skills_found: usize,
    pub groups: Vec<OnboardingGroup>,
}

pub fn build_onboarding_plan<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &SkillStore,
) -> Result<OnboardingPlan> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("failed to resolve home directory"))?;
    let central = resolve_central_repo_path(app, store)?;
    let managed_skills = store.list_skills()?;
    let mut managed_targets = store
        .list_all_skill_target_paths()?
        .into_iter()
        .map(|(tool, path)| managed_target_key(&tool, Path::new(&path)))
        .collect::<std::collections::HashSet<_>>();
    for skill in &managed_skills {
        if let Some(source_ref) = skill.source_ref.as_ref() {
            managed_targets.insert(managed_target_key(
                CLAUDE_PLUGIN_TOOL_KEY,
                Path::new(source_ref),
            ));
        }
    }
    let claude_config_dir = resolve_claude_config_dir(&home);
    let plan = build_onboarding_plan_with_claude_dir(
        &home,
        &claude_config_dir,
        Some(&central),
        Some(&managed_targets),
        &managed_skills,
    )?;
    filter_ignored_discovered_skills(store, plan)
}

pub fn ignore_discovered_skill(store: &SkillStore, paths: &[String]) -> Result<usize> {
    let mut ignored = 0;
    let mut seen = HashSet::new();
    for raw_path in paths {
        let path = Path::new(raw_path);
        let issue_key = discovered_issue_key(path);
        if !seen.insert(issue_key.clone()) {
            continue;
        }
        store.upsert_ignored_issue(&IgnoredIssueRecord {
            id: Uuid::new_v4().to_string(),
            scope: ONBOARDING_IGNORE_SCOPE.to_string(),
            scope_id: None,
            issue_key,
            fingerprint: discovered_issue_fingerprint(path),
            ignored_at: now_ms(),
        })?;
        ignored += 1;
    }
    Ok(ignored)
}

fn filter_ignored_discovered_skills(
    store: &SkillStore,
    plan: OnboardingPlan,
) -> Result<OnboardingPlan> {
    let mut groups = Vec::new();
    for mut group in plan.groups {
        let mut variants = Vec::with_capacity(group.variants.len());
        for variant in group.variants {
            if !store.is_issue_ignored(
                ONBOARDING_IGNORE_SCOPE,
                None,
                &discovered_issue_key(&variant.path),
                &discovered_issue_fingerprint(&variant.path),
            )? {
                variants.push(variant);
            }
        }
        if variants.is_empty() {
            continue;
        }
        group.has_conflict = variants
            .iter()
            .filter_map(|variant| variant.fingerprint.as_ref())
            .collect::<HashSet<_>>()
            .len()
            > 1;
        group.variants = variants;
        groups.push(group);
    }

    let total_skills_found = groups.iter().map(|group| group.variants.len()).sum();
    Ok(OnboardingPlan {
        total_tools_scanned: plan.total_tools_scanned,
        total_skills_found,
        groups,
    })
}

fn discovered_issue_key(path: &Path) -> String {
    format!(
        "{}\n{}",
        ONBOARDING_IGNORE_ISSUE_KEY,
        normalize_path_for_key(path)
    )
}

fn discovered_issue_fingerprint(path: &Path) -> String {
    hash_dir(path).unwrap_or_else(|_| "missing".to_string())
}

#[cfg(test)]
fn build_onboarding_plan_in_home(
    home: &Path,
    exclude_root: Option<&Path>,
    exclude_managed_targets: Option<&std::collections::HashSet<String>>,
) -> Result<OnboardingPlan> {
    build_onboarding_plan_with_managed_skills_in_home(
        home,
        exclude_root,
        exclude_managed_targets,
        &[],
    )
}

#[cfg(test)]
fn build_onboarding_plan_with_managed_skills_in_home(
    home: &Path,
    exclude_root: Option<&Path>,
    exclude_managed_targets: Option<&std::collections::HashSet<String>>,
    managed_skills: &[SkillRecord],
) -> Result<OnboardingPlan> {
    build_onboarding_plan_with_claude_dir(
        home,
        &home.join(".claude"),
        exclude_root,
        exclude_managed_targets,
        managed_skills,
    )
}

fn build_onboarding_plan_with_claude_dir(
    home: &Path,
    claude_config_dir: &Path,
    exclude_root: Option<&Path>,
    exclude_managed_targets: Option<&std::collections::HashSet<String>>,
    managed_skills: &[SkillRecord],
) -> Result<OnboardingPlan> {
    let adapters = default_tool_adapters();
    let mut all_detected: Vec<DetectedSkill> = Vec::new();
    let mut scanned = 0usize;
    let mut scanned_claude = false;

    for adapter in &adapters {
        if !home.join(adapter.relative_detect_dir).exists() {
            continue;
        }
        scanned += 1;
        if adapter.id.as_key() == "claude_code" {
            scanned_claude = true;
        }
        let dir = home.join(adapter.relative_skills_dir);
        let detected = scan_tool_dir(adapter, &dir)?;
        all_detected.extend(filter_detected(
            detected,
            exclude_root,
            exclude_managed_targets,
        ));
    }

    let mut grouped: HashMap<String, Vec<OnboardingVariant>> = HashMap::new();
    for skill in all_detected.iter() {
        let fingerprint = hash_dir(&skill.path).ok();
        let entry = grouped.entry(skill.name.clone()).or_default();
        entry.push(OnboardingVariant {
            tool: skill.tool.as_key().to_string(),
            name: skill.name.clone(),
            path: skill.path.clone(),
            fingerprint,
            is_link: skill.is_link,
            link_target: skill.link_target.clone(),
            plugin_name: None,
            plugin_version: None,
            plugin_scope: None,
            hub_relation: None,
        });
    }

    let mut seen_source_paths = all_detected
        .iter()
        .filter_map(|skill| fs::canonicalize(&skill.path).ok())
        .collect::<HashSet<_>>();
    let plugin_variants = discover_claude_plugin_skills(claude_config_dir)
        .into_iter()
        .filter(|variant| {
            let canonical_path =
                fs::canonicalize(&variant.path).unwrap_or_else(|_| variant.path.clone());
            if !seen_source_paths.insert(canonical_path) {
                return false;
            }
            if let Some(exclude_root) = exclude_root {
                if is_under(&variant.path, exclude_root) {
                    return false;
                }
            }
            if let Some(exclude) = exclude_managed_targets {
                if exclude.contains(&managed_target_key(&variant.tool, &variant.path)) {
                    return false;
                }
            }
            true
        })
        .collect::<Vec<_>>();
    if !plugin_variants.is_empty() && !scanned_claude {
        scanned += 1;
    }
    let plugin_skill_count = plugin_variants.len();
    for variant in plugin_variants {
        grouped
            .entry(variant.name.clone())
            .or_default()
            .push(variant);
    }

    let groups: Vec<OnboardingGroup> = grouped
        .into_iter()
        .map(|(name, mut variants)| {
            let mut uniq = variants
                .iter()
                .filter_map(|v| v.fingerprint.as_ref())
                .collect::<std::collections::HashSet<_>>()
                .len();
            if uniq == 0 {
                uniq = 1;
            }
            let hub_skill = managed_skills
                .iter()
                .find(|skill| skill.name.eq_ignore_ascii_case(&name))
                .map(|skill| {
                    let fingerprint = hash_dir(Path::new(&skill.central_path)).ok();
                    for variant in &mut variants {
                        variant.hub_relation = Some(
                            match (variant.fingerprint.as_ref(), fingerprint.as_ref()) {
                                (Some(discovered), Some(hub)) if discovered == hub => "same",
                                (Some(_), Some(_)) => "different",
                                _ => "unknown",
                            }
                            .to_string(),
                        );
                    }
                    OnboardingHubSkill {
                        id: skill.id.clone(),
                        name: skill.name.clone(),
                        central_path: PathBuf::from(&skill.central_path),
                        fingerprint,
                    }
                });
            OnboardingGroup {
                name,
                has_conflict: uniq > 1,
                variants,
                hub_skill,
            }
        })
        .collect();

    Ok(OnboardingPlan {
        total_tools_scanned: scanned,
        total_skills_found: all_detected.len() + plugin_skill_count,
        groups,
    })
}

const CLAUDE_PLUGIN_TOOL_KEY: &str = "claude_code_plugin";

#[derive(Debug, Deserialize)]
struct ClaudePluginRegistry {
    #[serde(default)]
    plugins: HashMap<String, Vec<ClaudePluginInstall>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudePluginInstall {
    #[serde(default)]
    scope: String,
    install_path: Option<PathBuf>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudePluginManifest {
    skills: Option<ClaudePluginSkillPaths>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ClaudePluginSkillPaths {
    One(String),
    Many(Vec<String>),
}

impl ClaudePluginSkillPaths {
    fn paths(self) -> Vec<String> {
        match self {
            Self::One(path) => vec![path],
            Self::Many(paths) => paths,
        }
    }
}

fn resolve_claude_config_dir(home: &Path) -> PathBuf {
    let Some(value) = std::env::var_os("CLAUDE_CONFIG_DIR") else {
        return home.join(".claude");
    };
    let path = PathBuf::from(value);
    if let Ok(relative) = path.strip_prefix("~") {
        return home.join(relative);
    }
    path
}

fn discover_claude_plugin_skills(claude_config_dir: &Path) -> Vec<OnboardingVariant> {
    let registry_path = claude_config_dir.join("plugins/installed_plugins.json");
    let Ok(content) = fs::read_to_string(&registry_path) else {
        return Vec::new();
    };
    let registry: ClaudePluginRegistry = match serde_json::from_str(&content) {
        Ok(registry) => registry,
        Err(err) => {
            log::warn!(
                "[onboarding] failed to parse Claude plugin registry {:?}: {}",
                registry_path,
                err
            );
            return Vec::new();
        }
    };

    let mut variants = Vec::new();
    let mut seen_paths = HashSet::new();
    for (plugin_name, installs) in registry.plugins {
        for install in installs {
            if install.scope != "user" {
                continue;
            }
            let Some(install_path) = install.install_path else {
                continue;
            };
            let Ok(plugin_root) = fs::canonicalize(install_path) else {
                continue;
            };
            if !plugin_root.is_dir() {
                continue;
            }

            let mut candidates = Vec::new();
            add_skill_dir_candidate(&plugin_root, &plugin_root, &mut candidates);
            add_child_skill_candidates(&plugin_root.join("skills"), &plugin_root, &mut candidates);
            for declared_path in read_declared_plugin_skill_paths(&plugin_root) {
                add_declared_skill_candidates(
                    &plugin_root.join(declared_path),
                    &plugin_root,
                    &mut candidates,
                );
            }

            for candidate in candidates {
                let Ok(canonical_path) = fs::canonicalize(&candidate) else {
                    continue;
                };
                if !seen_paths.insert(canonical_path.clone()) {
                    continue;
                }
                let name = if canonical_path == plugin_root {
                    plugin_name
                        .split_once('@')
                        .map_or_else(|| plugin_name.clone(), |(name, _)| name.to_string())
                } else {
                    let Some(name) = canonical_path
                        .file_name()
                        .map(|value| value.to_string_lossy().to_string())
                    else {
                        continue;
                    };
                    name
                };
                variants.push(OnboardingVariant {
                    tool: CLAUDE_PLUGIN_TOOL_KEY.to_string(),
                    name,
                    fingerprint: hash_dir(&canonical_path).ok(),
                    path: canonical_path,
                    is_link: false,
                    link_target: None,
                    plugin_name: Some(plugin_name.clone()),
                    plugin_version: install.version.clone(),
                    plugin_scope: Some(install.scope.clone()),
                    hub_relation: None,
                });
            }
        }
    }
    variants
}

fn read_declared_plugin_skill_paths(plugin_root: &Path) -> Vec<String> {
    let path = plugin_root.join(".claude-plugin/plugin.json");
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<ClaudePluginManifest>(&content)
        .ok()
        .and_then(|manifest| manifest.skills)
        .map(ClaudePluginSkillPaths::paths)
        .unwrap_or_default()
}

fn add_declared_skill_candidates(path: &Path, plugin_root: &Path, out: &mut Vec<PathBuf>) {
    if path.file_name().is_some_and(|name| name == "SKILL.md") {
        if let Some(parent) = path.parent() {
            add_skill_dir_candidate(parent, plugin_root, out);
        }
        return;
    }
    if add_skill_dir_candidate(path, plugin_root, out) {
        return;
    }
    add_child_skill_candidates(path, plugin_root, out);
}

fn add_child_skill_candidates(path: &Path, plugin_root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        add_skill_dir_candidate(&entry.path(), plugin_root, out);
    }
}

fn add_skill_dir_candidate(path: &Path, plugin_root: &Path, out: &mut Vec<PathBuf>) -> bool {
    let Ok(canonical_root) = fs::canonicalize(plugin_root) else {
        return false;
    };
    let Ok(canonical_path) = fs::canonicalize(path) else {
        return false;
    };
    if !canonical_path.starts_with(canonical_root)
        || !canonical_path.is_dir()
        || !canonical_path.join("SKILL.md").is_file()
    {
        return false;
    }
    out.push(path.to_path_buf());
    true
}

fn filter_detected(
    detected: Vec<DetectedSkill>,
    exclude_root: Option<&Path>,
    exclude_managed_targets: Option<&std::collections::HashSet<String>>,
) -> Vec<DetectedSkill> {
    if exclude_root.is_none() && exclude_managed_targets.is_none() {
        return detected;
    }
    detected
        .into_iter()
        .filter(|skill| {
            if let Some(exclude_root) = exclude_root {
                if is_under(&skill.path, exclude_root) {
                    return false;
                }
                if let Some(target) = &skill.link_target {
                    if is_under(target, exclude_root) {
                        return false;
                    }
                }
            }
            if let Some(exclude) = exclude_managed_targets {
                if exclude.contains(&managed_target_key(skill.tool.as_key(), &skill.path)) {
                    return false;
                }
            }
            true
        })
        .collect()
}

fn is_under(path: &Path, base: &Path) -> bool {
    path.starts_with(base)
}

fn managed_target_key(tool: &str, path: &Path) -> String {
    let tool = tool.to_ascii_lowercase();
    let normalized = normalize_path_for_key(path);
    format!("{tool}\n{normalized}")
}

fn normalize_path_for_key(path: &Path) -> String {
    let normalized: PathBuf = path.components().collect();
    let s = normalized.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        s.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        s
    }
}

fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

#[cfg(test)]
#[path = "tests/onboarding.rs"]
mod tests;
