//! Phase 1 module: per-project local dev sites, reverse-proxied through nginx — or
//! Apache, whichever the user picks (see `web_server`) — with a locally trusted SSL
//! certificate. See the approved plan for the full design;
//! this file just wires the submodules together and owns the shared app-data paths
//! everything else here builds on.

pub mod apache;
pub mod ca;
pub mod hosts;
pub mod laravel;
pub mod limits;
pub mod nginx;
pub mod php_fpm;
pub mod port_conflict;
pub mod ports;
pub mod sites;
pub mod web_server;

use std::path::PathBuf;
use tauri::Manager;

pub fn migrations() -> Vec<&'static str> {
    sites::migrations()
}

fn app_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

pub fn certs_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_root_dir(app)?.join("certs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn nginx_sites_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_root_dir(app)?.join("nginx").join("sites");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Apache's counterpart of the `nginx` directory: `devspro.conf` and `sites/`.
pub fn apache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_root_dir(app)?.join("apache");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn apache_sites_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = apache_dir(app)?.join("sites");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn logs_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_root_dir(app)?.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}
