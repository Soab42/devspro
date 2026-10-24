mod auth;
mod db;
mod elevate;
mod modules;
mod tray;

use modules::ai;
use modules::apps;
use modules::backup;
use modules::browser;
use modules::cleaner;
use modules::databases;
use modules::docker;
use modules::env_diff;
use modules::file_manager;
use modules::hosts_file;
use modules::http_client;
use modules::laravel_log;
use modules::logs;
use modules::mailcatcher;
use modules::memory;
use modules::nginx_ssl::{ca, laravel, nginx, php_fpm, ports, sites};
use modules::processes;
use modules::redis;
use modules::remote_desktop;
use modules::scaffold;
use modules::scheduled;
use modules::settings;
use modules::ssh;
use modules::system;
use modules::tunnel;
use modules::workers;
use tauri::Manager;

#[tauri::command(async)]
fn elevate_ping(app: tauri::AppHandle) -> Result<String, String> {
    elevate::ping(&app)
}

/// WebKitGTK 2.42+ renders through DMA-BUF by default, and on the proprietary
/// NVIDIA driver that path is the well-known cause of stuttering and blank
/// webviews (the Tauri Linux troubleshooting guide's first suggestion). The
/// app's own UI and every Browser tab are WebKit views, so switch to the
/// older renderer when that driver is loaded. An explicit setting in the
/// environment always wins, so `WEBKIT_DISABLE_DMABUF_RENDERER=0` restores
/// the default for anyone who wants to compare.
#[cfg(target_os = "linux")]
fn tune_webkit_for_gpu() {
    const VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    if std::env::var_os(VAR).is_some() {
        return;
    }
    let nvidia_bound = std::fs::read_dir("/sys/bus/pci/drivers/nvidia")
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("0000:"))
        })
        .unwrap_or(false);
    if nvidia_bound {
        std::env::set_var(VAR, "1");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    tune_webkit_for_gpu();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // The File Manager's image, audio, video and PDF tabs load from here.
        .register_asynchronous_uri_scheme_protocol(file_manager::media::SCHEME, |_ctx, request, responder| {
            file_manager::media::respond(request, responder)
        })
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let conn = db::open(&app_data_dir).expect("failed to open/migrate app database");
            app.manage(db::Db(std::sync::Mutex::new(conn)));
            app.manage(system::SystemState::default());
            app.manage(tunnel::TunnelState::default());
            app.manage(workers::WorkersState::default());
            app.manage(ssh::SshState::default());
            app.manage(remote_desktop::RemoteDesktopState::default());
            app.manage(memory::MemoryState::default());
            app.manage(cleaner::CleanerState::default());
            // After the Db and MemoryState it reads are managed. Runs for the
            // life of the process, window open or not.
            memory::watchdog::start(app.handle().clone());

            tray::setup(app.handle())?;

            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if tray::close_to_tray_enabled(&app_handle) {
                            api.prevent_close();
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.hide();
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            elevate_ping,
            settings::settings_get,
            settings::settings_set,
            backup::backup_export,
            backup::backup_inspect,
            backup::backup_import,
            tray::autostart_set_enabled,
            tray::autostart_is_enabled,
            auth::auth_has_profile,
            auth::auth_list_profiles,
            auth::auth_create_profile,
            auth::auth_unlock,
            auth::auth_current_session,
            auth::auth_logout,
            nginx::nginx_ssl_detect_nginx,
            nginx::nginx_ssl_install_nginx,
            ca::nginx_ssl_ca_status,
            ca::nginx_ssl_ca_ensure,
            ca::nginx_ssl_ca_install,
            ports::nginx_ssl_suggest_port,
            ports::nginx_ssl_check_port,
            php_fpm::nginx_ssl_detect_php_fpm,
            php_fpm::nginx_ssl_list_php_versions,
            laravel::nginx_ssl_validate_laravel_project,
            sites::nginx_ssl_list_sites,
            sites::nginx_ssl_hostname_conflict,
            sites::nginx_ssl_add_site,
            sites::nginx_ssl_set_site_enabled,
            sites::nginx_ssl_set_site_php_version,
            sites::nginx_ssl_set_site_limits,
            sites::nginx_ssl_fix_laravel_permissions,
            sites::nginx_ssl_reissue_site_cert,
            sites::nginx_ssl_remove_site,
            scaffold::scaffold_create_laravel_project,
            scaffold::wizard::scaffold_wizard_run,
            docker::docker_detect,
            docker::docker_list_containers,
            docker::docker_start,
            docker::docker_stop,
            docker::docker_restart,
            docker::docker_logs,
            docker::docker_change_port,
            docker::docker_bulk_action,
            docker::docker_compose_project_action,
            docker::docker_compose_up_file,
            docker::docker_compose_validate,
            docker::docker_read_container_compose,
            docker::docker_read_project_compose,
            docker::docker_write_compose,
            env_diff::env_diff_read_file,
            env_diff::env_diff_write_file,
            file_manager::fm_places,
            file_manager::fm_list,
            file_manager::fm_stat,
            file_manager::fm_stat_detail,
            file_manager::fm_mkdir,
            file_manager::fm_create_file,
            file_manager::fm_rename,
            file_manager::fm_move,
            file_manager::fm_copy,
            file_manager::fm_dir_stats,
            file_manager::fm_editors,
            file_manager::fm_open_terminal,
            file_manager::fm_thumbnail,
            file_manager::fm_trash,
            file_manager::fm_read_file,
            file_manager::fm_write_file,
            file_manager::fm_open,
            file_manager::fm_open_in_editor,
            file_manager::fm_display_names,
            file_manager::devices::fm_devices,
            file_manager::devices::fm_device_action,
            file_manager::media::fm_media_token,
            file_manager::git::fm_git_status,
            file_manager::git::fm_git_base,
            file_manager::git::fm_git_blame,
            file_manager::git::fm_git_log,
            file_manager::git::fm_git_show,
            file_manager::format::fm_format,
            browser::browser_open,
            browser::browser_navigate,
            browser::browser_back,
            browser::browser_forward,
            browser::browser_reload,
            browser::browser_stop,
            browser::browser_set_bounds,
            browser::browser_set_visible,
            browser::browser_set_zoom,
            browser::browser_close,
            browser::browser_list,
            browser::browser_focus,
            browser::browser_focus_parent,
            browser::browser_devtools,
            browser::browser_print,
            browser::browser_find,
            browser::browser_clear_data,
            browser::browser_bookmarks_list,
            browser::browser_bookmark_add,
            browser::browser_bookmark_update,
            browser::browser_bookmark_delete,
            browser::browser_bookmarks_reorder,
            browser::browser_history_search,
            browser::browser_history_delete,
            browser::browser_history_clear,
            browser::browser_system_browsers,
            browser::browser_open_in,
            browser::credentials::browser_credentials_list,
            browser::credentials::browser_credentials_for,
            browser::credentials::browser_credential_save,
            browser::credentials::browser_credential_delete,
            browser::credentials::browser_credential_reveal,
            browser::credentials::browser_credential_fill,
            hosts_file::hosts_file_read,
            hosts_file::hosts_file_add,
            hosts_file::hosts_file_remove,
            hosts_file::hosts_file_update,
            hosts_file::hosts_file_missing_sites,
            http_client::http_send,
            http_client::http_history_list,
            http_client::http_history_save,
            http_client::http_history_delete,
            http_client::http_history_clear,
            logs::logs_list_sources,
            logs::logs_tail,
            mailcatcher::mailcatcher_detect_docker,
            mailcatcher::mailcatcher_status,
            mailcatcher::mailcatcher_start,
            mailcatcher::mailcatcher_stop,
            mailcatcher::mailcatcher_remove,
            cleaner::cleaner_settings_get,
            cleaner::cleaner_settings_set,
            cleaner::cleaner_history_list,
            cleaner::cleaner_history_clear,
            cleaner::cleaner_cancel_scan,
            cleaner::snapshot::cleaner_snapshot_get,
            cleaner::snapshot::cleaner_snapshot_set,
            cleaner::snapshot::cleaner_snapshot_clear,
            cleaner::caches::cleaner_scan_caches,
            cleaner::caches::cleaner_clean_caches,
            cleaner::system_items::cleaner_scan_system,
            cleaner::system_items::cleaner_clean_system,
            cleaner::docker::cleaner_scan_docker,
            cleaner::docker::cleaner_clean_docker,
            cleaner::large_files::cleaner_scan_large_files,
            cleaner::large_files::cleaner_folder_sizes,
            cleaner::large_files::cleaner_remove_files,
            cleaner::projects::cleaner_scan_projects,
            cleaner::projects::cleaner_clean_projects,
            cleaner::storage_map::cleaner_scan_tree,
            memory::memory_snapshot,
            memory::memory_stop,
            memory::memory_start,
            memory::memory_apps_get,
            memory::memory_apps_set,
            memory::memory_watchdog_get,
            memory::memory_watchdog_set,
            memory::memory_watchdog_log,
            memory::memory_drop_caches,
            memory::memory_reset_preview,
            memory::memory_reset,
            processes::processes_list_ports,
            processes::processes_kill,
            redis::redis_ping,
            redis::redis_scan,
            redis::redis_get,
            redis::redis_delete,
            redis::redis_flush_db,
            redis::redis_laravel_queues,
            redis::redis_peek_job,
            ai::ai_agents_list,
            ai::ai_custom_agent_save,
            ai::ai_custom_agent_delete,
            apps::apps_list,
            apps::apps_install_batch,
            apps::apps_list_nvm_versions,
            apps::apps_write_nvmrc,
            databases::databases_create,
            databases::databases_list,
            databases::databases_get_credentials,
            databases::databases_remove,
            databases::databases_list_users,
            databases::databases_create_user,
            databases::databases_drop_user,
            databases::databases_change_admin_password,
            databases::databases_change_user_password,
            databases::databases_backup,
            databases::databases_restore,
            databases::databases_create_database,
            databases::browse::databases_list_databases,
            databases::browse::databases_list_tables,
            databases::browse::databases_list_schemas,
            databases::browse::databases_table_rows,
            databases::browse::databases_query,
            databases::browse::databases_dump,
            databases::browse::databases_restore_dump,
            databases::browse::databases_test_connection,
            databases::connections::databases_connections_list,
            databases::connections::databases_connections_save,
            databases::connections::databases_connections_delete,
            databases::connections::databases_inspect_container,
            databases::connections::databases_mode_get,
            databases::connections::databases_mode_set,
            databases::browse::databases_table_structure,
            databases::browse::databases_apply_edits,
            databases::browse::databases_export,
            databases::browse::databases_add_column,
            databases::history::databases_history_list,
            databases::history::databases_history_clear,
            system::system_info_snapshot,
            tunnel::tunnel_detect,
            tunnel::tunnel_start,
            tunnel::tunnel_stop,
            tunnel::tunnel_list,
            workers::workers_list,
            workers::workers_create,
            workers::workers_update,
            workers::workers_delete,
            workers::workers_start,
            workers::workers_stop,
            workers::workers_restart,
            workers::workers_logs,
            ssh::hosts::ssh_hosts_list,
            ssh::hosts::ssh_hosts_save,
            ssh::hosts::ssh_hosts_delete,
            ssh::keys::ssh_keys_list,
            ssh::keys::ssh_keys_generate,
            ssh::keys::ssh_keys_import,
            ssh::keys::ssh_keys_delete,
            ssh::keys::ssh_keys_install_on_host,
            ssh::key_perms::ssh_key_permissions,
            ssh::key_perms::ssh_key_restrict,
            ssh::pty::pty_open,
            ssh::pty::pty_write,
            ssh::pty::pty_resize,
            ssh::pty::pty_close,
            ssh::pty::pty_adopt,
            ssh::pty::pty_cwd,
            ssh::sftp::sftp_connect,
            ssh::sftp::sftp_trust_host,
            ssh::sftp::sftp_disconnect,
            ssh::sftp::sftp_home,
            ssh::sftp::sftp_list,
            ssh::sftp::sftp_mkdir,
            ssh::sftp::sftp_rename,
            ssh::sftp::sftp_delete,
            ssh::sftp::sftp_download,
            ssh::sftp::sftp_download_zip,
            ssh::sftp::sftp_upload,
            ssh::sftp::sftp_read_file,
            ssh::sftp::sftp_write_file,
            ssh::sftp::sftp_open_in_editor,
            ssh::stats::term_host_stats,
            remote_desktop::connections::remote_desktop_list,
            remote_desktop::connections::remote_desktop_save,
            remote_desktop::connections::remote_desktop_delete,
            remote_desktop::vnc::remote_desktop_vnc_open,
            remote_desktop::vnc::remote_desktop_vnc_close,
            remote_desktop::rdp::remote_desktop_rdp_clients,
            remote_desktop::rdp::remote_desktop_rdp_launch,
            scheduled::scheduled_list,
            scheduled::scheduled_save,
            scheduled::scheduled_delete,
            scheduled::scheduled_set_enabled,
            scheduled::scheduled_run_now,
            scheduled::scheduled_logs,
            laravel_log::laravel_log_errors,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Tunnels are live child processes with no persistence — if we
            // don't kill them here, they'd keep running (and keep the public
            // URL alive) after DevsPro itself has quit.
            match event {
                tauri::RunEvent::ExitRequested { .. } => {
                    tunnel::kill_all(app_handle.state::<tunnel::TunnelState>());
                    ssh::pty::kill_all(&app_handle.state::<ssh::SshState>());
                    remote_desktop::vnc::kill_all(&app_handle.state::<remote_desktop::RemoteDesktopState>());
                }
                // A popped-out terminal window owns its shells; closing the
                // window is closing those terminals.
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::Destroyed,
                    ..
                } => ssh::pty::kill_owned_by(&app_handle.state::<ssh::SshState>(), &label),
                _ => {}
            }
        });
}
