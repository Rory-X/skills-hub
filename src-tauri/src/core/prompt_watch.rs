use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{recommended_watcher, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use uuid::Uuid;

use super::prompt_adapters::{
    load_codex_prompt_config, resolve_prompt_profile, should_skip_prompt_scan_dir, PromptTool,
};
use super::prompts::list_prompt_sources;
use super::skill_store::SkillStore;

const MAX_PROMPT_WATCH_ROOTS: usize = 32;
const PROMPT_WATCH_THROTTLE: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PromptWatchEvent {
    pub watch_id: String,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PromptWatchStatus {
    pub watch_id: String,
    pub watched_roots: Vec<String>,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct PromptWatchRoot {
    pub path: PathBuf,
    pub recursive: bool,
}

#[derive(Clone, Debug)]
pub struct PromptWatchPlan {
    pub roots: Vec<PromptWatchRoot>,
    pub exact_paths: HashSet<String>,
    pub filenames: HashSet<String>,
    pub truncated: bool,
}

struct ActivePromptWatch {
    _watcher: RecommendedWatcher,
    status: PromptWatchStatus,
}

#[derive(Default)]
pub struct PromptWatchService {
    active: Mutex<Option<ActivePromptWatch>>,
}

impl PromptWatchService {
    pub fn start<F>(&self, plan: PromptWatchPlan, emit: F) -> Result<PromptWatchStatus>
    where
        F: Fn(PromptWatchEvent) + Send + Sync + 'static,
    {
        let watch_id = Uuid::new_v4().to_string();
        let event_watch_id = watch_id.clone();
        let exact_paths = Arc::new(plan.exact_paths);
        let filenames = Arc::new(plan.filenames);
        let last_emitted = Arc::new(Mutex::new(None::<Instant>));
        let emit = Arc::new(emit);

        let mut watcher = recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let mut paths = event
                .paths
                .into_iter()
                .filter(|path| prompt_watch_path_relevant(path, &exact_paths, &filenames))
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>();
            paths.sort();
            paths.dedup();
            if paths.is_empty() {
                return;
            }
            let Ok(mut previous) = last_emitted.lock() else {
                return;
            };
            if previous
                .as_ref()
                .map(|instant| instant.elapsed() < PROMPT_WATCH_THROTTLE)
                .unwrap_or(false)
            {
                return;
            }
            *previous = Some(Instant::now());
            emit(PromptWatchEvent {
                watch_id: event_watch_id.clone(),
                paths,
            });
        })
        .context("create prompt file watcher")?;

        let mut watched_roots = Vec::new();
        for root in plan.roots {
            let mode = if root.recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            watcher
                .watch(&root.path, mode)
                .with_context(|| format!("watch prompt directory {:?}", root.path))?;
            watched_roots.push(root.path.to_string_lossy().to_string());
        }

        let status = PromptWatchStatus {
            watch_id,
            watched_roots,
            truncated: plan.truncated,
        };
        let mut active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("prompt watcher lock is poisoned"))?;
        *active = Some(ActivePromptWatch {
            _watcher: watcher,
            status: status.clone(),
        });
        Ok(status)
    }

    pub fn stop(&self, watch_id: Option<&str>) -> Result<bool> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("prompt watcher lock is poisoned"))?;
        let matches = active
            .as_ref()
            .map(|watch| {
                watch_id
                    .map(|expected| expected == watch.status.watch_id)
                    .unwrap_or(true)
            })
            .unwrap_or(false);
        if matches {
            active.take();
        }
        Ok(matches)
    }
}

