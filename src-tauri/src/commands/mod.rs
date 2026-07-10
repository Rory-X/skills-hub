use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::State;

use std::sync::Arc;

use crate::core::assignments::{
    apply_skill_assignments as apply_skill_assignments_core,
    ignore_all_assignment_issues as ignore_all_assignment_issues_core,
    ignore_assignment_issue as ignore_assignment_issue_core, managed_skill_projections,
    preview_skill_assignments as preview_skill_assignments_core,
    resume_skill_assignment as resume_skill_assignment_core, AssignmentConflictKey,
    AssignmentDraft, AssignmentProjection, AssignmentToolSpec, ProjectAssignmentDraft,
};
use crate::core::auto_update::{
    get_auto_update_config as get_auto_update_config_core, record_auto_update_triggered,
    run_auto_update_now as run_auto_update_now_core,
    set_auto_update_config as set_auto_update_config_core, AutoUpdateConfig,
    AutoUpdateIntervalUnit, AutoUpdateProgressSnapshot, AutoUpdateRunResult, AutoUpdateSchedule,
    AutoUpdateScheduleType,
};
use crate::core::cache_cleanup::{
    cleanup_git_cache_dirs, get_git_cache_cleanup_days as get_git_cache_cleanup_days_core,
    get_git_cache_ttl_secs as get_git_cache_ttl_secs_core,
    set_git_cache_cleanup_days as set_git_cache_cleanup_days_core,
    set_git_cache_ttl_secs as set_git_cache_ttl_secs_core,
};
use crate::core::cancel_token::CancelToken;
use crate::core::central_repo::{ensure_central_repo, resolve_central_repo_path};
use crate::core::featured_skills::{fetch_featured_skills, FeaturedSkill};
use crate::core::github_search::{search_github_repos, RepoSummary};
use crate::core::installer::{
    import_discovered_skill, install_git_skill, install_git_skill_from_selection,
    install_local_skill, install_local_skill_from_selection, list_git_skills, list_local_skills,
    reimport_git_skill_from_selection, reimport_local_skill_from_selection,
    update_managed_skill_from_source, GitSkillCandidate, InstallResult, LocalSkillCandidate,
};
use crate::core::network_proxy::{
    get_github_proxy_config as get_github_proxy_config_core,
    get_github_proxy_url as get_github_proxy_url_core,
    set_github_proxy_config as set_github_proxy_config_core,
    set_github_proxy_url as set_github_proxy_url_core, GithubProxyConfig,
};
use crate::core::onboarding::{build_onboarding_plan, OnboardingPlan};
use crate::core::projects::{
    build_project_detail_data, build_project_summary_data, collect_project_skill_and_scan,
    ignore_all_project_issues_and_scan, ignore_project_issue_and_scan, import_project_and_scan,
    inject_project_skill_and_scan, refresh_project, remove_project_skill_and_scan,
    repair_project_issue_and_scan, update_project_and_scan, visible_project_issue_projections,
    ProjectIssue, ProjectIssueProjection, ProjectToolSpec,
};
use crate::core::skill_store::{ProjectRecord, SkillStore};
use crate::core::skills_search::{
    search_skills_online as search_skills_online_core, OnlineSkillResult,
};
use crate::core::sync_engine::{copy_dir_recursive, sync_dir_hybrid, SyncMode};
use crate::core::system_scheduler::{
    current_scheduler_config, get_auto_update_task_status, install_auto_update_task,
    trigger_auto_update_task_now, uninstall_auto_update_task,
};
use crate::core::tool_adapters::{
    is_builtin_tool_enabled, is_tool_installed, load_tool_config, project_relative_skills_dir,
    resolve_default_path, save_tool_config, supports_project_scope, CustomToolConfig, ToolConfig,
};

const RECENT_PROJECTS_SETTING: &str = "recent_projects_v1";

fn format_anyhow_error(err: anyhow::Error) -> String {
    let first = err.to_string();
    // Frontend relies on these prefixes for special flows.
    if first.starts_with("MULTI_SKILLS|")
        || first.starts_with("TARGET_EXISTS|")
        || first.starts_with("TOOL_NOT_INSTALLED|")
        || first.starts_with("PROJECT_LIBRARY_NAME_REQUIRED|")
        || first.starts_with("HUB_NAME_CONFLICT|")
    {
        return first;
    }

    // Include the full error chain (causes), not just the top context.
    let mut full = format!("{:#}", err);

    // Redact noisy temp paths from clone context (we care about the cause, not the dest).
    // Example: `clone https://... into "/Users/.../skills-hub-git-<uuid>"`
    if let Some(head) = full.lines().next() {
        if head.starts_with("clone ") {
            if let Some(pos) = head.find(" into ") {
                let head_redacted = format!("{} (已省略临时目录)", &head[..pos]);
                let rest: String = full.lines().skip(1).collect::<Vec<_>>().join("\n");
                full = if rest.is_empty() {
                    head_redacted
                } else {
                    format!("{}\n{}", head_redacted, rest)
                };
            }
        }
    }

    let root = err.root_cause().to_string();
    let lower = full.to_lowercase();

    // Heuristic-friendly messaging for GitHub clone failures.
    if lower.contains("github.com")
        && (lower.contains("clone ") || lower.contains("remote") || lower.contains("fetch"))
    {
        if lower.contains("securetransport") {
            return format!(
        "无法从 GitHub 拉取仓库：TLS/证书校验失败（macOS SecureTransport）。\n\n建议：\n- 检查网络/代理是否拦截 HTTPS\n- 如在公司网络，可能需要安装公司根证书或使用可信代理\n- 也可在终端确认 `git clone {}` 是否可用\n\n详细：{}",
        "https://github.com/<owner>/<repo>",
        root
      );
        }
        let hint = if lower.contains("authentication")
            || lower.contains("permission denied")
            || lower.contains("credentials")
        {
            "无法访问该仓库：可能是私有仓库/权限不足/需要鉴权。"
        } else if lower.contains("not found") {
            "仓库不存在或无权限访问（GitHub 返回 not found）。"
        } else if lower.contains("failed to resolve")
            || lower.contains("could not resolve")
            || lower.contains("dns")
        {
            "无法解析 GitHub 域名（DNS）。请检查网络/代理。"
        } else if lower.contains("timed out") || lower.contains("timeout") {
            "连接 GitHub 超时。请检查网络/代理。"
        } else if lower.contains("connection refused") || lower.contains("connection reset") {
            "连接 GitHub 失败（连接被拒绝/重置）。请检查网络/代理。"
        } else {
            "无法从 GitHub 拉取仓库。请检查网络/代理，或稍后重试。"
        };

        return format!("{}\n\n详细：{}", hint, root);
    }

    full
}

