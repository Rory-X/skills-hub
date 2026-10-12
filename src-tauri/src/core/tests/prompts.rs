use std::fs;
use std::path::Path;

use tempfile::tempdir;

use super::*;
use crate::core::prompt_adapters::{save_custom_prompt_profiles, PromptProfileInput};
use crate::core::skill_store::ProjectRecord;

fn test_store() -> (tempfile::TempDir, SkillStore) {
    let directory = tempdir().expect("temp dir");
    let store = SkillStore::new(directory.path().join("skills_hub.db"));
    store.ensure_schema().expect("schema");
    (directory, store)
}

fn insert_project(store: &SkillStore, root: &Path) {
    store
        .insert_project(&ProjectRecord {
            id: "project-1".to_string(),
            name: "Project".to_string(),
            root_path: root.to_string_lossy().to_string(),
            enabled_tool_keys: vec!["codex".to_string(), "claude_code".to_string()],
            local_git_exclude_enabled: false,
            created_at: 1,
            updated_at: 1,
            last_scanned_at: None,
        })
        .expect("insert project");
}

fn custom_profile(store: &SkillStore, tool: PromptTool, root: &Path) -> String {
    save_custom_prompt_profiles(
        store,
        vec![PromptProfileInput {
            id: format!("{}:test", tool.as_key()),
            tool,
            label: "Test".to_string(),
            config_root: root.to_string_lossy().to_string(),
        }],
    )
    .expect("save profile")
    .into_iter()
    .find(|profile| profile.source == "custom")
    .expect("custom profile")
    .id
}

#[test]
fn discovers_codex_root_nested_and_configured_fallback_files() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("codex-home");
    let project_root = directory.path().join("project");
    fs::create_dir_all(project_root.join("service")).expect("service");
    fs::create_dir_all(project_root.join("node_modules/package")).expect("ignored");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::write(
        profile_root.join("config.toml"),
        "project_doc_fallback_filenames = [\"TEAM.md\"]\n",
    )
    .expect("config");
    fs::write(project_root.join("AGENTS.md"), "root").expect("root prompt");
    fs::write(project_root.join("service/TEAM.md"), "service").expect("fallback");
    fs::write(
        project_root.join("node_modules/package/AGENTS.md"),
        "ignored",
    )
    .expect("ignored prompt");
    insert_project(&store, &project_root);
    let profile_id = custom_profile(&store, PromptTool::Codex, &profile_root);

    let sources =
        list_prompt_sources(&store, &profile_id, Some("project-1")).expect("list prompt sources");
    assert!(sources
        .iter()
        .any(|source| source.path.ends_with("project/AGENTS.md") && source.exists));
    assert!(sources
        .iter()
        .any(|source| source.path.ends_with("service/TEAM.md") && source.exists));
    assert!(!sources
        .iter()
        .any(|source| source.path.contains("node_modules")));
}

#[test]
fn codex_chain_uses_global_override_and_directory_precedence() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("codex-home");
    let project_root = directory.path().join("project");
    fs::create_dir_all(project_root.join("service")).expect("service");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::create_dir_all(project_root.join(".git")).expect("git marker");
    fs::write(profile_root.join("AGENTS.md"), "global base").expect("global base");
    fs::write(profile_root.join("AGENTS.override.md"), "global override").expect("global override");
    fs::write(project_root.join("AGENTS.md"), "project").expect("project");
    fs::write(
        project_root.join("service/AGENTS.override.md"),
        "service override",
    )
    .expect("service");
    insert_project(&store, &project_root);
    let profile_id = custom_profile(&store, PromptTool::Codex, &profile_root);

    let chain = resolve_prompt_instruction_chain(
        &store,
        &PromptScenario {
            profile_id,
            project_id: Some("project-1".to_string()),
            working_directory: Some(project_root.join("service").to_string_lossy().to_string()),
            target_path: None,
        },
    )
    .expect("resolve chain");
    let paths = chain
        .startup_sources
        .iter()
        .map(|source| source.source.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 3);
    assert!(paths[0].ends_with("codex-home/AGENTS.override.md"));
    assert!(paths[1].ends_with("project/AGENTS.md"));
    assert!(paths[2].ends_with("service/AGENTS.override.md"));
}

