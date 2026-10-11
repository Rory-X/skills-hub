use std::path::{Path, PathBuf};

use anyhow::Result;

use super::skill_store::{SkillStore, SkillTargetRecord};
use super::sync_engine::sync_dir_copy_with_overwrite;
use super::tool_adapters::{adapter_by_key, is_tool_installed};

pub fn resync_managed_copy_targets(
    store: &SkillStore,
    skill_id: &str,
    central_path: &Path,
    synced_at: i64,
) -> Result<Vec<String>> {
    let targets = store.list_skill_targets(skill_id)?;
    let mut updated_targets = Vec::new();
    for target in targets {
        if target.status == "disabled" {
            continue;
        }
        if target.scope == "global" {
            if let Some(adapter) = adapter_by_key(&target.tool) {
                if !is_tool_installed(&adapter)? {
                    continue;
                }
            }
        }
        if target.mode != "copy" && target.tool != "cursor" {
            continue;
        }

        let target_path = PathBuf::from(&target.target_path);
        let sync_result = sync_dir_copy_with_overwrite(central_path, &target_path, true)?;
        store.upsert_skill_target(&SkillTargetRecord {
            id: target.id,
            skill_id: target.skill_id,
            tool: target.tool.clone(),
            scope: target.scope,
            project_path: target.project_path,
            target_path: sync_result.target_path.to_string_lossy().to_string(),
            mode: "copy".to_string(),
            status: "ok".to_string(),
            last_error: None,
            synced_at: Some(synced_at),
        })?;
        updated_targets.push(target.tool);
    }
    Ok(updated_targets)
}