#[derive(Debug, Serialize)]
pub struct ToolInfoDto {
    pub key: String,
    pub label: String,
    pub installed: bool,
    pub enabled: bool,
    pub is_custom: bool,
    pub skills_dir: String,
    pub project_skills_dir: String,
    pub supports_project_scope: bool,
}

#[derive(Debug, Serialize)]
pub struct ToolStatusDto {
    pub tools: Vec<ToolInfoDto>,
    pub installed: Vec<String>,
    pub newly_installed: Vec<String>,
}

#[derive(Clone, Debug)]
struct RuntimeTool {
    key: String,
    label: String,
    installed: bool,
    enabled: bool,
    is_custom: bool,
    skills_dir: std::path::PathBuf,
    project_skills_dir: String,
    supports_project_scope: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolConfigDto {
    pub disabled_builtin_tools: Vec<String>,
    pub custom_tools: Vec<CustomToolConfigDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomToolConfigDto {
    pub key: String,
    pub label: String,
    pub skills_dir: String,
    pub project_skills_dir: Option<String>,
    pub enabled: bool,
}

impl From<ToolConfig> for ToolConfigDto {
    fn from(config: ToolConfig) -> Self {
        Self {
            disabled_builtin_tools: config.disabled_builtin_tools,
            custom_tools: config
                .custom_tools
                .into_iter()
                .map(|tool| CustomToolConfigDto {
                    key: tool.key,
                    label: tool.label,
                    skills_dir: tool.skills_dir,
                    project_skills_dir: tool.project_skills_dir,
                    enabled: tool.enabled,
                })
                .collect(),
        }
    }
}

impl From<ToolConfigDto> for ToolConfig {
    fn from(config: ToolConfigDto) -> Self {
        Self {
            disabled_builtin_tools: config.disabled_builtin_tools,
            custom_tools: config
                .custom_tools
                .into_iter()
                .map(|tool| CustomToolConfig {
                    key: tool.key,
                    label: tool.label,
                    skills_dir: tool.skills_dir,
                    project_skills_dir: tool.project_skills_dir,
                    enabled: tool.enabled,
                })
                .collect(),
        }
    }
}

fn runtime_tools(store: &SkillStore, include_disabled: bool) -> anyhow::Result<Vec<RuntimeTool>> {
    let config = load_tool_config(store)?;
    let mut tools = Vec::new();

    for adapter in crate::core::tool_adapters::default_tool_adapters() {
        let enabled = is_builtin_tool_enabled(&config, adapter.id.as_key());
        if !include_disabled && !enabled {
            continue;
        }
        let detected = is_tool_installed(&adapter)?;
        tools.push(RuntimeTool {
            key: adapter.id.as_key().to_string(),
            label: adapter.display_name.to_string(),
            installed: enabled && detected,
            enabled,
            is_custom: false,
            skills_dir: resolve_default_path(&adapter)?,
            project_skills_dir: project_relative_skills_dir(&adapter).to_string(),
            supports_project_scope: supports_project_scope(&adapter),
        });
    }

    for custom in config.custom_tools {
        if !include_disabled && !custom.enabled {
            continue;
        }
        let skills_dir = expand_home_path(&custom.skills_dir)?;
        let supports_project_scope = custom.project_skills_dir.is_some();
        let detected = skills_dir.is_dir();
        tools.push(RuntimeTool {
            key: custom.key,
            label: custom.label,
            installed: custom.enabled && detected,
            enabled: custom.enabled,
            is_custom: true,
            skills_dir,
            project_skills_dir: custom.project_skills_dir.unwrap_or_default(),
            supports_project_scope,
        });
    }

    Ok(tools)
}

#[tauri::command]
pub async fn get_tool_config(store: State<'_, SkillStore>) -> Result<ToolConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || load_tool_config(&store).map(ToolConfigDto::from))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_tool_config(
    store: State<'_, SkillStore>,
    config: ToolConfigDto,
) -> Result<ToolConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_tool_config(&store, config.into()).map(ToolConfigDto::from)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_tool_status(store: State<'_, SkillStore>) -> Result<ToolStatusDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut tools: Vec<ToolInfoDto> = Vec::new();
        let mut installed: Vec<String> = Vec::new();

        for tool in runtime_tools(&store, true)? {
            tools.push(ToolInfoDto {
                key: tool.key.clone(),
                label: tool.label,
                installed: tool.installed,
                enabled: tool.enabled,
                is_custom: tool.is_custom,
                skills_dir: tool.skills_dir.to_string_lossy().to_string(),
                project_skills_dir: tool.project_skills_dir,
                supports_project_scope: tool.supports_project_scope,
            });
            if tool.installed {
                installed.push(tool.key);
            }
        }

        installed.dedup();

        let prev: Vec<String> = store
            .get_setting("installed_tools_v1")?
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();

        let prev_set: std::collections::HashSet<String> = prev.into_iter().collect();
        let newly_installed: Vec<String> = installed
            .iter()
            .filter(|k| !prev_set.contains(*k))
            .cloned()
            .collect();

        // Persist current set (best effort).
        let _ = store.set_setting(
            "installed_tools_v1",
            &serde_json::to_string(&installed).unwrap_or_else(|_| "[]".to_string()),
        );

        Ok::<_, anyhow::Error>(ToolStatusDto {
            tools,
            installed,
            newly_installed,
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_onboarding_plan(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
) -> Result<OnboardingPlan, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || build_onboarding_plan(&app, &store))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_git_cache_cleanup_days(store: State<'_, SkillStore>) -> Result<i64, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, anyhow::Error>(get_git_cache_cleanup_days_core(&store))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_git_cache_cleanup_days(
    store: State<'_, SkillStore>,
    days: i64,
) -> Result<i64, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_git_cache_cleanup_days_core(&store, days))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn clear_git_cache_now(app: tauri::AppHandle) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        cleanup_git_cache_dirs(&app, std::time::Duration::from_secs(0))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_git_cache_ttl_secs(store: State<'_, SkillStore>) -> Result<i64, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, anyhow::Error>(get_git_cache_ttl_secs_core(&store))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_git_cache_ttl_secs(
    store: State<'_, SkillStore>,
    secs: i64,
) -> Result<i64, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_git_cache_ttl_secs_core(&store, secs))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct AutoUpdateConfigDto {
    pub enabled: bool,
    pub interval_hours: i64,
    pub schedule_type: String,
    pub interval_value: i64,
    pub interval_unit: String,
    pub daily_time: String,
    pub local_skill_count: usize,
    pub protected_local_skill_count: usize,
    pub task_registered: bool,
    pub task_status_detail: String,
    pub last_run_at: Option<i64>,
    pub last_started_at: Option<i64>,
    pub last_finished_at: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_checked: usize,
    pub last_updated: usize,
    pub last_failed: usize,
    pub progress: AutoUpdateProgressSnapshot,
}

#[derive(Debug, Serialize)]
pub struct AutoUpdateRunResultDto {
    pub checked: usize,
    pub updated: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub progress: AutoUpdateProgressSnapshot,
}

#[derive(Debug, Serialize)]
pub struct GithubProxyConfigDto {
    pub enabled: bool,
    pub port: u16,
    pub url: String,
    pub auto_detected: bool,
}

#[tauri::command]
pub async fn get_auto_update_config(
    store: State<'_, SkillStore>,
) -> Result<AutoUpdateConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        get_auto_update_config_core(&store).map(to_auto_update_config_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_auto_update_config(
    store: State<'_, SkillStore>,
    enabled: bool,
    intervalHours: i64,
    scheduleType: Option<String>,
    intervalValue: Option<i64>,
    intervalUnit: Option<String>,
    dailyTime: Option<String>,
) -> Result<AutoUpdateConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let schedule = build_auto_update_schedule(
            intervalHours,
            scheduleType.as_deref(),
            intervalValue,
            intervalUnit.as_deref(),
            dailyTime.as_deref(),
        )?;
        if enabled {
            let scheduler_config = current_scheduler_config(schedule.clone())?;
            install_auto_update_task(&scheduler_config)?;
        } else {
            uninstall_auto_update_task()?;
        }

        let existing = get_auto_update_config_core(&store)?;
        let saved = set_auto_update_config_core(
            &store,
            AutoUpdateConfig {
                enabled,
                interval_hours: intervalHours,
                schedule,
                local_skill_count: existing.local_skill_count,
                protected_local_skill_count: existing.protected_local_skill_count,
                last_run_at: existing.last_run_at,
                last_started_at: existing.last_started_at,
                last_finished_at: existing.last_finished_at,
                last_status: existing.last_status,
                last_error: existing.last_error,
                last_checked: existing.last_checked,
                last_updated: existing.last_updated,
                last_failed: existing.last_failed,
                progress: existing.progress,
            },
        )?;
        Ok::<_, anyhow::Error>(to_auto_update_config_dto(saved))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

fn build_auto_update_schedule(
    legacy_interval_hours: i64,
    schedule_type: Option<&str>,
    interval_value: Option<i64>,
    interval_unit: Option<&str>,
    daily_time: Option<&str>,
) -> anyhow::Result<AutoUpdateSchedule> {
    let schedule_type = match schedule_type.unwrap_or("interval") {
        "daily" => AutoUpdateScheduleType::Daily,
        "interval" => AutoUpdateScheduleType::Interval,
        other => anyhow::bail!("unsupported auto update schedule type: {other}"),
    };
    let interval_unit = match interval_unit.unwrap_or("hours") {
        "minutes" => AutoUpdateIntervalUnit::Minutes,
        "hours" => AutoUpdateIntervalUnit::Hours,
        other => anyhow::bail!("unsupported auto update interval unit: {other}"),
    };
    let schedule = AutoUpdateSchedule {
        schedule_type,
        interval_value: interval_value.unwrap_or(legacy_interval_hours),
        interval_unit,
        daily_time: daily_time.unwrap_or("03:00").to_string(),
    };
    match schedule.schedule_type {
        AutoUpdateScheduleType::Interval => {
            let minutes = schedule.interval_minutes();
            if !(15..=24 * 30 * 60).contains(&minutes) {
                anyhow::bail!("interval minutes must be between 15 and 43200");
            }
        }
        AutoUpdateScheduleType::Daily => {
            let Some((hour, minute)) = schedule.daily_time.split_once(':') else {
                anyhow::bail!("daily time must use HH:mm format");
            };
            if hour.len() != 2 || minute.len() != 2 {
                anyhow::bail!("daily time must use HH:mm format");
            }
            let hour = hour.parse::<u8>().context("parse daily schedule hour")?;
            let minute = minute
                .parse::<u8>()
                .context("parse daily schedule minute")?;
            if hour > 23 || minute > 59 {
                anyhow::bail!("daily time must use HH:mm format");
            }
        }
    }
    Ok(schedule)
}

#[tauri::command]
pub async fn run_auto_update_now(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
) -> Result<AutoUpdateRunResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_auto_update_now_core(&app, &store).map(to_auto_update_run_result_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn trigger_auto_update_task_now_cmd(store: State<'_, SkillStore>) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config = get_auto_update_config_core(&store)?;
        let scheduler_config = current_scheduler_config(config.schedule)?;
        install_auto_update_task(&scheduler_config)?;
        record_auto_update_triggered(&store)?;
        trigger_auto_update_task_now()
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct InstallResultDto {
    pub skill_id: String,
    pub name: String,
    pub central_path: String,
    pub content_hash: Option<String>,
}

fn expand_home_path(input: &str) -> Result<std::path::PathBuf, anyhow::Error> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("storage path is empty");
    }
    if trimmed == "~" {
        let home = dirs::home_dir().context("failed to resolve home directory")?;
        return Ok(home);
    }
    if let Some(stripped) = trimmed.strip_prefix("~/") {
        let home = dirs::home_dir().context("failed to resolve home directory")?;
        return Ok(home.join(stripped));
    }
    Ok(std::path::PathBuf::from(trimmed))
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectSummaryDto {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub enabled_tool_keys: Vec<String>,
    pub local_git_exclude_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_scanned_at: Option<i64>,
    pub skill_count: usize,
    pub attention_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectToolDto {
    pub key: String,
    pub label: String,
    pub project_skills_dir: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectSkillPathDto {
    pub discovered_skill_id: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub tool_keys: Vec<String>,
    pub content_hash: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectSkillDto {
    pub name: String,
    pub description: Option<String>,
    pub source: String,
    pub status: String,
    pub linked_skill_id: Option<String>,
    pub matched_skill_id: Option<String>,
    pub tool_keys: Vec<String>,
    pub paths: Vec<ProjectSkillPathDto>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectLibrarySkillDto {
    pub skill_id: String,
    pub name: String,
    pub description: Option<String>,
    pub availability: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectIssueActionDto {
    pub kind: String,
    pub value: Option<String>,
    pub label: Option<String>,
    pub confirmation_required: bool,
    pub destructive: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectIssueDto {
    pub id: String,
    pub kind: String,
    pub skill_id: Option<String>,
    pub skill_name: Option<String>,
    pub tool_key: Option<String>,
    pub path: Option<String>,
    pub actions: Vec<ProjectIssueActionDto>,
    pub fingerprint: String,
    pub can_ignore: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectDetailDto {
    pub project: ProjectSummaryDto,
    pub tools: Vec<ProjectToolDto>,
    pub current_skills: Vec<ProjectSkillDto>,
    pub library_skills: Vec<ProjectLibrarySkillDto>,
    pub issues: Vec<ProjectIssueDto>,
}

#[tauri::command]
pub async fn list_projects(store: State<'_, SkillStore>) -> Result<Vec<ProjectSummaryDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let specs = runtime_project_tool_specs(&store)?;
        store
            .list_projects()?
            .into_iter()
            .map(|project| build_project_summary(&store, &project, &specs))
            .collect::<anyhow::Result<Vec<_>>>()
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn import_project(
    store: State<'_, SkillStore>,
    projectPath: String,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = expand_home_path(&projectPath)?;
        let runtime_tools = runtime_project_tools(&store)?;
        let default_tools = runtime_tools
            .iter()
            .map(|tool| tool.key.clone())
            .collect::<Vec<_>>();
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = import_project_and_scan(&store, &root, &default_tools, &specs)?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn scan_project(
    store: State<'_, SkillStore>,
    projectId: String,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = refresh_project(&store, &projectId, &specs)?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn update_project_settings(
    store: State<'_, SkillStore>,
    projectId: String,
    enabledToolKeys: Vec<String>,
    localGitExcludeEnabled: bool,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let available = runtime_tools
            .iter()
            .map(|tool| tool.key.clone())
            .collect::<Vec<_>>();
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = update_project_and_scan(
            &store,
            &projectId,
            &enabledToolKeys,
            localGitExcludeEnabled,
            &available,
            &specs,
        )?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn inject_project_skill(
    store: State<'_, SkillStore>,
    projectId: String,
    skillId: String,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = inject_project_skill_and_scan(&store, &projectId, &skillId, &specs)?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn remove_project_skill(
    store: State<'_, SkillStore>,
    projectId: String,
    skillId: String,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = remove_project_skill_and_scan(&store, &projectId, &skillId, &specs)?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn repair_project_issue(
    store: State<'_, SkillStore>,
    projectId: String,
    issueId: String,
    actionKind: String,
    actionValue: Option<String>,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = repair_project_issue_and_scan(
            &store,
            &projectId,
            &issueId,
            &actionKind,
            actionValue.as_deref(),
            &specs,
        )?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn collect_project_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    projectId: String,
    discoveredSkillId: String,
    libraryName: Option<String>,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = collect_project_skill_and_scan(
            &app,
            &store,
            &projectId,
            &discoveredSkillId,
            libraryName.as_deref(),
            &specs,
        )?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

fn runtime_project_tools(store: &SkillStore) -> anyhow::Result<Vec<RuntimeTool>> {
    Ok(runtime_tools(store, false)?
        .into_iter()
        .filter(|tool| tool.supports_project_scope && !tool.project_skills_dir.is_empty())
        .collect())
}

fn runtime_project_tool_specs(store: &SkillStore) -> anyhow::Result<Vec<ProjectToolSpec>> {
    runtime_project_tools(store).map(|tools| project_specs_from_runtime(&tools))
}

fn project_specs_from_runtime(tools: &[RuntimeTool]) -> Vec<ProjectToolSpec> {
    tools
        .iter()
        .map(|tool| ProjectToolSpec::new(&tool.key, &tool.project_skills_dir))
        .collect()
}

fn build_project_summary(
    store: &SkillStore,
    project: &ProjectRecord,
    specs: &[ProjectToolSpec],
) -> anyhow::Result<ProjectSummaryDto> {
    let data = build_project_summary_data(store, project, specs)?;
    let attention_count = visible_project_issues(store, project, data.issues)?.len();
    Ok(ProjectSummaryDto {
        id: project.id.clone(),
        name: project.name.clone(),
        root_path: project.root_path.clone(),
        enabled_tool_keys: project.enabled_tool_keys.clone(),
        local_git_exclude_enabled: project.local_git_exclude_enabled,
        created_at: project.created_at,
        updated_at: project.updated_at,
        last_scanned_at: project.last_scanned_at,
        skill_count: data.skill_count,
        attention_count,
    })
}

fn build_project_detail(
    store: &SkillStore,
    project: &ProjectRecord,
    runtime_tools: &[RuntimeTool],
) -> anyhow::Result<ProjectDetailDto> {
    let specs = project_specs_from_runtime(runtime_tools);
    let data = build_project_detail_data(store, project, &specs)?;
    let issues = visible_project_issues(store, project, data.issues)?;
    let summary = build_project_summary(store, project, &specs)?;
    let tools = runtime_tools
        .iter()
        .map(|tool| ProjectToolDto {
            key: tool.key.clone(),
            label: tool.label.clone(),
            project_skills_dir: tool.project_skills_dir.clone(),
            enabled: project.enabled_tool_keys.contains(&tool.key),
        })
        .collect::<Vec<_>>();
    Ok(ProjectDetailDto {
        project: summary,
        tools,
        current_skills: data
            .current_skills
            .into_iter()
            .map(|skill| ProjectSkillDto {
                name: skill.name,
                description: skill.description,
                source: skill.source,
                status: skill.status,
                linked_skill_id: skill.linked_skill_id,
                matched_skill_id: skill.matched_skill_id,
                tool_keys: skill.tool_keys,
                paths: skill
                    .paths
                    .into_iter()
                    .map(|path| ProjectSkillPathDto {
                        discovered_skill_id: path.discovered_skill_id,
                        relative_path: path.relative_path,
                        absolute_path: path.absolute_path,
                        tool_keys: path.tool_keys,
                        content_hash: path.content_hash,
                        status: path.status,
                    })
                    .collect(),
            })
            .collect(),
        library_skills: data
            .library_skills
            .into_iter()
            .map(|skill| ProjectLibrarySkillDto {
                skill_id: skill.skill_id,
                name: skill.name,
                description: skill.description,
                availability: skill.availability,
            })
            .collect(),
        issues,
    })
}

fn project_issue_dto(projection: ProjectIssueProjection) -> ProjectIssueDto {
    let ProjectIssueProjection {
        issue,
        fingerprint,
        can_ignore,
        ..
    } = projection;
    ProjectIssueDto {
        id: issue.id,
        kind: issue.kind,
        skill_id: issue.skill_id,
        skill_name: issue.skill_name,
        tool_key: issue.tool_key,
        path: issue.path,
        actions: issue
            .actions
            .into_iter()
            .map(|action| ProjectIssueActionDto {
                kind: action.kind,
                value: action.value,
                label: action.label,
                confirmation_required: action.confirmation_required,
                destructive: action.destructive,
            })
            .collect(),
        fingerprint,
        can_ignore,
    }
}

fn visible_project_issues(
    store: &SkillStore,
    project: &ProjectRecord,
    issues: Vec<ProjectIssue>,
) -> anyhow::Result<Vec<ProjectIssueDto>> {
    Ok(visible_project_issue_projections(store, project, issues)?
        .into_iter()
        .map(project_issue_dto)
        .collect())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn ignore_project_issue(
    store: State<'_, SkillStore>,
    projectId: String,
    issueId: String,
    fingerprint: String,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let specs = project_specs_from_runtime(&runtime_tools);
        let project =
            ignore_project_issue_and_scan(&store, &projectId, &issueId, &fingerprint, &specs)?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn ignore_all_project_issues(
    store: State<'_, SkillStore>,
    projectId: String,
) -> Result<ProjectDetailDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let runtime_tools = runtime_project_tools(&store)?;
        let specs = project_specs_from_runtime(&runtime_tools);
        let project = ignore_all_project_issues_and_scan(&store, &projectId, &specs)?;
        build_project_detail(&store, &project, &runtime_tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_recent_projects(store: State<'_, SkillStore>) -> Result<Vec<String>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || get_recent_projects_impl(&store))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_recent_project(
    store: State<'_, SkillStore>,
    projectPath: String,
) -> Result<Vec<String>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || save_recent_project_impl(&store, &projectPath))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

fn get_recent_projects_impl(store: &SkillStore) -> Result<Vec<String>, anyhow::Error> {
    let projects = store
        .get_setting(RECENT_PROJECTS_SETTING)?
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();
    Ok(projects)
}

fn save_recent_project_impl(
    store: &SkillStore,
    project_path: &str,
) -> Result<Vec<String>, anyhow::Error> {
    let path = expand_home_path(project_path)?;
    if !path.is_dir() {
        anyhow::bail!("projectPath must be an existing directory: {:?}", path);
    }
    let normalized = path.to_string_lossy().to_string();
    let mut projects = get_recent_projects_impl(store)?;
    projects.retain(|item| item != &normalized);
    projects.insert(0, normalized);
    projects.truncate(8);
    store.set_setting(
        RECENT_PROJECTS_SETTING,
        &serde_json::to_string(&projects).unwrap_or_else(|_| "[]".to_string()),
    )?;
    Ok(projects)
}

#[tauri::command]
pub async fn get_central_repo_path(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_central_repo_path(&app, &store)?;
        ensure_central_repo(&path)?;
        Ok::<_, anyhow::Error>(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_central_repo_path(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    path: String,
) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let new_base = expand_home_path(&path)?;
        if !new_base.is_absolute() {
            anyhow::bail!("storage path must be absolute");
        }
        ensure_central_repo(&new_base)?;

        let current_base = resolve_central_repo_path(&app, &store)?;
        let skills = store.list_skills()?;
        if current_base == new_base {
            store.set_setting("central_repo_path", new_base.to_string_lossy().as_ref())?;
            return Ok::<_, anyhow::Error>(new_base.to_string_lossy().to_string());
        }

        if !skills.is_empty() {
            for skill in skills {
                let old_path = std::path::PathBuf::from(&skill.central_path);
                if !old_path.exists() {
                    anyhow::bail!("central path not found: {:?}", old_path);
                }
                let file_name = old_path
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("invalid central path: {:?}", old_path))?;
                let new_path = new_base.join(file_name);
                if new_path.exists() {
                    anyhow::bail!("target path already exists: {:?}", new_path);
                }

                if let Err(err) = std::fs::rename(&old_path, &new_path) {
                    copy_dir_recursive(&old_path, &new_path)
                        .with_context(|| format!("copy {:?} -> {:?}", old_path, new_path))?;
                    std::fs::remove_dir_all(&old_path)
                        .with_context(|| format!("cleanup {:?}", old_path))?;
                    // Surface rename error in logs for troubleshooting.
                    eprintln!("rename failed, fallback used: {}", err);
                }

                let mut updated = skill.clone();
                updated.central_path = new_path.to_string_lossy().to_string();
                updated.updated_at = now_ms();
                store.upsert_skill(&updated)?;
            }
        }

        store.set_setting("central_repo_path", new_base.to_string_lossy().as_ref())?;
        Ok::<_, anyhow::Error>(new_base.to_string_lossy().to_string())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_local(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    sourcePath: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = install_local_skill(&app, &store, sourcePath.as_ref(), name)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_local_skills_cmd(basePath: String) -> Result<Vec<LocalSkillCandidate>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(basePath);
        list_local_skills(&path)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_local_selection(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    basePath: String,
    subpath: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let base = std::path::PathBuf::from(basePath);
        let result =
            install_local_skill_from_selection(&app, &store, base.as_ref(), &subpath, name)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_git(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    cancel: State<'_, Arc<CancelToken>>,
    repoUrl: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    cancel.reset();
    let cancel_token = Arc::clone(cancel.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let result = install_git_skill(&app, &store, &repoUrl, name, Some(&cancel_token))?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn list_git_skills_cmd(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    repoUrl: String,
) -> Result<Vec<GitSkillCandidate>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || list_git_skills(&app, &store, &repoUrl))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn install_git_selection(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    repoUrl: String,
    subpath: String,
    name: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = install_git_skill_from_selection(&app, &store, &repoUrl, &subpath, name)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn reimport_local_selection(
    store: State<'_, SkillStore>,
    skillId: String,
    basePath: String,
    subpath: String,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let base = std::path::PathBuf::from(basePath);
        let result = reimport_local_skill_from_selection(&store, &skillId, &base, &subpath)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn reimport_git_selection(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
    repoUrl: String,
    subpath: String,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = reimport_git_skill_from_selection(&app, &store, &skillId, &repoUrl, &subpath)?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct SyncResultDto {
    pub mode_used: String,
    pub target_path: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AssignmentDraftDto {
    pub global_tools: Vec<String>,
    pub projects: Vec<ProjectAssignmentDraftDto>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProjectAssignmentDraftDto {
    pub project_id: String,
    pub tool_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AssignmentPreviewItemDto {
    pub key: String,
    pub skill_id: String,
    pub skill_name: String,
    pub tool: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub target_path: Option<String>,
    pub outcome: String,
    pub blocking: bool,
}

#[derive(Debug, Serialize)]
pub struct AssignmentPreviewDto {
    pub items: Vec<AssignmentPreviewItemDto>,
}

#[derive(Debug, Serialize)]
pub struct SkillAssignmentApplyResultDto {
    pub skill_id: String,
    pub skill_name: String,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AssignmentApplyResultDto {
    pub skills: Vec<SkillAssignmentApplyResultDto>,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl From<AssignmentDraftDto> for AssignmentDraft {
    fn from(draft: AssignmentDraftDto) -> Self {
        Self {
            global_tools: draft.global_tools,
            projects: draft
                .projects
                .into_iter()
                .map(|project| ProjectAssignmentDraft {
                    project_id: project.project_id,
                    tool_keys: project.tool_keys,
                })
                .collect(),
        }
    }
}

fn assignment_tool_specs(store: &SkillStore) -> anyhow::Result<Vec<AssignmentToolSpec>> {
    Ok(runtime_tools(store, true)?
        .into_iter()
        .map(|tool| AssignmentToolSpec {
            key: tool.key,
            label: tool.label,
            global_skills_dir: tool.skills_dir,
            project_skills_dir: tool.project_skills_dir,
            global_available: tool.installed,
            enabled: tool.enabled,
            supports_project_scope: tool.supports_project_scope,
        })
        .collect())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn preview_skill_assignments(
    store: State<'_, SkillStore>,
    skillIds: Vec<String>,
    draft: AssignmentDraftDto,
    additive: Option<bool>,
) -> Result<AssignmentPreviewDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let tools = assignment_tool_specs(&store)?;
        let draft: AssignmentDraft = draft.into();
        let mut items = Vec::new();
        for preview in preview_skill_assignments_core(
            &store,
            &skillIds,
            &draft,
            &tools,
            additive.unwrap_or(false),
        )? {
            items.extend(
                preview
                    .items
                    .into_iter()
                    .map(|item| AssignmentPreviewItemDto {
                        key: AssignmentConflictKey::new(&preview.skill.id, item.key).encode(),
                        skill_id: preview.skill.id.clone(),
                        skill_name: preview.skill.name.clone(),
                        tool: item.tool,
                        scope: item.scope,
                        project_id: item.project_id,
                        project_name: item.project_name,
                        target_path: item.target_path,
                        outcome: item.outcome,
                        blocking: item.blocking,
                    }),
            );
        }
        Ok::<_, anyhow::Error>(AssignmentPreviewDto { items })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn apply_skill_assignments(
    store: State<'_, SkillStore>,
    skillIds: Vec<String>,
    draft: AssignmentDraftDto,
    replaceConflictKeys: Vec<String>,
    additive: Option<bool>,
) -> Result<AssignmentApplyResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let tools = assignment_tool_specs(&store)?;
        let draft: AssignmentDraft = draft.into();
        let replacements = replaceConflictKeys
            .into_iter()
            .map(|key| AssignmentConflictKey::decode(&key))
            .collect::<anyhow::Result<std::collections::HashSet<_>>>()?;
        let results = apply_skill_assignments_core(
            &store,
            &skillIds,
            &draft,
            &tools,
            &replacements,
            additive.unwrap_or(false),
        )?
        .into_iter()
        .map(|outcome| SkillAssignmentApplyResultDto {
            skill_id: outcome.skill.id,
            skill_name: outcome.skill.name,
            succeeded: outcome.summary.succeeded,
            failed: outcome.summary.failed,
            skipped: outcome.summary.skipped,
            errors: outcome.summary.errors,
        })
        .collect::<Vec<_>>();
        Ok::<_, anyhow::Error>(AssignmentApplyResultDto {
            succeeded: results.iter().map(|result| result.succeeded).sum(),
            failed: results.iter().map(|result| result.failed).sum(),
            skipped: results.iter().map(|result| result.skipped).sum(),
            skills: results,
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn resume_skill_assignments(
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<SkillAssignmentApplyResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skillId)?
            .with_context(|| format!("skill not found: {skillId}"))?;
        let tools = assignment_tool_specs(&store)?;
        let summary = resume_skill_assignment_core(&store, &skill.id, &tools)?;
        Ok::<_, anyhow::Error>(SkillAssignmentApplyResultDto {
            skill_id: skill.id,
            skill_name: skill.name,
            succeeded: summary.succeeded,
            failed: summary.failed,
            skipped: summary.skipped,
            errors: summary.errors,
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn ignore_assignment_issue(
    store: State<'_, SkillStore>,
    skillId: String,
    issueKey: String,
    fingerprint: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let tools = assignment_tool_specs(&store)?;
        ignore_assignment_issue_core(&store, &skillId, &issueKey, &fingerprint, &tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn ignore_all_assignment_issues(
    store: State<'_, SkillStore>,
    skillIds: Vec<String>,
) -> Result<usize, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let tools = assignment_tool_specs(&store)?;
        ignore_all_assignment_issues_core(&store, &skillIds, &tools)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn sync_skill_dir(
    source_path: String,
    target_path: String,
) -> Result<SyncResultDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = sync_dir_hybrid(source_path.as_ref(), target_path.as_ref())?;
        Ok::<_, anyhow::Error>(SyncResultDto {
            mode_used: match result.mode_used {
                SyncMode::Auto => "auto",
                SyncMode::Symlink => "symlink",
                SyncMode::Junction => "junction",
                SyncMode::Copy => "copy",
            }
            .to_string(),
            target_path: result.target_path.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_skill_enabled(
    store: State<'_, SkillStore>,
    skillId: String,
    enabled: bool,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !enabled {
            let targets = store.list_skill_targets(&skillId)?;
            let mut remove_failures: Vec<String> = Vec::new();
            for target in targets {
                if target.status != "disabled" {
                    if let Err(err) = remove_path_any(&target.target_path) {
                        remove_failures.push(format!("{}: {}", target.target_path, err));
                    }
                }
                store.update_skill_target_status(
                    &skillId,
                    &target.tool,
                    &target.scope,
                    target.project_path.as_deref(),
                    "disabled",
                )?;
            }
            store.set_skill_assignments_state(&skillId, "suspended")?;
            store.set_skill_enabled(&skillId, false)?;
            if !remove_failures.is_empty() {
                anyhow::bail!(
                    "已停用 Skill，但清理部分工具目录失败：\n- {}",
                    remove_failures.join("\n- ")
                );
            }
            return Ok::<_, anyhow::Error>(());
        }

        store.set_skill_enabled(&skillId, true)?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct UpdateResultDto {
    pub skill_id: String,
    pub name: String,
    pub content_hash: Option<String>,
    pub source_revision: Option<String>,
    pub updated_targets: Vec<String>,
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn update_managed_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<UpdateResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let res = update_managed_skill_from_source(&app, &store, &skillId)?;
        Ok::<_, anyhow::Error>(UpdateResultDto {
            skill_id: res.skill_id,
            name: res.name,
            content_hash: res.content_hash,
            source_revision: res.source_revision,
            updated_targets: res.updated_targets,
        })
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn search_github(
    store: State<'_, SkillStore>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<RepoSummary>, String> {
    let store = store.inner().clone();
    let limit = limit.unwrap_or(10) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let token = store.get_setting("github_token")?.unwrap_or_default();
        let proxy_url = get_github_proxy_url_core(&store)?;
        let token_opt = if token.is_empty() {
            None
        } else {
            Some(token.as_str())
        };
        search_github_repos(&query, limit, token_opt, &proxy_url)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_github_token(store: State<'_, SkillStore>) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<_, anyhow::Error>(store.get_setting("github_token")?.unwrap_or_default())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn set_github_token(store: State<'_, SkillStore>, token: String) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            store.set_setting("github_token", "")?;
        } else {
            store.set_setting("github_token", trimmed)?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_github_proxy_config(
    store: State<'_, SkillStore>,
) -> Result<GithubProxyConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        get_github_proxy_config_core(&store).map(to_github_proxy_config_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_github_proxy_config(
    store: State<'_, SkillStore>,
    enabled: bool,
    port: u16,
) -> Result<GithubProxyConfigDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        set_github_proxy_config_core(&store, enabled, port).map(to_github_proxy_config_dto)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn get_github_proxy_url(store: State<'_, SkillStore>) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || get_github_proxy_url_core(&store))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_github_proxy_url(
    store: State<'_, SkillStore>,
    proxyUrl: String,
) -> Result<String, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_github_proxy_url_core(&store, &proxyUrl))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn import_existing_skill(
    app: tauri::AppHandle,
    store: State<'_, SkillStore>,
    sourcePath: String,
    name: Option<String>,
    conflictResolution: Option<String>,
) -> Result<InstallResultDto, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = std::path::Path::new(&sourcePath);
        // Validate SKILL.md exists before importing (fixes #8: prevents importing
        // directories that were "discovered" but lack a valid SKILL.md).
        if !source.join("SKILL.md").exists() {
            anyhow::bail!("SKILL_INVALID|missing_skill_md");
        }
        let name = name.unwrap_or_else(|| {
            source
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed-skill".to_string())
        });
        let result =
            import_discovered_skill(&app, &store, source, &name, conflictResolution.as_deref())?;
        Ok::<_, anyhow::Error>(to_install_dto(result))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct ManagedSkillDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub central_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_sync_at: Option<i64>,
    pub enabled: bool,
    pub status: String,
    pub tags: Vec<TagDto>,
    pub targets: Vec<SkillTargetDto>,
    pub assignments: Vec<SkillAssignmentDto>,
    pub source_issue: Option<String>,
    pub pending_assignment: bool,
    pub assignment_issue_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SkillAssignmentDto {
    pub tool: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_path: Option<String>,
    pub ownership: String,
    pub state: String,
    pub last_error: Option<String>,
    pub valid: bool,
    pub issue_kind: Option<String>,
    pub issue_key: Option<String>,
    pub issue_fingerprint: Option<String>,
    pub can_ignore: bool,
    pub issue_ignored: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagDto {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct TagWithCountDto {
    pub id: i64,
    pub name: String,
    pub skill_count: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct SkillTargetDto {
    pub tool: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub mode: String,
    pub status: String,
    pub target_path: String,
    pub synced_at: Option<i64>,
}

#[tauri::command]
pub async fn get_managed_skills(
    store: State<'_, SkillStore>,
) -> Result<Vec<ManagedSkillDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || get_managed_skills_impl(&store))
        .await
        .map_err(|err| err.to_string())?
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub fn get_tags(store: State<'_, SkillStore>) -> Result<Vec<TagWithCountDto>, String> {
    store
        .list_tags_with_counts()
        .map(|tags| {
            tags.into_iter()
                .map(|tag| TagWithCountDto {
                    id: tag.id,
                    name: tag.name,
                    skill_count: tag.skill_count,
                    updated_at: tag.updated_at,
                })
                .collect()
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn create_tag(store: State<'_, SkillStore>, name: String) -> Result<TagDto, String> {
    store
        .create_tag(&name)
        .map(|tag| TagDto {
            id: tag.id,
            name: tag.name,
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn rename_tag(
    store: State<'_, SkillStore>,
    tagId: i64,
    name: String,
) -> Result<TagDto, String> {
    store
        .rename_tag(tagId, &name)
        .map(|tag| TagDto {
            id: tag.id,
            name: tag.name,
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn delete_tag(store: State<'_, SkillStore>, tagId: i64) -> Result<(), String> {
    store.delete_tag(tagId).map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_skill_tags(
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<Vec<TagDto>, String> {
    store
        .get_skill_tags(&skillId)
        .map(|tags| {
            tags.into_iter()
                .map(|tag| TagDto {
                    id: tag.id,
                    name: tag.name,
                })
                .collect()
        })
        .map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_skill_tags(
    store: State<'_, SkillStore>,
    skillId: String,
    tagIds: Vec<i64>,
) -> Result<(), String> {
    store
        .set_skill_tags(&skillId, &tagIds)
        .map_err(format_anyhow_error)
}

#[tauri::command]
pub fn get_untagged_skill_ids(store: State<'_, SkillStore>) -> Result<Vec<String>, String> {
    store.list_untagged_skill_ids().map_err(format_anyhow_error)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn delete_managed_skill(
    store: State<'_, SkillStore>,
    skillId: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 便于排查“按钮点了没反应”：确认前端确实触发了命令
        println!("[delete_managed_skill] skillId={}", skillId);

        // 先删除已同步到各工具目录的副本/软链接
        // 注意：如果先删 skills 行，会触发 skill_targets cascade，导致无法再拿到 target_path
        let targets = store.list_skill_targets(&skillId)?;

        let mut remove_failures: Vec<String> = Vec::new();
        for target in targets {
            if let Err(err) = remove_path_any(&target.target_path) {
                remove_failures.push(format!("{}: {}", target.target_path, err));
            }
        }

        let record = store.get_skill_by_id(&skillId)?;
        if let Some(skill) = record {
            let path = std::path::PathBuf::from(skill.central_path);
            if path.exists() {
                std::fs::remove_dir_all(&path)?;
            }
            store.delete_skill(&skillId)?;
        }

        if !remove_failures.is_empty() {
            anyhow::bail!(
                "已删除托管记录，但清理部分工具目录失败：\n- {}",
                remove_failures.join("\n- ")
            );
        }

        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

fn remove_path_any(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Ok(());
    }

    let meta = std::fs::symlink_metadata(p).map_err(|err| err.to_string())?;
    let ft = meta.file_type();

    // 软链接（即使指向目录）也应该用 remove_file 删除链接本身
    if ft.is_symlink() {
        std::fs::remove_file(p).map_err(|err| err.to_string())?;
        return Ok(());
    }

    if ft.is_dir() {
        std::fs::remove_dir_all(p).map_err(|err| err.to_string())?;
        return Ok(());
    }

    std::fs::remove_file(p).map_err(|err| err.to_string())?;
    Ok(())
}

fn to_install_dto(result: InstallResult) -> InstallResultDto {
    InstallResultDto {
        skill_id: result.skill_id,
        name: result.name,
        central_path: result.central_path.to_string_lossy().to_string(),
        content_hash: result.content_hash,
    }
}

fn to_auto_update_config_dto(mut config: AutoUpdateConfig) -> AutoUpdateConfigDto {
    let task_status = get_auto_update_task_status();
    if config.last_status.as_deref() == Some("running")
        && task_status.detail.contains("state = not running")
    {
        config.last_status = Some("stopped".to_string());
    }
    AutoUpdateConfigDto {
        enabled: config.enabled,
        interval_hours: config.interval_hours,
        schedule_type: match config.schedule.schedule_type {
            AutoUpdateScheduleType::Interval => "interval".to_string(),
            AutoUpdateScheduleType::Daily => "daily".to_string(),
        },
        interval_value: config.schedule.interval_value,
        interval_unit: match config.schedule.interval_unit {
            AutoUpdateIntervalUnit::Minutes => "minutes".to_string(),
            AutoUpdateIntervalUnit::Hours => "hours".to_string(),
        },
        daily_time: config.schedule.daily_time,
        local_skill_count: config.local_skill_count,
        protected_local_skill_count: config.protected_local_skill_count,
        task_registered: task_status.registered,
        task_status_detail: task_status.detail,
        last_run_at: config.last_run_at,
        last_started_at: config.last_started_at,
        last_finished_at: config.last_finished_at,
        last_status: config.last_status,
        last_error: config.last_error,
        last_checked: config.last_checked,
        last_updated: config.last_updated,
        last_failed: config.last_failed,
        progress: config.progress,
    }
}

fn to_auto_update_run_result_dto(result: AutoUpdateRunResult) -> AutoUpdateRunResultDto {
    AutoUpdateRunResultDto {
        checked: result.checked,
        updated: result.updated,
        failed: result.failed,
        errors: result.errors,
        progress: result.progress,
    }
}

fn to_github_proxy_config_dto(config: GithubProxyConfig) -> GithubProxyConfigDto {
    GithubProxyConfigDto {
        enabled: config.enabled,
        port: config.port,
        url: config.url,
        auto_detected: config.auto_detected,
    }
}

fn now_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as i64
}

fn get_managed_skills_impl(store: &SkillStore) -> anyhow::Result<Vec<ManagedSkillDto>> {
    let tools = assignment_tool_specs(store)?;
    let projections = managed_skill_projections(store, &tools)?;
    let mut managed = Vec::with_capacity(projections.len());
    for projection in projections {
        let skill = projection.skill;
        let targets = projection
            .targets
            .into_iter()
            .map(|target| SkillTargetDto {
                tool: target.tool,
                scope: target.scope,
                project_path: target.project_path,
                mode: target.mode,
                status: target.status,
                target_path: target.target_path,
                synced_at: target.synced_at,
            })
            .collect();
        let assignments = projection
            .assignments
            .into_iter()
            .map(|projection| skill_assignment_dto(store, projection))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let tags = store
            .get_skill_tags(&skill.id)?
            .into_iter()
            .map(|tag| TagDto {
                id: tag.id,
                name: tag.name,
            })
            .collect();

        managed.push(ManagedSkillDto {
            id: skill.id,
            name: skill.name,
            description: skill.description,
            source_type: skill.source_type,
            source_ref: skill.source_ref,
            central_path: skill.central_path,
            created_at: skill.created_at,
            updated_at: skill.updated_at,
            last_sync_at: skill.last_sync_at,
            enabled: skill.enabled,
            status: skill.status,
            tags,
            targets,
            assignments,
            source_issue: projection.source_issue,
            pending_assignment: projection.pending_assignment,
            assignment_issue_count: projection.assignment_issue_count,
        });
    }
    Ok(managed)
}

fn skill_assignment_dto(
    store: &SkillStore,
    projection: AssignmentProjection,
) -> anyhow::Result<SkillAssignmentDto> {
    let assignment = projection.assignment;
    let project = match assignment.project_id.as_deref() {
        Some(project_id) => store.get_project(project_id)?,
        None => None,
    };
    Ok(SkillAssignmentDto {
        tool: assignment.tool,
        scope: assignment.scope,
        project_id: assignment.project_id,
        project_name: project.as_ref().map(|item| item.name.clone()),
        project_path: project.map(|item| item.root_path),
        ownership: assignment.ownership,
        state: projection.effective_state,
        last_error: projection.effective_error,
        valid: projection.valid,
        issue_kind: projection.issue_kind,
        issue_key: projection.issue_key,
        issue_fingerprint: projection.issue_fingerprint,
        can_ignore: projection.can_ignore,
        issue_ignored: projection.ignored,
    })
}

#[derive(Debug, Serialize)]
pub struct FeaturedSkillDto {
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub downloads: u64,
    pub stars: u64,
    pub source_url: String,
}

impl From<FeaturedSkill> for FeaturedSkillDto {
    fn from(s: FeaturedSkill) -> Self {
        Self {
            slug: s.slug,
            name: s.name,
            summary: s.summary,
            downloads: s.downloads,
            stars: s.stars,
            source_url: s.source_url,
        }
    }
}

#[tauri::command]
pub async fn get_featured_skills(
    store: State<'_, SkillStore>,
) -> Result<Vec<FeaturedSkillDto>, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = fetch_featured_skills(&store)?;
        Ok::<_, anyhow::Error>(skills.into_iter().map(FeaturedSkillDto::from).collect())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
pub struct OnlineSkillDto {
    pub name: String,
    pub installs: u64,
    pub source: String,
    pub source_url: String,
}

impl From<OnlineSkillResult> for OnlineSkillDto {
    fn from(r: OnlineSkillResult) -> Self {
        Self {
            name: r.name,
            installs: r.installs,
            source: r.source,
            source_url: r.source_url,
        }
    }
}

#[tauri::command]
pub async fn search_skills_online(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<OnlineSkillDto>, String> {
    let limit = limit.unwrap_or(20) as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let results = search_skills_online_core(&query, limit)?;
        Ok::<_, anyhow::Error>(results.into_iter().map(OnlineSkillDto::from).collect())
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileEntry {
    pub path: String,
    pub size: u64,
}

#[tauri::command]
pub async fn list_skill_files(central_path: String) -> Result<Vec<SkillFileEntry>, String> {
    let path = std::path::PathBuf::from(&central_path);
    tauri::async_runtime::spawn_blocking(move || {
        let entries = crate::core::skill_files::list_files(&path)?;
        Ok::<_, anyhow::Error>(
            entries
                .into_iter()
                .map(|e| SkillFileEntry {
                    path: e.path,
                    size: e.size,
                })
                .collect(),
        )
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub async fn read_skill_file(central_path: String, file_path: String) -> Result<String, String> {
    let base = std::path::PathBuf::from(&central_path);
    tauri::async_runtime::spawn_blocking(move || {
        crate::core::skill_files::read_file(&base, &file_path)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(format_anyhow_error)
}

#[tauri::command]
pub fn cancel_current_operation(cancel: State<'_, Arc<CancelToken>>) -> Result<(), String> {
    cancel.cancel();
    Ok(())
}

#[cfg(test)]
#[path = "tests/commands.rs"]
mod tests;