#[test]
fn ds_harness_chain_loads_global_and_per_directory_candidates() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("dsh-home");
    let project_root = directory.path().join("project");
    fs::create_dir_all(project_root.join("src")).expect("src");
    fs::create_dir_all(project_root.join(".git")).expect("git marker");
    fs::create_dir_all(project_root.join("node_modules/pkg")).expect("ignored dir");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::write(profile_root.join("AGENTS.md"), "user global").expect("global prompt");
    fs::write(project_root.join("AGENTS.md"), "root agents").expect("root agents");
    fs::write(project_root.join("CLAUDE.md"), "root claude").expect("root claude");
    fs::write(project_root.join("src/AGENTS.md"), "nested agents").expect("nested agents");
    fs::write(project_root.join("src/AGENTS.local.md"), "local overlay").expect("local overlay");
    fs::write(project_root.join("node_modules/pkg/CLAUDE.md"), "ignored").expect("ignored prompt");
    insert_project(&store, &project_root);
    let profile_id = custom_profile(&store, PromptTool::DsHarness, &profile_root);

    let chain = resolve_prompt_instruction_chain(
        &store,
        &PromptScenario {
            profile_id: profile_id.clone(),
            project_id: Some("project-1".to_string()),
            working_directory: Some(project_root.join("src").to_string_lossy().to_string()),
            target_path: None,
        },
    )
    .expect("resolve chain");
    let paths = chain
        .startup_sources
        .iter()
        .map(|source| source.source.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 5);
    assert!(paths[0].ends_with("dsh-home/AGENTS.md"));
    assert!(paths[1].ends_with("project/AGENTS.md"));
    assert!(paths[2].ends_with("project/CLAUDE.md"));
    assert!(paths[3].ends_with("src/AGENTS.md"));
    assert!(paths[4].ends_with("src/AGENTS.local.md"));
    let kinds = chain
        .startup_sources
        .iter()
        .map(|source| source.source.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            PromptSourceKind::Primary,
            PromptSourceKind::Primary,
            PromptSourceKind::Primary,
            PromptSourceKind::Primary,
            PromptSourceKind::Local,
        ]
    );

    let sources =
        list_prompt_sources(&store, &profile_id, Some("project-1")).expect("list prompt sources");
    assert!(sources.iter().any(|source| {
        source.path.ends_with("src/AGENTS.local.md") && source.scope == PromptSourceScope::Local
    }));
    assert!(!sources
        .iter()
        .any(|source| source.path.contains("node_modules")));

    let global = sources
        .iter()
        .find(|source| source.path.ends_with("dsh-home/AGENTS.md"))
        .expect("user-global AGENTS.md is listed");
    assert_eq!(global.scope, PromptSourceScope::Global);
    assert_eq!(
        global.warning, None,
        "the user-global AGENTS.md is a normal global instruction file and must not carry a banner"
    );
}

#[test]
fn claude_chain_expands_imports_and_separates_path_rules() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("claude-home");
    let project_root = directory.path().join("project");
    fs::create_dir_all(profile_root.join("rules")).expect("user rules");
    fs::create_dir_all(project_root.join(".claude/rules")).expect("project rules");
    fs::create_dir_all(project_root.join("docs")).expect("docs");
    fs::create_dir_all(project_root.join("src/api")).expect("api");
    fs::write(profile_root.join("CLAUDE.md"), "user instructions").expect("user prompt");
    fs::write(
        project_root.join("CLAUDE.md"),
        "Project instructions @docs/workflow.md",
    )
    .expect("project prompt");
    fs::write(project_root.join("docs/workflow.md"), "workflow").expect("import");
    fs::write(
        project_root.join(".claude/rules/testing.md"),
        "# Testing\nRun tests.",
    )
    .expect("unconditional rule");
    fs::write(
        project_root.join(".claude/rules/api.md"),
        "---\npaths:\n  - \"src/api/**/*.ts\"\n---\n# API\nValidate input.",
    )
    .expect("conditional rule");
    fs::write(project_root.join("src/api/CLAUDE.md"), "nested api").expect("nested prompt");
    fs::write(project_root.join("src/api/handler.ts"), "export {};").expect("target");
    insert_project(&store, &project_root);
    let profile_id = custom_profile(&store, PromptTool::ClaudeCode, &profile_root);

    let sources =
        list_prompt_sources(&store, &profile_id, Some("project-1")).expect("list sources");
    assert!(sources.iter().any(|source| {
        source.kind == PromptSourceKind::Import && source.path.ends_with("docs/workflow.md")
    }));

    let chain = resolve_prompt_instruction_chain(
        &store,
        &PromptScenario {
            profile_id,
            project_id: Some("project-1".to_string()),
            working_directory: Some(project_root.to_string_lossy().to_string()),
            target_path: Some(
                project_root
                    .join("src/api/handler.ts")
                    .to_string_lossy()
                    .to_string(),
            ),
        },
    )
    .expect("resolve chain");
    assert!(chain.startup_sources.iter().any(|source| {
        source.source.kind == PromptSourceKind::Import
            && source.source.path.ends_with("docs/workflow.md")
    }));
    assert!(chain.startup_sources.iter().any(|source| {
        source.source.kind == PromptSourceKind::Rule
            && source.source.path.ends_with("rules/testing.md")
    }));
    assert!(chain.conditional_sources.iter().any(|source| {
        source.source.path.ends_with("rules/api.md") && source.matches_target == Some(true)
    }));
    assert!(chain
        .conditional_sources
        .iter()
        .any(|source| { source.source.path.ends_with("src/api/CLAUDE.md") }));
}

