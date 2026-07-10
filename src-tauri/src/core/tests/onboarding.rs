use std::fs;

use crate::core::skill_store::SkillRecord;

use super::{build_onboarding_plan_in_home, build_onboarding_plan_with_managed_skills_in_home};

fn managed_skill(name: &str, central_path: &std::path::Path) -> SkillRecord {
    SkillRecord {
        id: format!("managed-{name}"),
        name: name.to_string(),
        description: None,
        source_type: "local".to_string(),
        source_ref: None,
        source_subpath: None,
        source_revision: None,
        central_path: central_path.to_string_lossy().to_string(),
        content_hash: None,
        created_at: 0,
        updated_at: 0,
        last_sync_at: None,
        last_seen_at: 0,
        enabled: true,
        status: "ok".to_string(),
    }
}

#[test]
fn groups_by_name_and_detects_conflicts_by_fingerprint() {
    let home = tempfile::tempdir().unwrap();

    // Cursor installed
    fs::create_dir_all(home.path().join(".cursor")).unwrap();
    fs::create_dir_all(home.path().join(".cursor/skills/foo")).unwrap();
    fs::write(home.path().join(".cursor/skills/foo/a.txt"), b"cursor").unwrap();

    // Codex installed
    fs::create_dir_all(home.path().join(".codex")).unwrap();
    fs::create_dir_all(home.path().join(".codex/skills/foo")).unwrap();
    fs::write(home.path().join(".codex/skills/foo/a.txt"), b"codex").unwrap();

    // Codex .system should be ignored
    fs::create_dir_all(home.path().join(".codex/skills/.system")).unwrap();
    fs::write(home.path().join(".codex/skills/.system/SKILL.md"), b"x").unwrap();

    let plan = build_onboarding_plan_in_home(home.path(), None, None).unwrap();
    assert_eq!(plan.total_tools_scanned, 2);
    assert_eq!(plan.total_skills_found, 2);
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].name, "foo");
    assert!(plan.groups[0].has_conflict, "同名但内容不同应冲突");
    assert_eq!(plan.groups[0].variants.len(), 2);
}

#[test]
fn marks_discovered_variant_that_matches_managed_skill() {
    let home = tempfile::tempdir().unwrap();
    let central = home.path().join("central/web-deploy");
    let discovered = home.path().join(".codex/skills/web-deploy");
    fs::create_dir_all(&central).unwrap();
    fs::create_dir_all(&discovered).unwrap();
    fs::write(central.join("SKILL.md"), b"# Web deploy").unwrap();
    fs::write(discovered.join("SKILL.md"), b"# Web deploy").unwrap();
    fs::write(discovered.join("_skillhub_meta.json"), b"{}").unwrap();

    let plan = build_onboarding_plan_with_managed_skills_in_home(
        home.path(),
        Some(home.path().join("central").as_path()),
        None,
        &[managed_skill("web-deploy", &central)],
    )
    .unwrap();

    let group = plan
        .groups
        .iter()
        .find(|group| group.name == "web-deploy")
        .unwrap();
    assert_eq!(group.hub_skill.as_ref().unwrap().id, "managed-web-deploy");
    assert_eq!(group.variants[0].hub_relation.as_deref(), Some("same"));
}

#[test]
fn marks_discovered_variant_that_differs_from_managed_skill() {
    let home = tempfile::tempdir().unwrap();
    let central = home.path().join("central/demo");
    let discovered = home.path().join(".cursor/skills/demo");
    fs::create_dir_all(&central).unwrap();
    fs::create_dir_all(&discovered).unwrap();
    fs::write(central.join("SKILL.md"), b"# Hub version").unwrap();
    fs::write(discovered.join("SKILL.md"), b"# Discovered version").unwrap();

    let plan = build_onboarding_plan_with_managed_skills_in_home(
        home.path(),
        Some(home.path().join("central").as_path()),
        None,
        &[managed_skill("demo", &central)],
    )
    .unwrap();

    let group = plan
        .groups
        .iter()
        .find(|group| group.name == "demo")
        .unwrap();
    assert_eq!(group.variants[0].hub_relation.as_deref(), Some("different"));
}

#[test]
#[cfg(unix)]
fn excludes_central_repo_path() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();

    // Cursor installed
    std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
    std::fs::create_dir_all(home.path().join(".cursor/skills")).unwrap();

    let central = home.path().join("central");
    std::fs::create_dir_all(central.join("skill-a")).unwrap();

    let link_path = home.path().join(".cursor/skills/skill-a");
    symlink(central.join("skill-a"), &link_path).unwrap();

    let plan = build_onboarding_plan_in_home(home.path(), Some(&central), None).unwrap();
    assert_eq!(plan.total_skills_found, 0);
}

