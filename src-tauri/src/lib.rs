mod commands;
mod core;
mod services;

use std::sync::Arc;

use core::cancel_token::CancelToken;
use core::runtime_paths::{RuntimePaths, RuntimeProfile};
use core::skill_store::{default_db_path, migrate_legacy_db_if_needed, SkillStore};
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

pub(crate) fn runtime_paths_for_tauri<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> anyhow::Result<RuntimePaths> {
    if let Some(paths) = app.try_state::<RuntimePaths>() {
        return Ok(paths.inner().clone());
    }
    let app_data_dir = app.path().app_data_dir()?;
    let home_root = dirs::home_dir().unwrap_or_else(|| app_data_dir.clone());
    let cache_dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| app_data_dir.clone());
    Ok(RuntimePaths::from_tauri(
        RuntimeProfile::current(),
        home_root,
        app_data_dir,
        cache_dir,
    ))
}

fn init_store<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<SkillStore> {
    let db_path = default_db_path(app)?;
    migrate_legacy_db_if_needed(&db_path)?;
    let store = SkillStore::new(db_path);
    store.ensure_schema()?;
    store.migrate_device_sync_startup_credential_consent()?;
    Ok(store)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .targets([
                        Target::new(TargetKind::LogDir { file_name: None }),
                        #[cfg(desktop)]
                        Target::new(TargetKind::Stdout),
                    ])
                    .build(),
            )?;

            let is_background_update = std::env::args()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|pair| pair[0] == "--background-task" && pair[1] == "update-skills");
            let force_background_update = std::env::args().any(|arg| arg == "--force");

            let store = init_store(app.handle()).map_err(tauri::Error::from)?;

            if is_background_update {
                #[cfg(target_os = "macos")]
                {
                    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
                let run_result = if force_background_update {
                    core::auto_update::run_auto_update_now(app.handle(), &store).map(Some)
                } else {
                    core::auto_update::run_due_auto_update(app.handle(), &store)
                };
                match run_result {
                    Ok(Some(result)) => {
                        log::info!(
                            "auto update finished: checked={}, updated={}, failed={}",
                            result.checked,
                            result.updated,
                            result.failed
                        );
                        app.handle().exit(if result.failed == 0 { 0 } else { 2 });
                    }
                    Ok(None) => {
                        app.handle().exit(0);
                    }
                    Err(err) => {
                        eprintln!("auto update failed: {err:#}");
                        app.handle().exit(1);
                    }
                }
                return Ok(());
            }

            let paths = runtime_paths_for_tauri(app.handle()).map_err(tauri::Error::from)?;
            app.manage(paths.clone());
            app.manage(store.clone());
            app.manage(Arc::new(CancelToken::new()));

            let repair_store = store.clone();
            let repair_paths = paths.clone();
            if let Err(err) = (|| -> anyhow::Result<()> {
                let workspace = repair_paths.app_data_dir.join("device-sync");
                let central = core::central_repo::resolve_central_repo_from_runtime(
                    &repair_paths,
                    &repair_store,
                )?;
                let credentials = core::device_sync::credentials::SystemCredentialStore;
                core::device_sync::DeviceSyncService::new(
                    &repair_store,
                    &credentials,
                    workspace,
                    central,
                )
                .repair_legacy_same_device_conflicts()?;
                Ok(())
            })() {
                log::warn!("same-device sync baseline recovery skipped: {err:#}");
            }
            core::device_sync::scheduler::start(app.handle().clone(), store.clone());

            // Backfill description for skills that were installed before V2 schema.
            core::installer::backfill_skill_descriptions(&store);
            let backfilled_hashes = core::installer::backfill_skill_content_hashes(&store);
            if backfilled_hashes > 0 {
                log::info!("backfilled content hashes for {backfilled_hashes} skills");
            }

            // Best-effort cleanup of our own old git temp directories.
            // Safety:
            // - Only deletes directories that match prefix `skills-hub-git-*`
            // - And contain our marker file `.skills-hub-git-temp`
            // - And are older than the max age.
            let handle = app.handle().clone();
            let store_for_cleanup = store.clone();
            let recycle_dir = paths.recycle_bin_dir.clone();
            tauri::async_runtime::spawn(async move {
                let removed = core::temp_cleanup::cleanup_old_git_temp_dirs(
                    &handle,
                    std::time::Duration::from_secs(24 * 60 * 60),
                )
                .unwrap_or(0);
                if removed > 0 {
                    log::info!("cleaned up {} old git temp dirs", removed);
                }

                let cleanup_days =
                    core::cache_cleanup::get_git_cache_cleanup_days(&store_for_cleanup);
                if cleanup_days > 0 {
                    let max_age =
                        std::time::Duration::from_secs(cleanup_days as u64 * 24 * 60 * 60);
                    let removed =
                        core::cache_cleanup::cleanup_git_cache_dirs(&handle, max_age).unwrap_or(0);
                    if removed > 0 {
                        log::info!("cleaned up {} git cache dirs", removed);
                    }
                }

                match core::recycle_bin::RecycleBinService::new(&store_for_cleanup, recycle_dir)
                    .cleanup_expired(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64,
                    ) {
                    Ok(removed) if removed > 0 => {
                        log::info!("cleaned up {removed} expired recycle bin items");
                    }
                    Ok(_) => {}
                    Err(error) => log::warn!("recycle bin cleanup failed: {error:#}"),
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_central_repo_path,
            commands::set_central_repo_path,
            commands::get_recent_projects,
            commands::save_recent_project,
            commands::list_projects,
            commands::import_project,
            commands::scan_project,
            commands::update_project_settings,
            commands::inject_project_skill,
            commands::remove_project_skill,
            commands::repair_project_issue,
            commands::ignore_project_issue,
            commands::ignore_all_project_issues,
            commands::collect_project_skill,
            commands::get_tool_config,
            commands::set_tool_config,
            commands::get_tool_status,
            commands::get_git_cache_cleanup_days,
            commands::get_git_cache_ttl_secs,
            commands::set_git_cache_cleanup_days,
            commands::set_git_cache_ttl_secs,
            commands::clear_git_cache_now,
            commands::get_auto_update_config,
            commands::set_auto_update_config,
            commands::run_auto_update_now,
            commands::retry_auto_update_skills,
            commands::trigger_auto_update_task_now_cmd,
            commands::get_onboarding_plan,
            commands::install_local,
            commands::list_local_skills_cmd,
            commands::install_local_selection,
            commands::install_git,
            commands::list_git_skills_cmd,
            commands::install_git_selection,
            commands::reimport_local_selection,
            commands::reimport_git_selection,
            commands::sync_skill_dir,
            commands::preview_skill_assignments,
            commands::apply_skill_assignments,
            commands::resume_skill_assignments,
            commands::ignore_assignment_issue,
            commands::ignore_all_assignment_issues,
            commands::set_skill_enabled,
            commands::update_managed_skill,
            commands::search_github,
            commands::get_github_token,
            commands::set_github_token,
            commands::get_github_proxy_config,
            commands::set_github_proxy_config,
            commands::get_github_proxy_url,
            commands::set_github_proxy_url,
            commands::import_existing_skill,
            commands::get_managed_skills,
            commands::get_tags,
            commands::create_tag,
            commands::rename_tag,
            commands::delete_tag,
            commands::get_skill_tags,
            commands::set_skill_tags,
            commands::get_untagged_skill_ids,
            commands::delete_managed_skill,
            commands::get_featured_skills,
            commands::search_skills_online,
            commands::list_skill_files,
            commands::read_skill_file,
            commands::list_skill_preview,
            commands::read_skill_preview_file,
            commands::cancel_current_operation,
            commands::get_device_sync_config,
            commands::save_device_sync_config,
            commands::get_device_sync_oauth_availability,
            commands::get_device_sync_pending_oauth,
            commands::start_device_sync_oauth,
            commands::poll_device_sync_oauth,
            commands::clear_device_sync_pending_oauth,
            commands::cancel_device_sync_oauth,
            commands::validate_device_sync_account,
            commands::create_device_sync_repository,
            commands::list_device_sync_repositories,
            commands::get_device_sync_status,
            commands::check_device_sync,
            commands::run_device_sync,
            commands::get_device_sync_history,
            commands::get_device_sync_devices,
            commands::set_device_sync_device_alias,
            commands::get_device_sync_conflicts,
            commands::get_device_sync_trash,
            commands::get_recycle_bin_locations,
            commands::get_recycle_bin_items,
            commands::restore_recycle_bin_item,
            commands::delete_recycle_bin_item,
            commands::clear_recycle_bin,
            commands::resolve_device_sync_conflict,
            commands::restore_device_sync_trash,
            commands::disconnect_device_sync
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, _event| {});
}