#[test]
fn invalid_utf8_prompt_is_read_only() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("codex-home");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::write(profile_root.join("AGENTS.md"), [0xff, 0xfe]).expect("invalid prompt");
    let profile_id = custom_profile(&store, PromptTool::Codex, &profile_root);
    let source = list_prompt_sources(&store, &profile_id, None)
        .expect("sources")
        .into_iter()
        .find(|source| source.path.ends_with("AGENTS.md"))
        .expect("source");

    let document = read_prompt_document(source).expect("document");
    assert!(document.read_only);
    assert_eq!(document.source.encoding, "unsupported");
}

#[test]
fn saves_with_conflict_detection_and_revision_history() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("codex-home");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::write(profile_root.join("AGENTS.md"), "before\n").expect("prompt");
    let profile_id = custom_profile(&store, PromptTool::Codex, &profile_root);
    let source = list_prompt_sources(&store, &profile_id, None)
        .expect("sources")
        .into_iter()
        .find(|source| source.path.ends_with("AGENTS.md"))
        .expect("source");
    let original_hash = source.content_hash.clone();

    let result = save_prompt_document(
        &store,
        &PromptSaveRequest {
            profile_id: profile_id.clone(),
            project_id: None,
            path: source.path.clone(),
            content: "after\n".to_string(),
            expected_hash: original_hash.clone(),
            line_ending: "lf".to_string(),
            has_bom: false,
            allow_external: false,
            force: false,
        },
    )
    .expect("save prompt");
    assert_eq!(
        fs::read_to_string(profile_root.join("AGENTS.md")).unwrap(),
        "after\n"
    );
    assert!(!result.created);
    let revisions = list_prompt_revision_summaries(&store, &source.path).expect("revisions");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].operation, "before_save");
    assert!(save_prompt_document(
        &store,
        &PromptSaveRequest {
            profile_id,
            project_id: None,
            path: source.path,
            content: "stale edit".to_string(),
            expected_hash: original_hash,
            line_ending: "lf".to_string(),
            has_bom: false,
            allow_external: false,
            force: false,
        },
    )
    .expect_err("stale save must conflict")
    .to_string()
    .contains("PROMPT_CONFLICT"));
}

#[test]
fn delete_is_recoverable_from_revision() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("codex-home");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::write(profile_root.join("AGENTS.md"), "recover me\n").expect("prompt");
    let profile_id = custom_profile(&store, PromptTool::Codex, &profile_root);
    let source = list_prompt_sources(&store, &profile_id, None)
        .expect("sources")
        .into_iter()
        .find(|source| source.path.ends_with("AGENTS.md"))
        .expect("source");
    let deleted = delete_prompt_document(
        &store,
        &PromptDeleteRequest {
            profile_id,
            project_id: None,
            path: source.path.clone(),
            expected_hash: source.content_hash,
            allow_external: false,
            force: false,
        },
    )
    .expect("delete prompt");
    assert!(!profile_root.join("AGENTS.md").exists());

    restore_prompt_revision(
        &store,
        &PromptRestoreRequest {
            revision_id: deleted.revision_id,
            expected_hash: None,
            allow_external: false,
            force: false,
        },
    )
    .expect("restore prompt");
    assert_eq!(
        fs::read_to_string(profile_root.join("AGENTS.md")).unwrap(),
        "recover me\n"
    );
}