#[test]
fn excludes_managed_skill_targets() {
    let home = tempfile::tempdir().unwrap();

    // Cursor installed
    fs::create_dir_all(home.path().join(".cursor")).unwrap();
    fs::create_dir_all(home.path().join(".cursor/skills/foo")).unwrap();
    fs::write(home.path().join(".cursor/skills/foo/a.txt"), b"cursor").unwrap();

    let mut exclude = std::collections::HashSet::new();
    exclude.insert(super::managed_target_key(
        "cursor",
        &home.path().join(".cursor/skills/foo"),
    ));

    let plan = build_onboarding_plan_in_home(home.path(), None, Some(&exclude)).unwrap();
    assert_eq!(plan.total_skills_found, 0);
}

#[test]
fn discovers_user_scoped_claude_plugin_skills() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    let plugins_dir = claude_dir.join("plugins");
    let standard_plugin = plugins_dir.join("cache/official/standard/1.0.0");
    let custom_plugin = plugins_dir.join("cache/community/custom/2.0.0");
    let root_plugin = plugins_dir.join("cache/community/root/3.0.0");
    let project_plugin = plugins_dir.join("cache/official/project-only/1.0.0");

    fs::create_dir_all(standard_plugin.join("skills/standard-skill")).unwrap();
    fs::write(
        standard_plugin.join("skills/standard-skill/SKILL.md"),
        "# Standard",
    )
    .unwrap();

    fs::create_dir_all(custom_plugin.join("custom/custom-skill")).unwrap();
    fs::write(
        custom_plugin.join("custom/custom-skill/SKILL.md"),
        "# Custom",
    )
    .unwrap();
    fs::create_dir_all(custom_plugin.join(".claude-plugin")).unwrap();
    fs::write(
        custom_plugin.join(".claude-plugin/plugin.json"),
        r#"{"skills":["./custom/custom-skill"]}"#,
    )
    .unwrap();

    fs::create_dir_all(&root_plugin).unwrap();
    fs::write(root_plugin.join("SKILL.md"), "# Root").unwrap();

    fs::create_dir_all(project_plugin.join("skills/project-skill")).unwrap();
    fs::write(
        project_plugin.join("skills/project-skill/SKILL.md"),
        "# Project",
    )
    .unwrap();

    fs::create_dir_all(&plugins_dir).unwrap();
    fs::write(
        plugins_dir.join("installed_plugins.json"),
        format!(
            r#"{{
              "version": 2,
              "plugins": {{
                "standard@official": [{{
                  "scope": "user",
                  "installPath": "{}",
                  "version": "1.0.0"
                }}],
                "custom@community": [{{
                  "scope": "user",
                  "installPath": "{}",
                  "version": "2.0.0"
                }}],
                "root@community": [{{
                  "scope": "user",
                  "installPath": "{}",
                  "version": "3.0.0"
                }}],
                "broken@community": [{{
                  "scope": "user",
                  "version": "1.0.0"
                }}],
                "project-only@official": [{{
                  "scope": "project",
                  "projectPath": "/tmp/project",
                  "installPath": "{}",
                  "version": "1.0.0"
                }}]
              }}
            }}"#,
            standard_plugin.display(),
            custom_plugin.display(),
            root_plugin.display(),
            project_plugin.display()
        ),
    )
    .unwrap();

    let plan = build_onboarding_plan_in_home(home.path(), None, None).unwrap();
    assert_eq!(plan.total_tools_scanned, 1);
    assert_eq!(plan.total_skills_found, 3);

    let standard = plan
        .groups
        .iter()
        .find(|group| group.name == "standard-skill")
        .unwrap();
    assert_eq!(
        standard.variants[0].plugin_name.as_deref(),
        Some("standard@official")
    );
    assert_eq!(
        standard.variants[0].plugin_version.as_deref(),
        Some("1.0.0")
    );
    assert_eq!(standard.variants[0].plugin_scope.as_deref(), Some("user"));

    let custom = plan
        .groups
        .iter()
        .find(|group| group.name == "custom-skill")
        .unwrap();
    assert_eq!(
        custom.variants[0].plugin_name.as_deref(),
        Some("custom@community")
    );
    assert_eq!(
        custom.variants[0].path,
        fs::canonicalize(custom_plugin.join("custom/custom-skill")).unwrap()
    );
    let root = plan
        .groups
        .iter()
        .find(|group| group.name == "root")
        .unwrap();
    assert_eq!(
        root.variants[0].plugin_name.as_deref(),
        Some("root@community")
    );
    assert!(plan
        .groups
        .iter()
        .all(|group| group.name != "project-skill"));
}

