mod auth;
mod db;
mod elevate;
mod modules;

use modules::apps;
use modules::databases;
use modules::docker;
use modules::nginx_ssl::{ca, laravel, nginx, php_fpm, ports, sites};
use modules::processes;
use modules::system;
use tauri::Manager;

#[tauri::command]
fn elevate_ping() -> Result<String, String> {
    elevate::ping()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let conn = db::open(&app_data_dir).expect("failed to open/migrate app database");
            app.manage(db::Db(std::sync::Mutex::new(conn)));
            app.manage(system::SystemState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            elevate_ping,
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
            laravel::nginx_ssl_validate_laravel_project,
            sites::nginx_ssl_list_sites,
            sites::nginx_ssl_hostname_conflict,
            sites::nginx_ssl_add_site,
            sites::nginx_ssl_set_site_enabled,
            sites::nginx_ssl_fix_laravel_permissions,
            sites::nginx_ssl_remove_site,
            docker::docker_detect,
            docker::docker_list_containers,
            docker::docker_start,
            docker::docker_stop,
            docker::docker_restart,
            docker::docker_logs,
            docker::docker_change_port,
            processes::processes_list_ports,
            processes::processes_kill,
            apps::apps_list,
            apps::apps_install_batch,
            databases::databases_create,
            databases::databases_list,
            databases::databases_get_credentials,
            databases::databases_remove,
            databases::databases_list_users,
            databases::databases_create_user,
            databases::databases_drop_user,
            databases::databases_change_admin_password,
            databases::databases_change_user_password,
            system::system_info_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
