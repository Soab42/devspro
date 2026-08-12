mod auth;
mod db;
mod elevate;
mod modules;
mod tray;

use modules::apps;
use modules::databases;
use modules::docker;
use modules::logs;
use modules::mailcatcher;
use modules::nginx_ssl::{ca, laravel, nginx, php_fpm, ports, sites};
use modules::processes;
use modules::scaffold;
use modules::settings;
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
            sites::nginx_ssl_fix_laravel_permissions,
            sites::nginx_ssl_remove_site,
            scaffold::scaffold_create_laravel_project,
            docker::docker_detect,
            docker::docker_list_containers,
            docker::docker_start,
            docker::docker_stop,
            docker::docker_restart,
            docker::docker_logs,
            docker::docker_change_port,
            logs::logs_list_sources,
            logs::logs_tail,
            mailcatcher::mailcatcher_detect_docker,
            mailcatcher::mailcatcher_status,
            mailcatcher::mailcatcher_start,
            mailcatcher::mailcatcher_stop,
            mailcatcher::mailcatcher_remove,
            processes::processes_list_ports,
            processes::processes_kill,
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
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Tunnels are live child processes with no persistence — if we
            // don't kill them here, they'd keep running (and keep the public
            // URL alive) after DevsPro itself has quit.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                tunnel::kill_all(app_handle.state::<tunnel::TunnelState>());
            }
        });
}
