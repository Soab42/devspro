mod auth;
mod db;
mod elevate;
mod modules;
mod tray;

use modules::apps;
use modules::backup;
use modules::browser;
use modules::databases;
use modules::docker;
use modules::env_diff;
use modules::file_manager;
use modules::hosts_file;
use modules::http_client;
use modules::logs;
use modules::mailcatcher;
use modules::nginx_ssl::{ca, laravel, nginx, php_fpm, ports, sites};
use modules::processes;
use modules::redis;
use modules::scaffold;
use modules::scheduled;
use modules::settings;
use modules::ssh;
use modules::system;
use modules::tunnel;
use modules::workers;
use tauri::Manager;

#[tauri::command]
fn elevate_ping(app: tauri::AppHandle) -> Result<String, String> {
    elevate::ping(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
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
            file_manager::fm_mkdir,
            file_manager::fm_create_file,
            file_manager::fm_rename,
            file_manager::fm_trash,
            file_manager::fm_read_file,
            file_manager::fm_write_file,
            file_manager::fm_open,
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
            processes::processes_list_ports,
            processes::processes_kill,
            redis::redis_ping,
            redis::redis_scan,
            redis::redis_get,
            redis::redis_delete,
            redis::redis_flush_db,
            redis::redis_laravel_queues,
            redis::redis_peek_job,
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
            databases::browse::databases_table_rows,
            databases::browse::databases_query,
            databases::browse::databases_dump,
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
            ssh::keys::ssh_keys_delete,
            ssh::keys::ssh_keys_install_on_host,
            ssh::pty::pty_open,
            ssh::pty::pty_write,
            ssh::pty::pty_resize,
            ssh::pty::pty_close,
            ssh::sftp::sftp_connect,
            ssh::sftp::sftp_trust_host,
            ssh::sftp::sftp_disconnect,
            ssh::sftp::sftp_home,
            ssh::sftp::sftp_list,
            ssh::sftp::sftp_mkdir,
            ssh::sftp::sftp_rename,
            ssh::sftp::sftp_delete,
            ssh::sftp::sftp_download,
            ssh::sftp::sftp_upload,
            scheduled::scheduled_list,
            scheduled::scheduled_save,
            scheduled::scheduled_delete,
            scheduled::scheduled_set_enabled,
            scheduled::scheduled_run_now,
            scheduled::scheduled_logs,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Tunnels are live child processes with no persistence — if we
            // don't kill them here, they'd keep running (and keep the public
            // URL alive) after DevsPro itself has quit.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                tunnel::kill_all(app_handle.state::<tunnel::TunnelState>());
                ssh::pty::kill_all(&app_handle.state::<ssh::SshState>());
            }
        });
}