#[cfg(unix)]
#[test]
fn saving_symlink_preserves_link_and_updates_target() {
    use std::os::unix::fs::symlink;

    let (directory, store) = test_store();
    let profile_root = directory.path().join("codex-home");
    let dotfiles_root = directory.path().join("dotfiles");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::create_dir_all(&dotfiles_root).expect("dotfiles");
    let target = dotfiles_root.join("AGENTS.md");
    fs::write(&target, "old").expect("target");
    symlink(&target, profile_root.join("AGENTS.md")).expect("symlink");
    let profile_id = custom_profile(&store, PromptTool::Codex, &profile_root);
    let source = list_prompt_sources(&store, &profile_id, None)
        .expect("sources")
        .into_iter()
        .find(|source| source.path.ends_with("AGENTS.md"))
        .expect("source");
    assert!(!source.can_delete);

    save_prompt_document(
        &store,
        &PromptSaveRequest {
            profile_id,
            project_id: None,
            path: source.path.clone(),
            content: "new".to_string(),
            expected_hash: source.content_hash,
            line_ending: "lf".to_string(),
            has_bom: false,
            allow_external: true,
            force: false,
        },
    )
    .expect("save symlink target");
    assert!(fs::symlink_metadata(&source.path)
        .expect("link metadata")
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "new");
}

#[test]
fn pi_discovers_system_files_and_resolves_context_chain() {
    let (directory, store) = test_store();
    let profile_root = directory.path().join("pi-home");
    let project_root = directory.path().join("project");
    fs::create_dir_all(project_root.join("service")).expect("service");
    fs::create_dir_all(project_root.join(".pi")).expect("project pi");
    fs::create_dir_all(&profile_root).expect("profile");
    fs::write(profile_root.join("AGENTS.md"), "global agents").expect("global agents");
    fs::write(profile_root.join("SYSTEM.md"), "replace system").expect("system");
    fs::write(profile_root.join("APPEND_SYSTEM.md"), "append system").expect("append");
    fs::write(project_root.join("AGENTS.md"), "project agents").expect("project agents");
    fs::write(
        project_root.join("service/AGENTS.override.md"),
        "service override",
    )
    .expect("service override");
    fs::write(project_root.join("CLAUDE.md"), "unused when agents exists").expect("claude");
    fs::write(project_root.join(".pi/SYSTEM.md"), "project system").expect("project system");
    insert_project(&store, &project_root);
    let profile_id = custom_profile(&store, PromptTool::Pi, &profile_root);

    let sources =
        list_prompt_sources(&store, &profile_id, Some("project-1")).expect("list sources");
    assert!(sources
        .iter()
        .any(|source| source.path.ends_with("pi-home/SYSTEM.md") && source.exists));
    assert!(sources
        .iter()
        .any(|source| source.path.ends_with("pi-home/APPEND_SYSTEM.md") && source.exists));
    assert!(sources
        .iter()
        .any(|source| source.path.ends_with("project/.pi/SYSTEM.md") && source.exists));
    assert!(sources.iter().any(|source| {
        source.path.ends_with("service/AGENTS.override.md")
            && source.kind == PromptSourceKind::Override
    }));

    let chain = resolve_prompt_instruction_chain(
        &store,
        &PromptScenario {
            profile_id,
            project_id: Some("project-1".to_string()),
            working_directory: Some(project_root.join("service").to_string_lossy().to_string()),
            target_path: None,
        },
    )
    .expect("resolve chain");
    let paths = chain
        .startup_sources
        .iter()
        .map(|source| source.source.path.as_str())
        .collect::<Vec<_>>();
    assert!(paths[0].ends_with("pi-home/SYSTEM.md"));
    assert!(paths[1].ends_with("pi-home/APPEND_SYSTEM.md"));
    assert!(paths[2].ends_with("pi-home/AGENTS.md"));
    assert!(paths.iter().any(|path| path.ends_with("project/AGENTS.md")));
    assert!(paths
        .iter()
        .any(|path| path.ends_with("service/AGENTS.override.md")));
    assert!(paths
        .iter()
        .any(|path| path.ends_with("project/.pi/SYSTEM.md")));
    assert!(!paths.iter().any(|path| path.ends_with("project/CLAUDE.md")));
}