pub fn build_prompt_watch_plan(
    store: &SkillStore,
    profile_id: &str,
    project_id: Option<&str>,
) -> Result<PromptWatchPlan> {
    let profile = resolve_prompt_profile(store, profile_id)?;
    let sources = list_prompt_sources(store, profile_id, project_id)?;
    let mut roots = Vec::new();
    let mut root_indexes = HashMap::new();
    let mut exact_paths = HashSet::new();
    let mut filenames = HashSet::from([
        "AGENTS.md".to_string(),
        "AGENTS.override.md".to_string(),
        "AGENTS.local.md".to_string(),
        "CLAUDE.md".to_string(),
        "CLAUDE.local.md".to_string(),
        "config.toml".to_string(),
        "settings.json".to_string(),
        "settings.local.json".to_string(),
    ]);

    let config_root = PathBuf::from(&profile.config_root);
    add_exact_path(&mut exact_paths, &config_root);
    add_watch_directory(&mut roots, &mut root_indexes, &config_root, false);
    add_watch_directory(
        &mut roots,
        &mut root_indexes,
        &config_root.join("rules"),
        true,
    );
    add_exact_path(&mut exact_paths, &config_root.join("config.toml"));
    add_exact_path(&mut exact_paths, &config_root.join("settings.json"));

    if profile.tool == PromptTool::Codex {
        filenames.extend(load_codex_prompt_config(&profile).fallback_filenames);
    }
    if profile.tool == PromptTool::Pi {
        filenames.extend([
            "SYSTEM.md".to_string(),
            "APPEND_SYSTEM.md".to_string(),
            "CLAUDE.md".to_string(),
        ]);
    }

    if let Some(project_id) = project_id {
        let project = store
            .get_project(project_id)?
            .with_context(|| format!("project not found: {project_id}"))?;
        let project_root = PathBuf::from(project.root_path);
        add_watch_directory(&mut roots, &mut root_indexes, &project_root, false);
        add_watch_directory(
            &mut roots,
            &mut root_indexes,
            &project_root.join(".claude"),
            false,
        );
        add_watch_directory(
            &mut roots,
            &mut root_indexes,
            &project_root.join(".claude/rules"),
            true,
        );
        if profile.tool == PromptTool::Pi {
            add_watch_directory(
                &mut roots,
                &mut root_indexes,
                &project_root.join(".pi"),
                false,
            );
        }
        add_exact_path(
            &mut exact_paths,
            &project_root.join(".claude/settings.json"),
        );
        add_exact_path(
            &mut exact_paths,
            &project_root.join(".claude/settings.local.json"),
        );
    }

    for source in sources {
        let logical = PathBuf::from(source.path);
        add_exact_path(&mut exact_paths, &logical);
        if let Some(parent) = logical.parent() {
            add_watch_directory(&mut roots, &mut root_indexes, parent, false);
        }
        if let Some(real_path) = source.real_path {
            let real = PathBuf::from(real_path);
            add_exact_path(&mut exact_paths, &real);
            if let Some(parent) = real.parent() {
                add_watch_directory(&mut roots, &mut root_indexes, parent, false);
            }
        }
    }

    let truncated = roots.len() > MAX_PROMPT_WATCH_ROOTS;
    roots.truncate(MAX_PROMPT_WATCH_ROOTS);
    Ok(PromptWatchPlan {
        roots,
        exact_paths,
        filenames,
        truncated,
    })
}

fn add_watch_directory(
    roots: &mut Vec<PromptWatchRoot>,
    indexes: &mut HashMap<String, usize>,
    requested: &Path,
    recursive: bool,
) {
    let recursive = recursive && requested.is_dir();
    let Some(directory) = nearest_existing_directory(requested) else {
        return;
    };
    let key = normalized_path_key(&directory);
    if let Some(index) = indexes.get(&key).copied() {
        if recursive {
            roots[index].recursive = true;
        }
        return;
    }
    indexes.insert(key, roots.len());
    roots.push(PromptWatchRoot {
        path: directory,
        recursive,
    });
}

fn nearest_existing_directory(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn add_exact_path(paths: &mut HashSet<String>, path: &Path) {
    paths.insert(normalized_path_key(path));
}

fn normalized_path_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_lowercase()
}

fn prompt_watch_path_relevant(
    path: &Path,
    exact_paths: &HashSet<String>,
    filenames: &HashSet<String>,
) -> bool {
    if path.ancestors().skip(1).any(should_skip_prompt_scan_dir) {
        return false;
    }
    if exact_paths.contains(&normalized_path_key(path)) {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| filenames.contains(name))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "tests/prompt_watch.rs"]
mod tests;
