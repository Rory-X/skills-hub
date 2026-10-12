use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::skill_store::SkillStore;

const PROMPT_PROFILES_SETTING: &str = "prompt_profiles_v1";
const DEFAULT_CODEX_MAX_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptTool {
    Codex,
    ClaudeCode,
    Pi,
    DsHarness,
}

impl PromptTool {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude_code",
            Self::Pi => "pi",
            Self::DsHarness => "ds_harness",
        }
    }

    pub fn command_name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
            Self::Pi => "pi",
            Self::DsHarness => "dsh",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PromptProfile {
    pub id: String,
    pub tool: PromptTool,
    pub label: String,
    pub config_root: String,
    pub source: String,
    pub is_default: bool,
    pub config_root_exists: bool,
    pub tool_version: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PromptProfileInput {
    #[serde(default)]
    pub id: String,
    pub tool: PromptTool,
    pub label: String,
    pub config_root: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct StoredPromptProfile {
    id: String,
    tool: PromptTool,
    label: String,
    config_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CodexPromptConfig {
    pub config_path: String,
    pub project_doc_max_bytes: usize,
    pub fallback_filenames: Vec<String>,
    pub project_root_markers: Vec<String>,
    pub warning: Option<String>,
}

pub fn list_prompt_profiles(store: &SkillStore) -> Result<Vec<PromptProfile>> {
    let home = dirs::home_dir().context("failed to resolve home directory")?;
    let mut profiles = Vec::new();
    push_builtin_profile(
        &mut profiles,
        PromptTool::Codex,
        "codex:default",
        "Codex Default",
        home.join(".codex"),
        "default",
        true,
    );
    push_environment_profile(
        &mut profiles,
        PromptTool::Codex,
        "CODEX_HOME",
        "codex:environment",
        "Codex Environment",
    );
    push_builtin_profile(
        &mut profiles,
        PromptTool::ClaudeCode,
        "claude_code:default",
        "Claude Code Default",
        home.join(".claude"),
        "default",
        true,
    );
    push_environment_profile(
        &mut profiles,
        PromptTool::ClaudeCode,
        "CLAUDE_CONFIG_DIR",
        "claude_code:environment",
        "Claude Code Environment",
    );
    push_builtin_profile(
        &mut profiles,
        PromptTool::Pi,
        "pi:default",
        "Pi Default",
        home.join(".pi/agent"),
        "default",
        true,
    );
    push_builtin_profile(
        &mut profiles,
        PromptTool::DsHarness,
        "ds_harness:default",
        "DeepSeek Harness Default",
        home.join(".dsh"),
        "default",
        true,
    );
    push_environment_profile(
        &mut profiles,
        PromptTool::DsHarness,
        "DSH_HOME",
        "ds_harness:environment",
        "DeepSeek Harness Environment",
    );

    let stored = store
        .get_setting(PROMPT_PROFILES_SETTING)?
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<StoredPromptProfile>>(raw).ok())
        .unwrap_or_default();
    for profile in stored {
        let root = expand_home_path(&profile.config_root)?;
        profiles.push(runtime_profile(
            profile.id,
            profile.tool,
            profile.label,
            root,
            "custom",
            false,
        ));
    }

    deduplicate_profiles(&mut profiles);
    Ok(profiles)
}

pub fn save_custom_prompt_profiles(
    store: &SkillStore,
    inputs: Vec<PromptProfileInput>,
) -> Result<Vec<PromptProfile>> {
    let mut ids = HashSet::new();
    let mut stored = Vec::new();
    for input in inputs {
        let label = input.label.trim();
        if label.is_empty() {
            anyhow::bail!("prompt profile label is empty");
        }
        let root = expand_home_path(&input.config_root)?;
        if !root.is_absolute() {
            anyhow::bail!("prompt profile path must be absolute: {:?}", root);
        }
        let id = if input.id.trim().is_empty() {
            format!("{}:custom:{}", input.tool.as_key(), Uuid::new_v4())
        } else {
            input.id.trim().to_string()
        };
        if id.ends_with(":default") || id.ends_with(":environment") {
            anyhow::bail!("built-in prompt profile cannot be replaced: {id}");
        }
        if !ids.insert(id.clone()) {
            anyhow::bail!("duplicate prompt profile id: {id}");
        }
        stored.push(StoredPromptProfile {
            id,
            tool: input.tool,
            label: label.to_string(),
            config_root: root.to_string_lossy().to_string(),
        });
    }
    store.set_setting(PROMPT_PROFILES_SETTING, &serde_json::to_string(&stored)?)?;
    list_prompt_profiles(store)
}

pub fn resolve_prompt_profile(store: &SkillStore, profile_id: &str) -> Result<PromptProfile> {
    list_prompt_profiles(store)?
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .with_context(|| format!("prompt profile not found: {profile_id}"))
}

pub fn load_codex_prompt_config(profile: &PromptProfile) -> CodexPromptConfig {
    let config_path = PathBuf::from(&profile.config_root).join("config.toml");
    let mut result = CodexPromptConfig {
        config_path: config_path.to_string_lossy().to_string(),
        project_doc_max_bytes: DEFAULT_CODEX_MAX_BYTES,
        fallback_filenames: Vec::new(),
        project_root_markers: vec![".git".to_string()],
        warning: None,
    };
    let raw = match std::fs::read_to_string(&config_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return result,
        Err(error) => {
            result.warning = Some(format!("failed to read Codex config: {error}"));
            return result;
        }
    };
    let value = match raw.parse::<toml::Value>() {
        Ok(value) => value,
        Err(error) => {
            result.warning = Some(format!("failed to parse Codex config: {error}"));
            return result;
        }
    };
    if let Some(limit) = value
        .get("project_doc_max_bytes")
        .and_then(toml::Value::as_integer)
        .filter(|value| *value > 0)
    {
        result.project_doc_max_bytes = limit as usize;
    }
    if let Some(fallbacks) = value
        .get("project_doc_fallback_filenames")
        .and_then(toml::Value::as_array)
    {
        result.fallback_filenames = fallbacks
            .iter()
            .filter_map(toml::Value::as_str)
            .filter(|name| is_safe_filename(name))
            .map(str::to_string)
            .collect();
    }
    if let Some(markers) = value
        .get("project_root_markers")
        .and_then(toml::Value::as_array)
    {
        let markers = markers
            .iter()
            .filter_map(toml::Value::as_str)
            .filter(|name| is_safe_filename(name))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !markers.is_empty() {
            result.project_root_markers = markers;
        }
    }
    result
}

pub fn load_claude_md_excludes(profile: &PromptProfile, project_root: &Path) -> Vec<String> {
    let paths = [
        PathBuf::from(&profile.config_root).join("settings.json"),
        project_root.join(".claude/settings.json"),
        project_root.join(".claude/settings.local.json"),
    ];
    let mut excludes = Vec::new();
    for path in paths {
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if let Some(items) = value
            .get("claudeMdExcludes")
            .and_then(|value| value.as_array())
        {
            excludes.extend(
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string),
            );
        }
    }
    excludes.sort();
    excludes.dedup();
    excludes
}

pub fn claude_managed_policy_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        return PathBuf::from("/Library/Application Support/ClaudeCode/CLAUDE.md");
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        return PathBuf::from("/etc/claude-code/CLAUDE.md");
    }
    #[cfg(target_os = "windows")]
    {
        let program_files = env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        return program_files.join("ClaudeCode/CLAUDE.md");
    }
    #[allow(unreachable_code)]
    PathBuf::from("/etc/claude-code/CLAUDE.md")
}