#[test]
fn ignores_duplicate_and_escaping_claude_plugin_skill_paths() {
    let home = tempfile::tempdir().unwrap();
    let plugins_dir = home.path().join(".claude/plugins");
    let plugin = plugins_dir.join("cache/community/demo/1.0.0");
    let outside = home.path().join("outside-skill");

    fs::create_dir_all(plugin.join("skills/demo")).unwrap();
    fs::write(plugin.join("skills/demo/SKILL.md"), "# Demo").unwrap();
    fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("SKILL.md"), "# Outside").unwrap();
    fs::write(
        plugin.join(".claude-plugin/plugin.json"),
        r#"{"skills":["./skills/demo","../../../../../outside-skill"]}"#,
    )
    .unwrap();

    fs::create_dir_all(&plugins_dir).unwrap();
    fs::write(
        plugins_dir.join("installed_plugins.json"),
        format!(
            r#"{{
              "version": 2,
              "plugins": {{
                "demo@community": [{{
                  "scope": "user",
                  "installPath": "{}",
                  "version": "1.0.0"
                }}]
              }}
            }}"#,
            plugin.display()
        ),
    )
    .unwrap();

    let plan = build_onboarding_plan_in_home(home.path(), None, None).unwrap();
    assert_eq!(plan.total_skills_found, 1);
    assert_eq!(plan.groups[0].name, "demo");
    assert_eq!(plan.groups[0].variants.len(), 1);
}

#[test]
#[cfg(unix)]
fn deduplicates_plugin_skill_already_present_in_claude_skills() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let plugins_dir = home.path().join(".claude/plugins");
    let plugin = plugins_dir.join("cache/community/demo/1.0.0");
    let plugin_skill = plugin.join("skills/demo");

    fs::create_dir_all(&plugin_skill).unwrap();
    fs::write(plugin_skill.join("SKILL.md"), "# Demo").unwrap();
    fs::create_dir_all(home.path().join(".claude/skills")).unwrap();
    symlink(&plugin_skill, home.path().join(".claude/skills/demo")).unwrap();

    fs::create_dir_all(&plugins_dir).unwrap();
    fs::write(
        plugins_dir.join("installed_plugins.json"),
        format!(
            r#"{{
              "version": 2,
              "plugins": {{
                "demo@community": [{{
                  "scope": "user",
                  "installPath": "{}",
                  "version": "1.0.0"
                }}]
              }}
            }}"#,
            plugin.display()
        ),
    )
    .unwrap();

    let plan = build_onboarding_plan_in_home(home.path(), None, None).unwrap();
    assert_eq!(plan.total_skills_found, 1);
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].variants.len(), 1);
    assert_eq!(plan.groups[0].variants[0].tool, "claude_code");
}

#[test]
fn excludes_already_imported_claude_plugin_skill_source() {
    let home = tempfile::tempdir().unwrap();
    let plugins_dir = home.path().join(".claude/plugins");
    let plugin = plugins_dir.join("cache/community/demo/1.0.0");
    let plugin_skill = plugin.join("skills/demo");

    fs::create_dir_all(&plugin_skill).unwrap();
    fs::write(plugin_skill.join("SKILL.md"), "# Demo").unwrap();
    fs::create_dir_all(&plugins_dir).unwrap();
    fs::write(
        plugins_dir.join("installed_plugins.json"),
        format!(
            r#"{{
              "version": 2,
              "plugins": {{
                "demo@community": [{{
                  "scope": "user",
                  "installPath": "{}",
                  "version": "1.0.0"
                }}]
              }}
            }}"#,
            plugin.display()
        ),
    )
    .unwrap();

    let canonical_skill = fs::canonicalize(plugin_skill).unwrap();
    let mut exclude = std::collections::HashSet::new();
    exclude.insert(super::managed_target_key(
        super::CLAUDE_PLUGIN_TOOL_KEY,
        &canonical_skill,
    ));

    let plan = build_onboarding_plan_in_home(home.path(), None, Some(&exclude)).unwrap();
    assert_eq!(plan.total_skills_found, 0);
    assert!(plan.groups.is_empty());
}
