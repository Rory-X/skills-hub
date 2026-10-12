use std::fs;

use tempfile::tempdir;

use super::*;

fn test_store() -> (tempfile::TempDir, SkillStore) {
    let directory = tempdir().expect("temp dir");
    let store = SkillStore::new(directory.path().join("skills_hub.db"));
    store.ensure_schema().expect("schema");
    (directory, store)
}

#[test]
fn lists_default_profiles_for_all_tools() {
    let (_directory, store) = test_store();
    let profiles = list_prompt_profiles(&store).expect("profiles");

    assert!(profiles
        .iter()
        .any(|profile| profile.id == "codex:default" && profile.tool == PromptTool::Codex));
    assert!(profiles.iter().any(|profile| {
        profile.id == "claude_code:default" && profile.tool == PromptTool::ClaudeCode
    }));
    assert!(profiles
        .iter()
        .any(|profile| profile.id == "pi:default" && profile.tool == PromptTool::Pi));
    assert!(
        profiles
            .iter()
            .any(|profile| profile.id == "ds_harness:default"
                && profile.tool == PromptTool::DsHarness)
    );
}

#[test]
fn saves_and_resolves_custom_profile() {
    let (directory, store) = test_store();
    let root = directory.path().join("custom-codex");
    let profiles = save_custom_prompt_profiles(
        &store,
        vec![PromptProfileInput {
            id: String::new(),
            tool: PromptTool::Codex,
            label: "Work Codex".to_string(),
            config_root: root.to_string_lossy().to_string(),
        }],
    )
    .expect("save profiles");
    let custom = profiles
        .into_iter()
        .find(|profile| profile.source == "custom")
        .expect("custom profile");

    let resolved = resolve_prompt_profile(&store, &custom.id).expect("resolve profile");
    assert_eq!(resolved.label, "Work Codex");
    assert_eq!(resolved.config_root, root.to_string_lossy());
}

#[test]
fn parses_codex_prompt_discovery_configuration() {
    let directory = tempdir().expect("temp dir");
    fs::write(
        directory.path().join("config.toml"),
        r#"
project_doc_max_bytes = 65536
project_doc_fallback_filenames = ["TEAM.md", "nested/unsafe.md"]
project_root_markers = [".git", ".project"]
"#,
    )
    .expect("write config");
    let profile = PromptProfile {
        id: "codex:test".to_string(),
        tool: PromptTool::Codex,
        label: "Test".to_string(),
        config_root: directory.path().to_string_lossy().to_string(),
        source: "custom".to_string(),
        is_default: false,
        config_root_exists: true,
        tool_version: None,
    };

    let config = load_codex_prompt_config(&profile);
    assert_eq!(config.project_doc_max_bytes, 65536);
    assert_eq!(config.fallback_filenames, vec!["TEAM.md"]);
    assert_eq!(config.project_root_markers, vec![".git", ".project"]);
    assert!(config.warning.is_none());
}

#[test]
fn merges_claude_excludes_from_user_and_project_settings() {
    let directory = tempdir().expect("temp dir");
    let profile_root = directory.path().join("profile");
    let project_root = directory.path().join("project");
    fs::create_dir_all(project_root.join(".claude")).expect("project settings dir");
    fs::create_dir_all(&profile_root).expect("profile dir");
    fs::write(
        profile_root.join("settings.json"),
        r#"{"claudeMdExcludes":["vendor/**"]}"#,
    )
    .expect("user settings");
    fs::write(
        project_root.join(".claude/settings.local.json"),
        r#"{"claudeMdExcludes":["generated/**","vendor/**"]}"#,
    )
    .expect("project settings");
    let profile = PromptProfile {
        id: "claude:test".to_string(),
        tool: PromptTool::ClaudeCode,
        label: "Test".to_string(),
        config_root: profile_root.to_string_lossy().to_string(),
        source: "custom".to_string(),
        is_default: false,
        config_root_exists: true,
        tool_version: None,
    };

    assert_eq!(
        load_claude_md_excludes(&profile, &project_root),
        vec!["generated/**", "vendor/**"]
    );
}
