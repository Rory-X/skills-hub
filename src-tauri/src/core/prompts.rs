use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use git2::{Repository, Status};
use globset::{Glob, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use walkdir::{DirEntry, WalkDir};

use super::prompt_adapters::{
    claude_managed_policy_path, load_claude_md_excludes, load_codex_prompt_config,
    resolve_prompt_profile, should_skip_prompt_scan_dir, PromptProfile, PromptTool,
};
use super::skill_store::{ProjectRecord, PromptRevisionRecord, SkillStore};

const MAX_EDITABLE_PROMPT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CLAUDE_IMPORT_DEPTH: usize = 4;
const PROMPT_REVISION_RETENTION: usize = 20;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSourceScope {
    Managed,
    Global,
    Project,
    Local,
    Import,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSourceKind {
    Primary,
    Override,
    Fallback,
    Local,
    Rule,
    Import,
    ManagedPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptSource {
    pub id: String,
    pub tool: PromptTool,
    pub profile_id: String,
    pub project_id: Option<String>,
    pub scope: PromptSourceScope,
    pub kind: PromptSourceKind,
    pub path: String,
    pub display_path: String,
    pub real_path: Option<String>,
    pub exists: bool,
    pub is_symlink: bool,
    pub link_target: Option<String>,
    pub writable: bool,
    pub git_status: Option<String>,
    pub content_hash: Option<String>,
    pub modified_at: Option<i64>,
    pub byte_len: Option<u64>,
    pub empty: bool,
    pub encoding: String,
    pub can_create: bool,
    pub can_delete: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptDocument {
    pub source: PromptSource,
    pub content: String,
    pub content_hash: Option<String>,
    pub line_ending: String,
    pub has_bom: bool,
    pub read_only: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PromptScenario {
    pub profile_id: String,
    pub project_id: Option<String>,
    pub working_directory: Option<String>,
    pub target_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptChainSource {
    pub source: PromptSource,
    pub phase: String,
    pub order: usize,
    pub loaded_bytes: u64,
    pub truncated: bool,
    pub imported_from: Option<String>,
    pub path_patterns: Vec<String>,
    pub matches_target: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptInstructionChain {
    pub tool: PromptTool,
    pub profile_id: String,
    pub project_id: Option<String>,
    pub working_directory: Option<String>,
    pub target_path: Option<String>,
    pub startup_sources: Vec<PromptChainSource>,
    pub conditional_sources: Vec<PromptChainSource>,
    pub warnings: Vec<String>,
    pub accuracy: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PromptFileRequest {
    pub profile_id: String,
    pub project_id: Option<String>,
    pub path: String,
    #[serde(default)]
    pub allow_external: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PromptSaveRequest {
    pub profile_id: String,
    pub project_id: Option<String>,
    pub path: String,
    pub content: String,
    pub expected_hash: Option<String>,
    pub line_ending: String,
    pub has_bom: bool,
    #[serde(default)]
    pub allow_external: bool,
    #[serde(default)]
    pub force: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PromptDeleteRequest {
    pub profile_id: String,
    pub project_id: Option<String>,
    pub path: String,
    pub expected_hash: Option<String>,
    #[serde(default)]
    pub allow_external: bool,
    #[serde(default)]
    pub force: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PromptRestoreRequest {
    pub revision_id: String,
    pub expected_hash: Option<String>,
    #[serde(default)]
    pub allow_external: bool,
    #[serde(default)]
    pub force: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptMutationResult {
    pub source: PromptSource,
    pub revision_id: String,
    pub created: bool,
    pub deleted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptRevisionSummary {
    pub id: String,
    pub logical_path: String,
    pub real_path: Option<String>,
    pub tool: String,
    pub profile_id: String,
    pub project_id: Option<String>,
    pub operation: String,
    pub content_hash: String,
    pub byte_len: usize,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PromptRevisionDocument {
    pub summary: PromptRevisionSummary,
    pub content: String,
    pub encoding: String,
    pub line_ending: String,
    pub has_bom: bool,
}

#[derive(Clone)]
struct SourceSpec {
    scope: PromptSourceScope,
    kind: PromptSourceKind,
    can_create: bool,
    can_delete: bool,
    warning: Option<String>,
}

pub fn list_prompt_sources(
    store: &SkillStore,
    profile_id: &str,
    project_id: Option<&str>,
) -> Result<Vec<PromptSource>> {
    let profile = resolve_prompt_profile(store, profile_id)?;
    let project = project_id
        .map(|id| {
            store
                .get_project(id)?
                .with_context(|| format!("project not found: {id}"))
        })
        .transpose()?;
    let mut specs = BTreeMap::new();
    discover_global_specs(&profile, &mut specs)?;
    if let Some(project) = &project {
        discover_project_specs(&profile, project, &mut specs)?;
    }
    if profile.tool == PromptTool::ClaudeCode {
        discover_claude_import_specs(&profile, project.as_ref(), &mut specs)?;
    }
    let project_root = project.as_ref().map(|value| Path::new(&value.root_path));
    let mut sources = specs
        .into_iter()
        .map(|(path, spec)| build_prompt_source(&profile, project_id, project_root, &path, spec))
        .collect::<Result<Vec<_>>>()?;
    sources.sort_by(|left, right| {
        source_scope_order(left.scope)
            .cmp(&source_scope_order(right.scope))
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(sources)
}

pub fn read_prompt_document(source: PromptSource) -> Result<PromptDocument> {
    let path = PathBuf::from(&source.path);
    if !source.exists {
        return Ok(PromptDocument {
            content_hash: None,
            content: String::new(),
            line_ending: "lf".to_string(),
            has_bom: false,
            read_only: false,
            warnings: Vec::new(),
            source,
        });
    }
    let metadata =
        std::fs::metadata(&path).with_context(|| format!("read prompt metadata for {:?}", path))?;
    if metadata.len() > MAX_EDITABLE_PROMPT_BYTES {
        anyhow::bail!(
            "PROMPT_TOO_LARGE|{}|{}",
            metadata.len(),
            MAX_EDITABLE_PROMPT_BYTES
        );
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read prompt file {:?}", path))?;
    let has_bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let content_bytes = if has_bom { &bytes[3..] } else { &bytes };
    let (content, read_only, mut warnings) = match String::from_utf8(content_bytes.to_vec()) {
        Ok(content) => (content, false, Vec::new()),
        Err(_) => (
            String::from_utf8_lossy(content_bytes).to_string(),
            true,
            vec!["prompt file is not valid UTF-8 and is read-only".to_string()],
        ),
    };
    if !source.writable {
        warnings.push("prompt file is not writable".to_string());
    }
    let line_ending = if content.contains("\r\n") {
        "crlf"
    } else {
        "lf"
    };
    Ok(PromptDocument {
        content_hash: Some(hash_bytes(&bytes)),
        content,
        line_ending: line_ending.to_string(),
        has_bom,
        read_only: read_only || !source.writable,
        warnings,
        source,
    })
}

pub fn read_authorized_prompt_document(
    store: &SkillStore,
    request: &PromptFileRequest,
) -> Result<PromptDocument> {
    let mut read_request = request.clone();
    read_request.allow_external = true;
    let source = find_authorized_prompt_source(store, &read_request)?;
    read_prompt_document(source)
}

pub fn save_prompt_document(
    store: &SkillStore,
    request: &PromptSaveRequest,
) -> Result<PromptMutationResult> {
    let source = find_authorized_prompt_source(
        store,
        &PromptFileRequest {
            profile_id: request.profile_id.clone(),
            project_id: request.project_id.clone(),
            path: request.path.clone(),
            allow_external: request.allow_external,
        },
    )?;
    if source.kind == PromptSourceKind::ManagedPolicy {
        anyhow::bail!("PROMPT_READ_ONLY|managed policy files are not editable");
    }
    let created = !source.exists;
    check_prompt_conflict(&source, request.expected_hash.as_deref(), request.force)?;
    let revision_id = snapshot_prompt(
        store,
        &source,
        if created {
            "before_create"
        } else {
            "before_save"
        },
    )?;
    let bytes = encode_prompt_content(&request.content, &request.line_ending, request.has_bom);
    write_prompt_bytes(&source, &bytes)?;
    let updated = refresh_prompt_source(store, &source)?;
    Ok(PromptMutationResult {
        source: updated,
        revision_id,
        created,
        deleted: false,
    })
}

pub fn delete_prompt_document(
    store: &SkillStore,
    request: &PromptDeleteRequest,
) -> Result<PromptMutationResult> {
    let source = find_authorized_prompt_source(
        store,
        &PromptFileRequest {
            profile_id: request.profile_id.clone(),
            project_id: request.project_id.clone(),
            path: request.path.clone(),
            allow_external: request.allow_external,
        },
    )?;
    if !source.exists {
        anyhow::bail!("PROMPT_NOT_FOUND|{}", source.path);
    }
    if !source.can_delete || source.kind == PromptSourceKind::ManagedPolicy {
        anyhow::bail!("PROMPT_READ_ONLY|prompt source cannot be deleted");
    }
    check_prompt_conflict(&source, request.expected_hash.as_deref(), request.force)?;
    let revision_id = snapshot_prompt(store, &source, "before_delete")?;
    std::fs::remove_file(&source.path)
        .with_context(|| format!("delete prompt file {:?}", source.path))?;
    let mut deleted = source;
    deleted.exists = false;
    deleted.content_hash = None;
    deleted.modified_at = None;
    deleted.byte_len = None;
    deleted.empty = true;
    Ok(PromptMutationResult {
        source: deleted,
        revision_id,
        created: false,
        deleted: true,
    })
}

pub fn list_prompt_revision_summaries(
    store: &SkillStore,
    logical_path: &str,
) -> Result<Vec<PromptRevisionSummary>> {
    Ok(store
        .list_prompt_revisions(logical_path, PROMPT_REVISION_RETENTION)?
        .iter()
        .map(prompt_revision_summary)
        .collect())
}

pub fn read_prompt_revision(
    store: &SkillStore,
    revision_id: &str,
) -> Result<PromptRevisionDocument> {
    let revision = store
        .get_prompt_revision(revision_id)?
        .with_context(|| format!("prompt revision not found: {revision_id}"))?;
    Ok(prompt_revision_document(&revision))
}

pub fn restore_prompt_revision(
    store: &SkillStore,
    request: &PromptRestoreRequest,
) -> Result<PromptMutationResult> {
    let revision = store
        .get_prompt_revision(&request.revision_id)?
        .with_context(|| format!("prompt revision not found: {}", request.revision_id))?;
    let source = source_for_revision_restore(store, &revision, request.allow_external)?;
    let created = !source.exists;
    check_prompt_conflict(&source, request.expected_hash.as_deref(), request.force)?;
    let backup_id = snapshot_prompt(store, &source, "before_restore")?;
    write_prompt_bytes(&source, &revision.content)?;
    let updated = refresh_prompt_source(store, &source)?;
    Ok(PromptMutationResult {
        source: updated,
        revision_id: backup_id,
        created,
        deleted: false,
    })
}

pub fn resolve_prompt_instruction_chain(
    store: &SkillStore,
    scenario: &PromptScenario,
) -> Result<PromptInstructionChain> {
    let profile = resolve_prompt_profile(store, &scenario.profile_id)?;
    let project = scenario
        .project_id
        .as_deref()
        .map(|id| {
            store
                .get_project(id)?
                .with_context(|| format!("project not found: {id}"))
        })
        .transpose()?;
    match profile.tool {
        PromptTool::Codex => resolve_codex_chain(profile, project, scenario),
        PromptTool::ClaudeCode => resolve_claude_chain(profile, project, scenario),
        PromptTool::Pi => resolve_pi_chain(profile, project, scenario),
        PromptTool::DsHarness => resolve_ds_harness_chain(profile, project, scenario),
    }
}

fn discover_global_specs(
    profile: &PromptProfile,
    specs: &mut BTreeMap<PathBuf, SourceSpec>,
) -> Result<()> {
    let root = PathBuf::from(&profile.config_root);
    match profile.tool {
        PromptTool::Codex => {
            insert_spec(
                specs,
                root.join("AGENTS.override.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Override,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("AGENTS.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
        }
        PromptTool::ClaudeCode => {
            let managed = claude_managed_policy_path();
            if managed.exists() {
                insert_spec(
                    specs,
                    managed,
                    PromptSourceScope::Managed,
                    PromptSourceKind::ManagedPolicy,
                    false,
                    false,
                    Some("organization-managed policy".to_string()),
                );
            }
            insert_spec(
                specs,
                root.join("CLAUDE.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
            discover_rule_files(&root.join("rules"), PromptSourceScope::Global, specs)?;
        }
        PromptTool::Pi => {
            insert_spec(
                specs,
                root.join("AGENTS.override.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Override,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("AGENTS.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("SYSTEM.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Primary,
                true,
                true,
                Some("replaces the default Pi system prompt".to_string()),
            );
            insert_spec(
                specs,
                root.join("APPEND_SYSTEM.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Fallback,
                true,
                true,
                Some("appends to the Pi system prompt".to_string()),
            );
        }
        PromptTool::DsHarness => {
            insert_spec(
                specs,
                root.join("AGENTS.md"),
                PromptSourceScope::Global,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
        }
    }
    Ok(())
}

fn discover_project_specs(
    profile: &PromptProfile,
    project: &ProjectRecord,
    specs: &mut BTreeMap<PathBuf, SourceSpec>,
) -> Result<()> {
    let root = PathBuf::from(&project.root_path);
    if !root.is_dir() {
        anyhow::bail!("project path is no longer available: {}", project.root_path);
    }
    match profile.tool {
        PromptTool::Codex => {
            let config = load_codex_prompt_config(profile);
            let mut names = vec!["AGENTS.override.md".to_string(), "AGENTS.md".to_string()];
            names.extend(config.fallback_filenames);
            discover_named_project_files(&root, &names, profile.tool, specs)?;
            insert_spec(
                specs,
                root.join("AGENTS.override.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Override,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("AGENTS.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
        }
        PromptTool::ClaudeCode => {
            discover_named_project_files(
                &root,
                &["CLAUDE.md".to_string(), "CLAUDE.local.md".to_string()],
                profile.tool,
                specs,
            )?;
            discover_rule_files(
                &root.join(".claude/rules"),
                PromptSourceScope::Project,
                specs,
            )?;
            insert_spec(
                specs,
                root.join("CLAUDE.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join(".claude/CLAUDE.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("CLAUDE.local.md"),
                PromptSourceScope::Local,
                PromptSourceKind::Local,
                true,
                true,
                None,
            );
        }
        PromptTool::Pi => {
            discover_named_project_files(
                &root,
                &[
                    "AGENTS.override.md".to_string(),
                    "AGENTS.md".to_string(),
                    "CLAUDE.md".to_string(),
                ],
                profile.tool,
                specs,
            )?;
            insert_spec(
                specs,
                root.join("AGENTS.override.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Override,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("AGENTS.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("CLAUDE.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Fallback,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join(".pi/SYSTEM.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Primary,
                true,
                true,
                Some("loaded only after the project is trusted by Pi".to_string()),
            );
            insert_spec(
                specs,
                root.join(".pi/APPEND_SYSTEM.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Fallback,
                true,
                true,
                Some("loaded only after the project is trusted by Pi".to_string()),
            );
        }
        PromptTool::DsHarness => {
            discover_named_project_files(
                &root,
                &[
                    "AGENTS.md".to_string(),
                    "CLAUDE.md".to_string(),
                    "AGENTS.local.md".to_string(),
                    "CLAUDE.local.md".to_string(),
                ],
                profile.tool,
                specs,
            )?;
            insert_spec(
                specs,
                root.join("AGENTS.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("CLAUDE.md"),
                PromptSourceScope::Project,
                PromptSourceKind::Primary,
                true,
                true,
                None,
            );
            insert_spec(
                specs,
                root.join("AGENTS.local.md"),
                PromptSourceScope::Local,
                PromptSourceKind::Local,
                true,
                true,
                Some("local overlay, usually kept out of version control".to_string()),
            );
            insert_spec(
                specs,
                root.join("CLAUDE.local.md"),
                PromptSourceScope::Local,
                PromptSourceKind::Local,
                true,
                true,
                Some("local overlay, usually kept out of version control".to_string()),
            );
        }
    }
    Ok(())
}

fn discover_named_project_files(
    root: &Path,
    names: &[String],
    tool: PromptTool,
    specs: &mut BTreeMap<PathBuf, SourceSpec>,
) -> Result<()> {
    let names = names.iter().map(String::as_str).collect::<HashSet<_>>();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_visit_prompt_entry)
    {
        let entry = entry.with_context(|| format!("scan prompt files under {:?}", root))?;
        if !entry.file_type().is_file() && !entry.file_type().is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        if !names.contains(name) {
            continue;
        }
        let (scope, kind) = match (tool, name) {
            (PromptTool::Codex, "AGENTS.override.md") => {
                (PromptSourceScope::Project, PromptSourceKind::Override)
            }
            (PromptTool::Codex, "AGENTS.md") => {
                (PromptSourceScope::Project, PromptSourceKind::Primary)
            }
            (PromptTool::Codex, _) => (PromptSourceScope::Project, PromptSourceKind::Fallback),
            (PromptTool::ClaudeCode, "CLAUDE.local.md") => {
                (PromptSourceScope::Local, PromptSourceKind::Local)
            }
            (PromptTool::ClaudeCode, _) => (PromptSourceScope::Project, PromptSourceKind::Primary),
            (PromptTool::Pi, "AGENTS.override.md") => {
                (PromptSourceScope::Project, PromptSourceKind::Override)
            }
            (PromptTool::Pi, "AGENTS.md") => {
                (PromptSourceScope::Project, PromptSourceKind::Primary)
            }
            (PromptTool::Pi, "CLAUDE.md") => {
                (PromptSourceScope::Project, PromptSourceKind::Fallback)
            }
            (PromptTool::Pi, _) => (PromptSourceScope::Project, PromptSourceKind::Fallback),
            (PromptTool::DsHarness, "AGENTS.md" | "CLAUDE.md") => {
                (PromptSourceScope::Project, PromptSourceKind::Primary)
            }
            (PromptTool::DsHarness, _) => (PromptSourceScope::Local, PromptSourceKind::Local),
        };
        insert_spec(
            specs,
            entry.path().to_path_buf(),
            scope,
            kind,
            false,
            true,
            None,
        );
    }
    Ok(())
}

fn discover_rule_files(
    root: &Path,
    scope: PromptSourceScope,
    specs: &mut BTreeMap<PathBuf, SourceSpec>,
) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("scan Claude rules under {:?}", root))?;
        if (entry.file_type().is_file() || entry.file_type().is_symlink())
            && entry.path().extension().and_then(|value| value.to_str()) == Some("md")
        {
            insert_spec(
                specs,
                entry.path().to_path_buf(),
                scope,
                PromptSourceKind::Rule,
                false,
                true,
                None,
            );
        }
    }
    Ok(())
}

fn discover_claude_import_specs(
    profile: &PromptProfile,
    project: Option<&ProjectRecord>,
    specs: &mut BTreeMap<PathBuf, SourceSpec>,
) -> Result<()> {
    let mut pending = specs
        .keys()
        .filter(|path| path.is_file())
        .cloned()
        .map(|path| (path, 0usize))
        .collect::<Vec<_>>();
    let mut visited = HashSet::new();
    while let Some((source, depth)) = pending.pop() {
        let key = normalized_path(&source);
        if !visited.insert(key) || depth >= MAX_CLAUDE_IMPORT_DEPTH {
            continue;
        }
        let Ok(content) = read_utf8_prompt(&source) else {
            continue;
        };
        for import in parse_claude_imports(&content) {
            let resolved = resolve_claude_import_path(&source, &import)?;
            let external = project
                .map(|record| !path_is_within(&resolved, Path::new(&record.root_path)))
                .unwrap_or_else(|| !path_is_within(&resolved, Path::new(&profile.config_root)));
            insert_spec(
                specs,
                resolved.clone(),
                PromptSourceScope::Import,
                PromptSourceKind::Import,
                false,
                false,
                external.then(|| "external Claude import".to_string()),
            );
            if resolved.is_file() {
                pending.push((resolved, depth + 1));
            }
        }
    }
    Ok(())
}

fn resolve_codex_chain(
    profile: PromptProfile,
    project: Option<ProjectRecord>,
    scenario: &PromptScenario,
) -> Result<PromptInstructionChain> {
    let mut startup = Vec::new();
    let conditional = Vec::new();
    let mut warnings = Vec::new();
    let root = PathBuf::from(&profile.config_root);
    for candidate in [root.join("AGENTS.override.md"), root.join("AGENTS.md")] {
        if prompt_file_has_content(&candidate) {
            push_chain_source(
                &profile,
                scenario.project_id.as_deref(),
                project.as_ref().map(|value| Path::new(&value.root_path)),
                &candidate,
                PromptSourceScope::Global,
                if candidate.ends_with("AGENTS.override.md") {
                    PromptSourceKind::Override
                } else {
                    PromptSourceKind::Primary
                },
                "startup",
                None,
                Vec::new(),
                None,
                false,
                &mut startup,
            )?;
            break;
        }
    }

    let config = load_codex_prompt_config(&profile);
    if let Some(warning) = config.warning.clone() {
        warnings.push(warning);
    }
    if let Some(project) = &project {
        let registered_root = PathBuf::from(&project.root_path);
        let working_directory = scenario
            .working_directory
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| registered_root.clone());
        ensure_path_in_project(&working_directory, &registered_root)?;
        let project_root = detect_codex_project_root(
            &working_directory,
            &registered_root,
            &config.project_root_markers,
        );
        let mut project_bytes = 0usize;
        for directory in path_chain(&project_root, &working_directory)? {
            let mut names = vec!["AGENTS.override.md".to_string(), "AGENTS.md".to_string()];
            names.extend(config.fallback_filenames.clone());
            if let Some(path) = names
                .into_iter()
                .map(|name| directory.join(name))
                .find(|path| prompt_file_has_content(path))
            {
                let byte_len = std::fs::metadata(&path)
                    .map(|value| value.len())
                    .unwrap_or(0);
                let remaining = config.project_doc_max_bytes.saturating_sub(project_bytes);
                let truncated = byte_len as usize > remaining;
                push_chain_source(
                    &profile,
                    scenario.project_id.as_deref(),
                    Some(&registered_root),
                    &path,
                    PromptSourceScope::Project,
                    codex_kind_for_path(&path, &config.fallback_filenames),
                    "startup",
                    None,
                    Vec::new(),
                    None,
                    truncated,
                    &mut startup,
                )?;
                project_bytes = project_bytes.saturating_add(byte_len as usize);
                if project_bytes >= config.project_doc_max_bytes {
                    warnings.push(format!(
                        "Codex project instructions reached the {} byte limit",
                        config.project_doc_max_bytes
                    ));
                    break;
                }
            }
        }
    }
    renumber_chain(&mut startup);
    Ok(PromptInstructionChain {
        tool: profile.tool,
        profile_id: profile.id,
        project_id: scenario.project_id.clone(),
        working_directory: scenario.working_directory.clone(),
        target_path: scenario.target_path.clone(),
        startup_sources: startup,
        conditional_sources: conditional,
        warnings,
        accuracy: "calculated".to_string(),
    })
}

const DEFAULT_DSH_SOURCE_MAX_BYTES: u64 = 1024 * 1024;
const DSH_CANDIDATE_FILE_NAMES: [&str; 4] = [
    "AGENTS.md",
    "CLAUDE.md",
    "AGENTS.local.md",
    "CLAUDE.local.md",
];

fn ds_harness_kind_for_file_name(name: &str) -> PromptSourceKind {
    if name.ends_with(".local.md") {
        PromptSourceKind::Local
    } else {
        PromptSourceKind::Primary
    }
}

fn resolve_ds_harness_chain(
    profile: PromptProfile,
    project: Option<ProjectRecord>,
    scenario: &PromptScenario,
) -> Result<PromptInstructionChain> {
    let mut startup = Vec::new();
    let conditional = Vec::new();
    let mut warnings = Vec::new();
    let root = PathBuf::from(&profile.config_root);
    let global = root.join("AGENTS.md");
    if prompt_file_has_content(&global) {
        push_chain_source(
            &profile,
            scenario.project_id.as_deref(),
            project.as_ref().map(|value| Path::new(&value.root_path)),
            &global,
            PromptSourceScope::Global,
            PromptSourceKind::Primary,
            "startup",
            None,
            Vec::new(),
            None,
            false,
            &mut startup,
        )?;
    }
    if let Some(project) = &project {
        let registered_root = PathBuf::from(&project.root_path);
        let working_directory = scenario
            .working_directory
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| registered_root.clone());
        ensure_path_in_project(&working_directory, &registered_root)?;
        let project_root =
            detect_codex_project_root(&working_directory, &registered_root, &[".git".to_string()]);
        for directory in path_chain(&project_root, &working_directory)? {
            for name in DSH_CANDIDATE_FILE_NAMES {
                let path = directory.join(name);
                if !prompt_file_has_content(&path) {
                    continue;
                }
                let byte_len = std::fs::metadata(&path)
                    .map(|value| value.len())
                    .unwrap_or(0);
                let truncated = byte_len > DEFAULT_DSH_SOURCE_MAX_BYTES;
                if truncated {
                    warnings.push(format!(
                        "{name} exceeds the DeepSeek Harness {} byte per-source limit and will be truncated",
                        DEFAULT_DSH_SOURCE_MAX_BYTES
                    ));
                }
                push_chain_source(
                    &profile,
                    scenario.project_id.as_deref(),
                    Some(&registered_root),
                    &path,
                    PromptSourceScope::Project,
                    ds_harness_kind_for_file_name(name),
                    "startup",
                    None,
                    Vec::new(),
                    None,
                    truncated,
                    &mut startup,
                )?;
            }
        }
    }
    renumber_chain(&mut startup);
    Ok(PromptInstructionChain {
        tool: profile.tool,
        profile_id: profile.id,
        project_id: scenario.project_id.clone(),
        working_directory: scenario.working_directory.clone(),
        target_path: scenario.target_path.clone(),
        startup_sources: startup,
        conditional_sources: conditional,
        warnings,
        accuracy: "calculated".to_string(),
    })
}

fn resolve_pi_chain(
    profile: PromptProfile,
    project: Option<ProjectRecord>,
    scenario: &PromptScenario,
) -> Result<PromptInstructionChain> {
    let mut startup = Vec::new();
    let conditional = Vec::new();
    let warnings = Vec::new();
    let project_root = project
        .as_ref()
        .map(|value| PathBuf::from(&value.root_path));
    let root = PathBuf::from(&profile.config_root);

    for (path, kind, warning) in [
        (
            root.join("SYSTEM.md"),
            PromptSourceKind::Primary,
            Some("replaces the default Pi system prompt".to_string()),
        ),
        (
            root.join("APPEND_SYSTEM.md"),
            PromptSourceKind::Fallback,
            Some("appends to the Pi system prompt".to_string()),
        ),
    ] {
        if prompt_file_has_content(&path) {
            push_chain_source(
                &profile,
                scenario.project_id.as_deref(),
                project_root.as_deref(),
                &path,
                PromptSourceScope::Global,
                kind,
                "startup",
                None,
                Vec::new(),
                None,
                false,
                &mut startup,
            )?;
            if let Some(last) = startup.last_mut() {
                last.source.warning = warning;
            }
        }
    }

    for candidate in [root.join("AGENTS.override.md"), root.join("AGENTS.md")] {
        if prompt_file_has_content(&candidate) {
            push_chain_source(
                &profile,
                scenario.project_id.as_deref(),
                project_root.as_deref(),
                &candidate,
                PromptSourceScope::Global,
                if candidate.ends_with("AGENTS.override.md") {
                    PromptSourceKind::Override
                } else {
                    PromptSourceKind::Primary
                },
                "startup",
                None,
                Vec::new(),
                None,
                false,
                &mut startup,
            )?;
            break;
        }
    }

    if let Some(project) = &project {
        let registered_root = PathBuf::from(&project.root_path);
        let working_directory = scenario
            .working_directory
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| registered_root.clone());
        ensure_path_in_project(&working_directory, &registered_root)?;

        for directory in path_chain_from_filesystem_root(&working_directory) {
            if directory == root {
                continue;
            }
            if let Some(path) = [
                directory.join("AGENTS.override.md"),
                directory.join("AGENTS.md"),
                directory.join("CLAUDE.md"),
            ]
            .into_iter()
            .find(|path| prompt_file_has_content(path))
            {
                let kind = match path.file_name().and_then(|value| value.to_str()) {
                    Some("AGENTS.override.md") => PromptSourceKind::Override,
                    Some("AGENTS.md") => PromptSourceKind::Primary,
                    _ => PromptSourceKind::Fallback,
                };
                let scope = if path_is_within(&path, &registered_root) {
                    PromptSourceScope::Project
                } else {
                    PromptSourceScope::Global
                };
                push_chain_source(
                    &profile,
                    scenario.project_id.as_deref(),
                    Some(&registered_root),
                    &path,
                    scope,
                    kind,
                    "startup",
                    None,
                    Vec::new(),
                    None,
                    false,
                    &mut startup,
                )?;
            }
        }

        for (path, kind) in [
            (
                registered_root.join(".pi/SYSTEM.md"),
                PromptSourceKind::Primary,
            ),
            (
                registered_root.join(".pi/APPEND_SYSTEM.md"),
                PromptSourceKind::Fallback,
            ),
        ] {
            if prompt_file_has_content(&path) {
                push_chain_source(
                    &profile,
                    scenario.project_id.as_deref(),
                    Some(&registered_root),
                    &path,
                    PromptSourceScope::Project,
                    kind,
                    "startup",
                    None,
                    Vec::new(),
                    None,
                    false,
                    &mut startup,
                )?;
                if let Some(last) = startup.last_mut() {
                    last.source.warning =
                        Some("loaded only after the project is trusted by Pi".to_string());
                }
            }
        }
    }

    deduplicate_chain(&mut startup);
    renumber_chain(&mut startup);
    Ok(PromptInstructionChain {
        tool: profile.tool,
        profile_id: profile.id,
        project_id: scenario.project_id.clone(),
        working_directory: scenario.working_directory.clone(),
        target_path: scenario.target_path.clone(),
        startup_sources: startup,
        conditional_sources: conditional,
        warnings,
        accuracy: "calculated".to_string(),
    })
}

fn resolve_claude_chain(
    profile: PromptProfile,
    project: Option<ProjectRecord>,
    scenario: &PromptScenario,
) -> Result<PromptInstructionChain> {
    let mut startup = Vec::new();
    let mut conditional = Vec::new();
    let mut warnings = Vec::new();
    let project_root = project
        .as_ref()
        .map(|value| PathBuf::from(&value.root_path));
    let managed = claude_managed_policy_path();
    if prompt_file_has_content(&managed) {
        push_chain_source(
            &profile,
            scenario.project_id.as_deref(),
            project_root.as_deref(),
            &managed,
            PromptSourceScope::Managed,
            PromptSourceKind::ManagedPolicy,
            "startup",
            None,
            Vec::new(),
            None,
            false,
            &mut startup,
        )?;
    }
    let global = PathBuf::from(&profile.config_root).join("CLAUDE.md");
    if prompt_file_has_content(&global) {
        push_claude_source_with_imports(
            &profile,
            scenario,
            project_root.as_deref(),
            &global,
            PromptSourceScope::Global,
            PromptSourceKind::Primary,
            "startup",
            Vec::new(),
            None,
            &mut startup,
            &mut warnings,
        )?;
    }
    collect_claude_rules(
        &profile,
        scenario,
        project_root.as_deref(),
        &PathBuf::from(&profile.config_root).join("rules"),
        PromptSourceScope::Global,
        &mut startup,
        &mut conditional,
        &mut warnings,
    )?;

    if let Some(project) = &project {
        let registered_root = PathBuf::from(&project.root_path);
        let working_directory = scenario
            .working_directory
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| registered_root.clone());
        ensure_path_in_project(&working_directory, &registered_root)?;
        let excludes = load_claude_md_excludes(&profile, &registered_root);
        let exclude_set = compile_glob_set(&excludes);
        for directory in path_chain_from_filesystem_root(&working_directory) {
            for path in claude_files_at_directory(&directory, &registered_root) {
                if !prompt_file_has_content(&path)
                    || claude_path_excluded(&path, &registered_root, exclude_set.as_ref())
                {
                    continue;
                }
                let kind = if path.ends_with("CLAUDE.local.md") {
                    PromptSourceKind::Local
                } else {
                    PromptSourceKind::Primary
                };
                let scope = if kind == PromptSourceKind::Local {
                    PromptSourceScope::Local
                } else {
                    PromptSourceScope::Project
                };
                push_claude_source_with_imports(
                    &profile,
                    scenario,
                    Some(&registered_root),
                    &path,
                    scope,
                    kind,
                    "startup",
                    Vec::new(),
                    None,
                    &mut startup,
                    &mut warnings,
                )?;
            }
        }
        collect_claude_rules(
            &profile,
            scenario,
            Some(&registered_root),
            &registered_root.join(".claude/rules"),
            PromptSourceScope::Project,
            &mut startup,
            &mut conditional,
            &mut warnings,
        )?;

        if let Some(target) = scenario.target_path.as_deref().map(PathBuf::from) {
            ensure_path_in_project(&target, &registered_root)?;
            let target_directory = if target.is_dir() {
                target
            } else {
                target
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| registered_root.clone())
            };
            if target_directory.starts_with(&working_directory)
                && target_directory != working_directory
            {
                for directory in path_chain(&working_directory, &target_directory)?
                    .into_iter()
                    .skip(1)
                {
                    for path in claude_files_at_directory(&directory, &registered_root) {
                        if !prompt_file_has_content(&path)
                            || claude_path_excluded(&path, &registered_root, exclude_set.as_ref())
                        {
                            continue;
                        }
                        let kind = if path.ends_with("CLAUDE.local.md") {
                            PromptSourceKind::Local
                        } else {
                            PromptSourceKind::Primary
                        };
                        push_claude_source_with_imports(
                            &profile,
                            scenario,
                            Some(&registered_root),
                            &path,
                            if kind == PromptSourceKind::Local {
                                PromptSourceScope::Local
                            } else {
                                PromptSourceScope::Project
                            },
                            kind,
                            "conditional",
                            Vec::new(),
                            Some(true),
                            &mut conditional,
                            &mut warnings,
                        )?;
                    }
                }
            }
        }
    }
    deduplicate_chain(&mut startup);
    deduplicate_chain(&mut conditional);
    renumber_chain(&mut startup);
    renumber_chain(&mut conditional);
    Ok(PromptInstructionChain {
        tool: profile.tool,
        profile_id: profile.id,
        project_id: scenario.project_id.clone(),
        working_directory: scenario.working_directory.clone(),
        target_path: scenario.target_path.clone(),
        startup_sources: startup,
        conditional_sources: conditional,
        warnings,
        accuracy: "calculated".to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_claude_rules(
    profile: &PromptProfile,
    scenario: &PromptScenario,
    project_root: Option<&Path>,
    rules_root: &Path,
    scope: PromptSourceScope,
    startup: &mut Vec<PromptChainSource>,
    conditional: &mut Vec<PromptChainSource>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if !rules_root.is_dir() {
        return Ok(());
    }
    let mut paths = WalkDir::new(rules_root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            (entry.file_type().is_file() || entry.file_type().is_symlink())
                && entry.path().extension().and_then(|value| value.to_str()) == Some("md")
        })
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let content = read_utf8_prompt(&path).unwrap_or_default();
        let patterns = parse_claude_rule_paths(&content);
        if patterns.is_empty() {
            push_claude_source_with_imports(
                profile,
                scenario,
                project_root,
                &path,
                scope,
                PromptSourceKind::Rule,
                "startup",
                Vec::new(),
                None,
                startup,
                warnings,
            )?;
        } else {
            let matches = scenario
                .target_path
                .as_deref()
                .map(Path::new)
                .and_then(|target| {
                    project_root
                        .and_then(|root| target.strip_prefix(root).ok())
                        .or(Some(target))
                })
                .map(|target| glob_patterns_match(&patterns, target))
                .unwrap_or(false);
            push_claude_source_with_imports(
                profile,
                scenario,
                project_root,
                &path,
                scope,
                PromptSourceKind::Rule,
                "conditional",
                patterns,
                Some(matches),
                conditional,
                warnings,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_claude_source_with_imports(
    profile: &PromptProfile,
    scenario: &PromptScenario,
    project_root: Option<&Path>,
    path: &Path,
    scope: PromptSourceScope,
    kind: PromptSourceKind,
    phase: &str,
    path_patterns: Vec<String>,
    matches_target: Option<bool>,
    output: &mut Vec<PromptChainSource>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    push_chain_source(
        profile,
        scenario.project_id.as_deref(),
        project_root,
        path,
        scope,
        kind,
        phase,
        None,
        path_patterns.clone(),
        matches_target,
        false,
        output,
    )?;
    let mut visited = HashSet::new();
    push_claude_import_chain(
        profile,
        scenario,
        project_root,
        path,
        phase,
        &path_patterns,
        matches_target,
        0,
        &mut visited,
        output,
        warnings,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_claude_import_chain(
    profile: &PromptProfile,
    scenario: &PromptScenario,
    project_root: Option<&Path>,
    source_path: &Path,
    phase: &str,
    path_patterns: &[String],
    matches_target: Option<bool>,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    output: &mut Vec<PromptChainSource>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if depth >= MAX_CLAUDE_IMPORT_DEPTH || !visited.insert(normalized_path(source_path)) {
        return Ok(());
    }
    let Ok(content) = read_utf8_prompt(source_path) else {
        return Ok(());
    };
    for raw_import in parse_claude_imports(&content) {
        let import = resolve_claude_import_path(source_path, &raw_import)?;
        if !import.is_file() {
            warnings.push(format!("missing Claude import: {}", import.display()));
            continue;
        }
        push_chain_source(
            profile,
            scenario.project_id.as_deref(),
            project_root,
            &import,
            PromptSourceScope::Import,
            PromptSourceKind::Import,
            phase,
            Some(source_path.to_string_lossy().to_string()),
            path_patterns.to_vec(),
            matches_target,
            false,
            output,
        )?;
        push_claude_import_chain(
            profile,
            scenario,
            project_root,
            &import,
            phase,
            path_patterns,
            matches_target,
            depth + 1,
            visited,
            output,
            warnings,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_chain_source(
    profile: &PromptProfile,
    project_id: Option<&str>,
    project_root: Option<&Path>,
    path: &Path,
    scope: PromptSourceScope,
    kind: PromptSourceKind,
    phase: &str,
    imported_from: Option<String>,
    path_patterns: Vec<String>,
    matches_target: Option<bool>,
    truncated: bool,
    output: &mut Vec<PromptChainSource>,
) -> Result<()> {
    let source = build_prompt_source(
        profile,
        project_id,
        project_root,
        path,
        SourceSpec {
            scope,
            kind,
            can_create: false,
            can_delete: kind != PromptSourceKind::ManagedPolicy,
            warning: None,
        },
    )?;
    output.push(PromptChainSource {
        loaded_bytes: source.byte_len.unwrap_or(0),
        source,
        phase: phase.to_string(),
        order: output.len(),
        truncated,
        imported_from,
        path_patterns,
        matches_target,
    });
    Ok(())
}

fn build_prompt_source(
    profile: &PromptProfile,
    project_id: Option<&str>,
    project_root: Option<&Path>,
    path: &Path,
    spec: SourceSpec,
) -> Result<PromptSource> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let symlink_metadata = std::fs::symlink_metadata(&absolute).ok();
    let exists = symlink_metadata.is_some();
    let is_symlink = symlink_metadata
        .as_ref()
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    let link_target_path = if is_symlink {
        std::fs::read_link(&absolute).ok().map(|target| {
            if target.is_absolute() {
                target
            } else {
                absolute
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(target)
            }
        })
    } else {
        None
    };
    let real_path = if exists {
        std::fs::canonicalize(&absolute).ok()
    } else {
        None
    };
    let metadata = std::fs::metadata(&absolute).ok();
    let byte_len = metadata.as_ref().map(std::fs::Metadata::len);
    let modified_at = metadata
        .as_ref()
        .and_then(|value| value.modified().ok())
        .and_then(system_time_to_epoch);
    let (content_hash, empty, encoding, content_warning) = if exists {
        inspect_prompt_content(&absolute)
    } else {
        (None, true, "utf-8".to_string(), None)
    };
    let writable = prompt_path_writable(&absolute, exists);
    let display_path = project_root
        .and_then(|root| absolute.strip_prefix(root).ok())
        .map(|relative| relative.to_string_lossy().to_string())
        .unwrap_or_else(|| absolute.to_string_lossy().to_string());
    let warning = spec.warning.or(content_warning);
    Ok(PromptSource {
        id: prompt_source_id(profile.tool, &profile.id, project_id, &absolute),
        tool: profile.tool,
        profile_id: profile.id.clone(),
        project_id: project_id.map(str::to_string),
        scope: spec.scope,
        kind: spec.kind,
        path: absolute.to_string_lossy().to_string(),
        display_path,
        real_path: real_path.map(|value| value.to_string_lossy().to_string()),
        exists,
        is_symlink,
        link_target: link_target_path.map(|value| value.to_string_lossy().to_string()),
        writable,
        git_status: git_status_for_path(&absolute),
        content_hash,
        modified_at,
        byte_len,
        empty,
        encoding,
        can_create: spec.can_create,
        can_delete: spec.can_delete && !is_symlink,
        warning,
    })
}

fn insert_spec(
    specs: &mut BTreeMap<PathBuf, SourceSpec>,
    path: PathBuf,
    scope: PromptSourceScope,
    kind: PromptSourceKind,
    can_create: bool,
    can_delete: bool,
    warning: Option<String>,
) {
    specs.entry(path).or_insert(SourceSpec {
        scope,
        kind,
        can_create,
        can_delete,
        warning,
    });
}

fn should_visit_prompt_entry(entry: &DirEntry) -> bool {
    entry.depth() == 0 || !entry.file_type().is_dir() || !should_skip_prompt_scan_dir(entry.path())
}

fn parse_claude_imports(content: &str) -> Vec<String> {
    let import_pattern = Regex::new(r"(?:^|[\s(])@([^\s`]+)").expect("valid import regex");
    let mut imports = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let without_inline_code = strip_inline_code(line);
        for captures in import_pattern.captures_iter(&without_inline_code) {
            let Some(value) = captures.get(1) else {
                continue;
            };
            let import = value
                .as_str()
                .trim_end_matches([',', '.', ':', ';', ')', ']', '}'])
                .to_string();
            if !import.is_empty() {
                imports.push(import);
            }
        }
    }
    imports.sort();
    imports.dedup();
    imports
}

fn strip_inline_code(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut in_code = false;
    for character in line.chars() {
        if character == '`' {
            in_code = !in_code;
            output.push(' ');
        } else if in_code {
            output.push(' ');
        } else {
            output.push(character);
        }
    }
    output
}

fn resolve_claude_import_path(source: &Path, import: &str) -> Result<PathBuf> {
    if import == "~" {
        return dirs::home_dir().context("failed to resolve home directory");
    }
    if let Some(relative) = import.strip_prefix("~/") {
        return Ok(dirs::home_dir()
            .context("failed to resolve home directory")?
            .join(relative));
    }
    let path = PathBuf::from(import);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(source.parent().unwrap_or_else(|| Path::new(".")).join(path))
    }
}

fn parse_claude_rule_paths(content: &str) -> Vec<String> {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Vec::new();
    }
    let mut paths = Vec::new();
    let mut in_paths = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if trimmed == "paths:" {
            in_paths = true;
            continue;
        }
        if in_paths {
            if let Some(value) = trimmed.strip_prefix('-') {
                let value = value.trim().trim_matches(['\'', '"']);
                if !value.is_empty() {
                    paths.push(value.to_string());
                }
            } else if !trimmed.is_empty() && !line.starts_with([' ', '\t']) {
                in_paths = false;
            }
        }
    }
    paths
}

fn glob_patterns_match(patterns: &[String], target: &Path) -> bool {
    let Some(set) = compile_glob_set(patterns) else {
        return false;
    };
    set.is_match(target.to_string_lossy().replace('\\', "/"))
}

fn compile_glob_set(patterns: &[String]) -> Option<globset::GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut added = false;
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
            added = true;
        }
    }
    added.then(|| builder.build().ok()).flatten()
}

fn claude_path_excluded(
    path: &Path,
    project_root: &Path,
    excludes: Option<&globset::GlobSet>,
) -> bool {
    let Some(excludes) = excludes else {
        return false;
    };
    let candidate = path
        .strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    excludes.is_match(candidate)
}

fn claude_files_at_directory(directory: &Path, project_root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![directory.join("CLAUDE.md")];
    if directory == project_root {
        paths.push(directory.join(".claude/CLAUDE.md"));
    }
    paths.push(directory.join("CLAUDE.local.md"));
    paths
}

fn detect_codex_project_root(
    working_directory: &Path,
    registered_root: &Path,
    markers: &[String],
) -> PathBuf {
    let mut current = working_directory.to_path_buf();
    loop {
        if markers.iter().any(|marker| current.join(marker).exists()) {
            return current;
        }
        if current == registered_root || !current.pop() || !current.starts_with(registered_root) {
            break;
        }
    }
    registered_root.to_path_buf()
}

fn path_chain(root: &Path, leaf: &Path) -> Result<Vec<PathBuf>> {
    ensure_path_in_project(leaf, root)?;
    let relative = leaf
        .strip_prefix(root)
        .with_context(|| format!("path {:?} is outside {:?}", leaf, root))?;
    let mut paths = vec![root.to_path_buf()];
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        paths.push(current.clone());
    }
    Ok(paths)
}

fn path_chain_from_filesystem_root(leaf: &Path) -> Vec<PathBuf> {
    let mut paths = leaf.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
    paths.reverse();
    paths
}

fn ensure_path_in_project(path: &Path, project_root: &Path) -> Result<()> {
    let normalized_root = normalized_path(project_root);
    let normalized_path = normalized_path(path);
    if !normalized_path.starts_with(&normalized_root) {
        anyhow::bail!(
            "PROMPT_PATH_OUTSIDE_PROJECT|{}|{}",
            path.display(),
            project_root.display()
        );
    }
    Ok(())
}

fn prompt_file_has_content(path: &Path) -> bool {
    read_utf8_prompt(path)
        .map(|content| !content.trim().is_empty())
        .unwrap_or(false)
}

fn read_utf8_prompt(path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_EDITABLE_PROMPT_BYTES {
        anyhow::bail!("prompt file is too large: {:?}", path);
    }
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).context("prompt file is not UTF-8")
}

fn inspect_prompt_content(path: &Path) -> (Option<String>, bool, String, Option<String>) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (None, true, "unknown".to_string(), None);
    };
    if metadata.len() > MAX_EDITABLE_PROMPT_BYTES {
        return (
            hash_file(path).ok(),
            false,
            "utf-8".to_string(),
            Some(format!(
                "prompt exceeds the {} byte editor limit",
                MAX_EDITABLE_PROMPT_BYTES
            )),
        );
    }
    let Ok(bytes) = std::fs::read(path) else {
        return (None, false, "unknown".to_string(), None);
    };
    let content = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    match std::str::from_utf8(content) {
        Ok(text) => (
            Some(hash_bytes(&bytes)),
            text.trim().is_empty(),
            "utf-8".to_string(),
            None,
        ),
        Err(_) => (
            Some(hash_bytes(&bytes)),
            false,
            "unsupported".to_string(),
            Some("prompt is not valid UTF-8".to_string()),
        ),
    }
}

fn prompt_path_writable(path: &Path, exists: bool) -> bool {
    if exists {
        return OpenOptions::new().write(true).open(path).is_ok();
    }
    nearest_existing_parent(path)
        .and_then(|parent| std::fs::metadata(parent).ok())
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

fn nearest_existing_parent(path: &Path) -> Option<&Path> {
    let mut current = path.parent();
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn git_status_for_path(path: &Path) -> Option<String> {
    let repository = Repository::discover(path.parent().unwrap_or(path)).ok()?;
    let workdir = repository.workdir()?;
    let relative = path.strip_prefix(workdir).ok()?;
    let status = repository.status_file(relative).ok()?;
    if status.contains(Status::CONFLICTED) {
        return Some("conflicted".to_string());
    }
    if status.contains(Status::IGNORED) {
        return Some("ignored".to_string());
    }
    if status.intersects(Status::WT_NEW | Status::INDEX_NEW) {
        return Some("untracked".to_string());
    }
    if status.intersects(
        Status::WT_MODIFIED
            | Status::WT_DELETED
            | Status::WT_RENAMED
            | Status::INDEX_MODIFIED
            | Status::INDEX_DELETED
            | Status::INDEX_RENAMED,
    ) {
        return Some("modified".to_string());
    }
    let tracked = repository
        .index()
        .ok()
        .and_then(|index| index.get_path(relative, 0))
        .is_some();
    tracked.then(|| "tracked".to_string())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn prompt_source_id(
    tool: PromptTool,
    profile_id: &str,
    project_id: Option<&str>,
    path: &Path,
) -> String {
    let raw = format!(
        "{}|{}|{}|{}",
        tool.as_key(),
        profile_id,
        project_id.unwrap_or_default(),
        path.to_string_lossy()
    );
    format!("prompt:{}", &hash_bytes(raw.as_bytes())[..24])
}

fn system_time_to_epoch(value: SystemTime) -> Option<i64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

fn normalized_path(path: &Path) -> PathBuf {
    if let Ok(path) = std::fs::canonicalize(path) {
        return path;
    }
    let mut ancestor = path.parent();
    while let Some(existing) = ancestor {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            if let Ok(relative) = path.strip_prefix(existing) {
                return canonical.join(relative);
            }
        }
        ancestor = existing.parent();
    }
    path.to_path_buf()
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    normalized_path(path).starts_with(normalized_path(root))
}

fn codex_kind_for_path(path: &Path, fallbacks: &[String]) -> PromptSourceKind {
    match path.file_name().and_then(|value| value.to_str()) {
        Some("AGENTS.override.md") => PromptSourceKind::Override,
        Some("AGENTS.md") => PromptSourceKind::Primary,
        Some(name) if fallbacks.iter().any(|value| value == name) => PromptSourceKind::Fallback,
        _ => PromptSourceKind::Fallback,
    }
}

fn source_scope_order(scope: PromptSourceScope) -> u8 {
    match scope {
        PromptSourceScope::Managed => 0,
        PromptSourceScope::Global => 1,
        PromptSourceScope::Project => 2,
        PromptSourceScope::Local => 3,
        PromptSourceScope::Import => 4,
    }
}

fn deduplicate_chain(chain: &mut Vec<PromptChainSource>) {
    let mut seen = HashSet::new();
    chain.retain(|source| seen.insert((source.source.path.clone(), source.phase.clone())));
}

fn renumber_chain(chain: &mut [PromptChainSource]) {
    for (index, source) in chain.iter_mut().enumerate() {
        source.order = index;
    }
}

fn find_authorized_prompt_source(
    store: &SkillStore,
    request: &PromptFileRequest,
) -> Result<PromptSource> {
    let requested = PathBuf::from(&request.path);
    if !requested.is_absolute() {
        anyhow::bail!("prompt path must be absolute: {}", request.path);
    }
    let source = list_prompt_sources(store, &request.profile_id, request.project_id.as_deref())?
        .into_iter()
        .find(|source| Path::new(&source.path) == requested)
        .with_context(|| format!("PROMPT_SOURCE_NOT_DISCOVERED|{}", request.path))?;
    require_external_prompt_confirmation(store, &source, request.allow_external)?;
    Ok(source)
}

fn require_external_prompt_confirmation(
    store: &SkillStore,
    source: &PromptSource,
    allow_external: bool,
) -> Result<()> {
    let profile = resolve_prompt_profile(store, &source.profile_id)?;
    let target = source
        .real_path
        .as_deref()
        .or(source.link_target.as_deref())
        .unwrap_or(&source.path);
    let target = Path::new(target);
    let in_profile = path_is_within(target, Path::new(&profile.config_root));
    let in_project = source
        .project_id
        .as_deref()
        .map(|project_id| {
            store
                .get_project(project_id)
                .ok()
                .flatten()
                .map(|project| path_is_within(target, Path::new(&project.root_path)))
                .unwrap_or(false)
        })
        .unwrap_or(false);
    if !in_profile
        && !in_project
        && source.kind != PromptSourceKind::ManagedPolicy
        && !allow_external
    {
        anyhow::bail!("PROMPT_EXTERNAL_CONFIRMATION_REQUIRED|{}", target.display());
    }
    Ok(())
}

fn check_prompt_conflict(
    source: &PromptSource,
    expected_hash: Option<&str>,
    force: bool,
) -> Result<()> {
    if force {
        return Ok(());
    }
    match (expected_hash, source.content_hash.as_deref(), source.exists) {
        (Some(expected), Some(current), true) if expected == current => Ok(()),
        (None, None, false) => Ok(()),
        (_, current, _) => anyhow::bail!(
            "PROMPT_CONFLICT|{}|{}",
            source.path,
            current.unwrap_or_default()
        ),
    }
}

fn snapshot_prompt(store: &SkillStore, source: &PromptSource, operation: &str) -> Result<String> {
    let content = if source.exists {
        std::fs::read(&source.path)
            .with_context(|| format!("snapshot prompt file {:?}", source.path))?
    } else {
        Vec::new()
    };
    let id = Uuid::new_v4().to_string();
    let revision = PromptRevisionRecord {
        id: id.clone(),
        logical_path: source.path.clone(),
        real_path: source
            .real_path
            .clone()
            .or_else(|| source.link_target.clone()),
        tool: source.tool.as_key().to_string(),
        profile_id: source.profile_id.clone(),
        project_id: source.project_id.clone(),
        operation: operation.to_string(),
        content_hash: hash_bytes(&content),
        content,
        created_at: now_ms(),
    };
    store.insert_prompt_revision(&revision)?;
    store.prune_prompt_revisions(&source.path, PROMPT_REVISION_RETENTION)?;
    Ok(id)
}

fn encode_prompt_content(content: &str, line_ending: &str, has_bom: bool) -> Vec<u8> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let rendered = if line_ending == "crlf" {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    };
    let mut bytes = Vec::with_capacity(rendered.len() + usize::from(has_bom) * 3);
    if has_bom {
        bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    bytes.extend_from_slice(rendered.as_bytes());
    bytes
}

fn write_prompt_bytes(source: &PromptSource, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > MAX_EDITABLE_PROMPT_BYTES {
        anyhow::bail!(
            "PROMPT_TOO_LARGE|{}|{}",
            bytes.len(),
            MAX_EDITABLE_PROMPT_BYTES
        );
    }
    let logical_path = PathBuf::from(&source.path);
    let target = if source.is_symlink {
        source
            .real_path
            .as_deref()
            .or(source.link_target.as_deref())
            .map(PathBuf::from)
            .context("prompt symlink target is unavailable")?
    } else {
        logical_path
    };
    let parent = target
        .parent()
        .with_context(|| format!("prompt path has no parent: {:?}", target))?;
    if !parent.exists() {
        if !source.can_create {
            anyhow::bail!("PROMPT_PARENT_MISSING|{}", parent.display());
        }
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create prompt parent directory {:?}", parent))?;
    }
    let permissions = std::fs::metadata(&target)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("create temporary prompt file in {:?}", parent))?;
    temporary
        .write_all(bytes)
        .with_context(|| format!("write temporary prompt file for {:?}", target))?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    atomic_replace_prompt_file(temporary, &target)?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn atomic_replace_prompt_file(temporary: tempfile::NamedTempFile, target: &Path) -> Result<()> {
    temporary
        .persist(target)
        .map_err(|error| error.error)
        .with_context(|| format!("atomically replace prompt file {:?}", target))?;
    if let Some(parent) = target.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn atomic_replace_prompt_file(temporary: tempfile::NamedTempFile, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    if !target.exists() {
        temporary
            .persist(target)
            .map_err(|error| error.error)
            .with_context(|| format!("persist prompt file {:?}", target))?;
        return Ok(());
    }
    let (_file, temporary_path) = temporary.keep()?;
    let target_wide = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let temporary_wide = temporary_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
            target_wide.as_ptr(),
            temporary_wide.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if replaced == 0 {
        let error = std::io::Error::last_os_error();
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error).with_context(|| format!("replace prompt file {:?}", target));
    }
    Ok(())
}

fn refresh_prompt_source(store: &SkillStore, previous: &PromptSource) -> Result<PromptSource> {
    if let Some(source) =
        list_prompt_sources(store, &previous.profile_id, previous.project_id.as_deref())?
            .into_iter()
            .find(|source| source.path == previous.path)
    {
        return Ok(source);
    }
    let profile = resolve_prompt_profile(store, &previous.profile_id)?;
    let project_root = previous
        .project_id
        .as_deref()
        .map(|id| store.get_project(id))
        .transpose()?
        .flatten()
        .map(|project| PathBuf::from(project.root_path));
    build_prompt_source(
        &profile,
        previous.project_id.as_deref(),
        project_root.as_deref(),
        Path::new(&previous.path),
        SourceSpec {
            scope: previous.scope,
            kind: previous.kind,
            can_create: previous.can_create,
            can_delete: previous.can_delete,
            warning: previous.warning.clone(),
        },
    )
}

fn source_for_revision_restore(
    store: &SkillStore,
    revision: &PromptRevisionRecord,
    allow_external: bool,
) -> Result<PromptSource> {
    let request = PromptFileRequest {
        profile_id: revision.profile_id.clone(),
        project_id: revision.project_id.clone(),
        path: revision.logical_path.clone(),
        allow_external,
    };
    if let Ok(source) = find_authorized_prompt_source(store, &request) {
        return Ok(source);
    }
    let profile = resolve_prompt_profile(store, &revision.profile_id)?;
    if profile.tool.as_key() != revision.tool {
        anyhow::bail!("prompt revision tool does not match its profile");
    }
    let project = revision
        .project_id
        .as_deref()
        .map(|id| {
            store
                .get_project(id)?
                .with_context(|| format!("project not found: {id}"))
        })
        .transpose()?;
    let path = PathBuf::from(&revision.logical_path);
    let in_profile = path_is_within(&path, Path::new(&profile.config_root));
    let in_project = project
        .as_ref()
        .map(|project| path_is_within(&path, Path::new(&project.root_path)))
        .unwrap_or(false);
    if !in_profile && !in_project && !allow_external {
        anyhow::bail!("PROMPT_EXTERNAL_CONFIRMATION_REQUIRED|{}", path.display());
    }
    let (scope, kind) = restored_source_classification(&path, in_profile);
    build_prompt_source(
        &profile,
        revision.project_id.as_deref(),
        project
            .as_ref()
            .map(|project| Path::new(&project.root_path)),
        &path,
        SourceSpec {
            scope,
            kind,
            can_create: true,
            can_delete: true,
            warning: (!in_profile && !in_project).then(|| "external prompt source".to_string()),
        },
    )
}

fn restored_source_classification(
    path: &Path,
    in_profile: bool,
) -> (PromptSourceScope, PromptSourceKind) {
    let name = path.file_name().and_then(|value| value.to_str());
    let scope = if in_profile {
        PromptSourceScope::Global
    } else if name == Some("CLAUDE.local.md") {
        PromptSourceScope::Local
    } else {
        PromptSourceScope::Project
    };
    let kind = match name {
        Some("AGENTS.override.md") => PromptSourceKind::Override,
        Some("AGENTS.md" | "CLAUDE.md" | "SYSTEM.md") => PromptSourceKind::Primary,
        Some("APPEND_SYSTEM.md") => PromptSourceKind::Fallback,
        Some("CLAUDE.local.md") => PromptSourceKind::Local,
        Some(_) if path.to_string_lossy().contains(".claude/rules") => PromptSourceKind::Rule,
        _ => PromptSourceKind::Import,
    };
    (scope, kind)
}

fn prompt_revision_summary(revision: &PromptRevisionRecord) -> PromptRevisionSummary {
    PromptRevisionSummary {
        id: revision.id.clone(),
        logical_path: revision.logical_path.clone(),
        real_path: revision.real_path.clone(),
        tool: revision.tool.clone(),
        profile_id: revision.profile_id.clone(),
        project_id: revision.project_id.clone(),
        operation: revision.operation.clone(),
        content_hash: revision.content_hash.clone(),
        byte_len: revision.content.len(),
        created_at: revision.created_at,
    }
}

fn prompt_revision_document(revision: &PromptRevisionRecord) -> PromptRevisionDocument {
    let has_bom = revision.content.starts_with(&[0xef, 0xbb, 0xbf]);
    let bytes = if has_bom {
        &revision.content[3..]
    } else {
        &revision.content
    };
    let (content, encoding) = match String::from_utf8(bytes.to_vec()) {
        Ok(content) => (content, "utf-8".to_string()),
        Err(_) => (
            String::from_utf8_lossy(bytes).to_string(),
            "unsupported".to_string(),
        ),
    };
    let line_ending = if content.contains("\r\n") {
        "crlf"
    } else {
        "lf"
    };
    PromptRevisionDocument {
        summary: prompt_revision_summary(revision),
        content,
        encoding,
        line_ending: line_ending.to_string(),
        has_bom,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
#[path = "tests/prompts.rs"]
mod tests;