pub fn should_skip_prompt_scan_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            ".git"
                | ".hg"
                | ".svn"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | ".next"
                | ".turbo"
                | "vendor"
        )
    )
}

fn push_builtin_profile(
    profiles: &mut Vec<PromptProfile>,
    tool: PromptTool,
    id: &str,
    label: &str,
    root: PathBuf,
    source: &str,
    is_default: bool,
) {
    profiles.push(runtime_profile(
        id.to_string(),
        tool,
        label.to_string(),
        root,
        source,
        is_default,
    ));
}

fn push_environment_profile(
    profiles: &mut Vec<PromptProfile>,
    tool: PromptTool,
    variable: &str,
    id: &str,
    label: &str,
) {
    let Some(value) = env::var_os(variable).filter(|value| !value.is_empty()) else {
        return;
    };
    let root = PathBuf::from(value);
    let duplicate = profiles
        .iter()
        .any(|profile| profile.tool == tool && paths_equal(Path::new(&profile.config_root), &root));
    if !duplicate {
        push_builtin_profile(profiles, tool, id, label, root, "environment", false);
    }
}

fn runtime_profile(
    id: String,
    tool: PromptTool,
    label: String,
    root: PathBuf,
    source: &str,
    is_default: bool,
) -> PromptProfile {
    PromptProfile {
        id,
        tool,
        label,
        config_root_exists: root.exists(),
        config_root: root.to_string_lossy().to_string(),
        source: source.to_string(),
        is_default,
        tool_version: detect_tool_version(tool),
    }
}

fn deduplicate_profiles(profiles: &mut Vec<PromptProfile>) {
    let mut seen = HashSet::new();
    profiles.retain(|profile| {
        seen.insert((
            profile.tool,
            normalized_path_key(Path::new(&profile.config_root)),
        ))
    });
}

fn detect_tool_version(tool: PromptTool) -> Option<String> {
    let output = Command::new(tool.command_name())
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!raw.is_empty()).then_some(raw)
}

fn expand_home_path(input: &str) -> Result<PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("prompt profile path is empty");
    }
    if trimmed == "~" {
        return dirs::home_dir().context("failed to resolve home directory");
    }
    if let Some(relative) = trimmed.strip_prefix("~/") {
        return Ok(dirs::home_dir()
            .context("failed to resolve home directory")?
            .join(relative));
    }
    Ok(PathBuf::from(trimmed))
}

fn is_safe_filename(value: &str) -> bool {
    let path = Path::new(value);
    !value.trim().is_empty()
        && path.components().count() == 1
        && path.file_name().and_then(|name| name.to_str()) == Some(value)
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    normalized_path_key(left) == normalized_path_key(right)
}

fn normalized_path_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_lowercase()
}

#[cfg(test)]
#[path = "tests/prompt_adapters.rs"]
mod tests;
