use std::collections::HashSet;
use std::fs;
use std::path::Path;

use tempfile::tempdir;

use super::*;
use crate::core::prompt_adapters::{save_custom_prompt_profiles, PromptProfileInput, PromptTool};
use crate::core::skill_store::{ProjectRecord, SkillStore};

#[test]
fn filters_prompt_paths_and_skipped_directories() {
    let exact = HashSet::from([normalized_path_key(Path::new("/tmp/custom.md"))]);
    let names = HashSet::from(["AGENTS.md".to_string()]);

    assert!(prompt_watch_path_relevant(
        Path::new("/tmp/custom.md"),
        &exact,
        &names
    ));
    assert!(prompt_watch_path_relevant(
        Path::new("/tmp/project/AGENTS.md"),
        &exact,
        &names
    ));
    assert!(!prompt_watch_path_relevant(
        Path::new("/tmp/project/node_modules/pkg/AGENTS.md"),
        &exact,
        &names
    ));
}

#[test]
fn watch_plan_includes_prompt_parents_without_watching_whole_tree() {
    let directory = tempdir().expect("temp dir");
    let store = SkillStore::new(directory.path().join("skills.db"));
    store.ensure_schema().expect("schema");
    let profile_root = directory.path().join("codex-home");
    let project_root = directory.path().join("project");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::create_dir_all(project_root.join("nested")).expect("nested");
    fs::write(project_root.join("nested/AGENTS.md"), "nested").expect("prompt");
    store
        .insert_project(&ProjectRecord {
            id: "project-1".to_string(),
            name: "Project".to_string(),
            root_path: project_root.to_string_lossy().to_string(),
            enabled_tool_keys: Vec::new(),
            local_git_exclude_enabled: false,
            created_at: 1,
            updated_at: 1,
            last_scanned_at: None,
        })
        .expect("project");
    let profile_id = save_custom_prompt_profiles(
        &store,
        vec![PromptProfileInput {
            id: "codex:test".to_string(),
            tool: PromptTool::Codex,
            label: "Test".to_string(),
            config_root: profile_root.to_string_lossy().to_string(),
        }],
    )
    .expect("profile")
    .into_iter()
    .find(|profile| profile.source == "custom")
    .expect("custom")
    .id;

    let plan = build_prompt_watch_plan(&store, &profile_id, Some("project-1")).expect("watch plan");
    assert!(plan
        .roots
        .iter()
        .any(|root| root.path == project_root.join("nested") && !root.recursive));
    assert!(plan
        .roots
        .iter()
        .any(|root| root.path == project_root && !root.recursive));
    assert!(plan.roots.len() <= MAX_PROMPT_WATCH_ROOTS);
}
