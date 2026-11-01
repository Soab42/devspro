//! Which web server DevsPro's sites are written for — nginx or Apache — and everything
//! `sites` needs from it, behind one type so the add/enable/remove flows read the same
//! for either.
//!
//! One server at a time, by design: both want ports 80 and 443, and only one process can
//! own a port. The choice is a single setting, nginx by default so every install that
//! predates Apache support keeps behaving exactly as it did.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;

use super::apache::{self, ApacheDetection};
use super::limits::SiteLimits;
use super::nginx::{self, render::BackendTarget};
use crate::elevate::Operation;

const SETTING_KEY: &str = "nginx_ssl.web_server";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebServer {
    Nginx,
    Apache,
}

impl WebServer {
    pub fn label(self) -> &'static str {
        match self {
            WebServer::Nginx => "nginx",
            WebServer::Apache => "Apache",
        }
    }

    pub fn other(self) -> WebServer {
        match self {
            WebServer::Nginx => WebServer::Apache,
            WebServer::Apache => WebServer::Nginx,
        }
    }

    /// Where this server's vhosts live. Each server has its own directory, so switching
    /// never leaves one server reading the other's syntax.
    pub fn sites_dir(self, app: &AppHandle) -> Result<PathBuf, String> {
        match self {
            WebServer::Nginx => super::nginx_sites_dir(app),
            WebServer::Apache => super::apache_sites_dir(app),
        }
    }
}

pub fn active(conn: &Connection) -> WebServer {
    match crate::modules::settings::get(conn, SETTING_KEY).as_deref() {
        Some("apache") => WebServer::Apache,
        _ => WebServer::Nginx,
    }
}

pub fn set_active(conn: &Connection, server: WebServer) -> Result<(), String> {
    let value = match server {
        WebServer::Nginx => "nginx",
        WebServer::Apache => "apache",
    };
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (SETTING_KEY, value),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// A server that was found on this machine, with everything needed to drive it.
pub enum Ready {
    Nginx { binary_path: String, config_path: String },
    Apache(ApacheDetection),
}

/// Takes the database lock itself (to read and remember the binary's path — see
/// `locate`), so it must not be called while the caller holds it.
pub fn require(db: &crate::db::Db, server: WebServer) -> Result<Ready, String> {
    match server {
        WebServer::Nginx => {
            let detection = super::locate::detect_nginx(db);
            let binary_path = detection
                .binary_path
                .ok_or_else(|| "nginx was not found on this machine — install it first (see the Nginx tab)".to_string())?;
            let config_path = detection
                .config_path
                .ok_or_else(|| "could not determine nginx's main config path".to_string())?;
            Ok(Ready::Nginx { binary_path, config_path })
        }
        WebServer::Apache => {
            let detection = super::locate::detect_apache(db);
            if !detection.found {
                return Err("Apache was not found on this machine — install it first (see the Apache tab)".to_string());
            }
            if detection.config_path.is_none() {
                return Err("could not determine Apache's main config file".to_string());
            }
            Ok(Ready::Apache(detection))
        }
    }
}

impl Ready {
    pub fn kind(&self) -> WebServer {
        match self {
            Ready::Nginx { .. } => WebServer::Nginx,
            Ready::Apache(_) => WebServer::Apache,
        }
    }

    pub fn label(&self) -> &'static str {
        self.kind().label()
    }

    pub fn conf_path(&self, app: &AppHandle, slug: &str) -> Result<String, String> {
        Ok(self
            .kind()
            .sites_dir(app)?
            .join(format!("{slug}.conf"))
            .to_string_lossy()
            .to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        hostname: &str,
        target: &BackendTarget,
        limits: &SiteLimits,
        cert_path: &str,
        key_path: &str,
        access_log: &str,
        error_log: &str,
    ) -> String {
        match self {
            Ready::Nginx { .. } => {
                nginx::render::render_site_conf(hostname, target, limits, cert_path, key_path, access_log, error_log)
            }
            Ready::Apache(_) => {
                apache::render::render_site_conf(hostname, target, limits, cert_path, key_path, access_log, error_log)
            }
        }
    }

    /// Every elevated operation that makes the server read DevsPro's sites. For Apache
    /// that includes rewriting `devspro.conf` itself, since which modules and `Listen`
    /// lines it needs depends on the user's config as it is now.
    pub fn wire_in_ops(&self, app: &AppHandle) -> Result<Vec<Operation>, String> {
        let sites_glob = self.kind().sites_dir(app)?.join("*.conf").to_string_lossy().to_string();
        match self {
            Ready::Nginx { config_path, .. } => nginx::wire_in_sites_ops(config_path, &sites_glob),
            Ready::Apache(detection) => {
                let global = super::apache_dir(app)?.join("devspro.conf");
                let contents = apache::render_global_conf(detection, &global, &sites_glob)?;
                let global = global.to_string_lossy().to_string();
                let config_path = detection.config_path.as_deref().unwrap_or_default();
                let mut ops = vec![Operation::WriteFile {
                    path: global.clone(),
                    contents,
                }];
                ops.extend(apache::platform::wire_in_ops(config_path, &global));
                Ok(ops)
            }
        }
    }

    /// The config test, then — where it happens inside the manifest — the reload.
    pub fn test_and_reload_ops(&self) -> Vec<Operation> {
        match self {
            Ready::Nginx { binary_path, .. } => nginx::test_and_reload_ops(binary_path),
            Ready::Apache(detection) => {
                let binary_path = detection.binary_path.clone().unwrap_or_default();
                let mut ops = vec![Operation::RunApacheTest {
                    binary_path: binary_path.clone(),
                    server_root: detection.server_root_arg.clone(),
                }];
                ops.extend(apache::platform::reload_ops(&binary_path, detection.pid_file.as_deref()));
                ops
            }
        }
    }

    /// Whatever has to happen after the manifest succeeded and cannot happen inside it —
    /// on Windows, restarting Apache unelevated. `Ok(Some(note))` is something the user
    /// should be told even though nothing failed.
    pub fn after_apply(&self) -> Result<Option<String>, String> {
        match self {
            Ready::Nginx { .. } => Ok(None),
            Ready::Apache(detection) => apache::platform::after_apply(
                detection.binary_path.as_deref().unwrap_or_default(),
                detection.server_root_arg.as_deref(),
                detection.pid_file.as_deref(),
            ),
        }
    }
}

#[tauri::command(async)]
pub fn nginx_ssl_get_web_server(db: tauri::State<crate::db::Db>) -> Result<WebServer, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(active(&conn))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        conn
    }

    #[test]
    fn nginx_is_the_default_so_existing_installs_are_unchanged() {
        assert_eq!(active(&conn()), WebServer::Nginx);
    }

    #[test]
    fn the_choice_round_trips() {
        let conn = conn();
        set_active(&conn, WebServer::Apache).unwrap();
        assert_eq!(active(&conn), WebServer::Apache);
        set_active(&conn, WebServer::Nginx).unwrap();
        assert_eq!(active(&conn), WebServer::Nginx);
    }

    #[test]
    fn serialises_as_the_lowercase_name_the_frontend_uses() {
        assert_eq!(serde_json::to_string(&WebServer::Apache).unwrap(), "\"apache\"");
        assert_eq!(serde_json::from_str::<WebServer>("\"nginx\"").unwrap(), WebServer::Nginx);
    }
}
