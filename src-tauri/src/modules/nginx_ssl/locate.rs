//! Which nginx or Apache binary to drive — found from what is actually running rather
//! than only guessed from a list of install folders.
//!
//! In order:
//!
//! 1. **The running process.** Whatever listens with the server's process name is asked
//!    for its executable path (`processes::exe_path`). That is exactly the server the user
//!    runs, wherever it was installed — XAMPP, WAMP, Laragon, a hand-unzipped copy — and
//!    which of several installs is the live one. The path is remembered in `settings`.
//! 2. **The remembered path**, for when the server is stopped — as long as the file is
//!    still there (an upgrade renames Laragon's versioned folder).
//! 3. **The usual install locations** (`nginx::find_binary`, `apache::find_binary`).
//!
//! Every path is checked to be named like the server it claims to be before it is used:
//! the helper later runs it elevated (`nginx -t`), so a remembered path is effectively an
//! instruction to execute, and the name check keeps it from ever being anything else.

use serde::Serialize;
use std::fs;
use std::path::Path;

use super::apache::{self, ApacheDetection};
use super::nginx::{self, NginxDetection};
use super::web_server::WebServer;
use crate::db::Db;
use crate::modules::processes::{self, PortEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Read from the process running right now.
    Running,
    /// Remembered from an earlier time it was running.
    Saved,
    /// One of the usual install locations.
    Search,
}

fn setting_key(server: WebServer) -> &'static str {
    match server {
        WebServer::Nginx => "nginx_ssl.nginx_binary",
        WebServer::Apache => "nginx_ssl.apache_binary",
    }
}

/// File names a running process may have, without `.exe`.
fn process_names(server: WebServer) -> &'static [&'static str] {
    match server {
        WebServer::Nginx => &["nginx"],
        WebServer::Apache => &["httpd", "apache2"],
    }
}

/// File names a path DevsPro drives may have: the process names, plus Apache's control
/// scripts, which is what runs it on Linux and macOS.
fn binary_names(server: WebServer) -> &'static [&'static str] {
    match server {
        WebServer::Nginx => &["nginx"],
        WebServer::Apache => &["httpd", "apache2", "apachectl", "apache2ctl"],
    }
}

fn stem_is(path_or_name: &str, names: &[&str]) -> bool {
    let name = Path::new(path_or_name)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    names.contains(&stem)
}

fn usable(server: WebServer, path: &str) -> bool {
    stem_is(path, binary_names(server)) && Path::new(path).is_file()
}

/// The running server's binary, preferring one that holds 80 or 443 — that is the one
/// serving sites, not Laragon's nginx parked on 8080 alongside it.
fn running(server: WebServer) -> Option<String> {
    let listeners = processes::listening_ports().ok()?;
    running_from(&listeners, server, processes::exe_path)
}

fn running_from(listeners: &[PortEntry], server: WebServer, exe_path: impl Fn(u32) -> Option<String>) -> Option<String> {
    let mut candidates: Vec<&PortEntry> = listeners
        .iter()
        .filter(|e| e.process_name.as_deref().is_some_and(|n| stem_is(n, process_names(server))))
        .collect();
    candidates.sort_by_key(|e| !matches!(e.port, 80 | 443));
    candidates.iter().filter_map(|e| e.pid).find_map(|pid| {
        let exe = exe_path(pid)?;
        if !stem_is(&exe, process_names(server)) {
            return None;
        }
        match server {
            WebServer::Nginx => Some(exe),
            WebServer::Apache => apache::platform::control_for_running(&exe),
        }
    })
}

/// Whether the pid recorded in `pid_file` belongs to a live process of `server`. A pid
/// file on its own proves nothing — Laragon leaves `httpd.pid` behind when it stops
/// Apache, and that pid may since have gone to an unrelated process.
pub fn pid_is_live(pid_file: &Path, server: WebServer) -> bool {
    let Some(pid) = fs::read_to_string(pid_file).ok().and_then(|t| t.trim().parse::<u32>().ok()) else {
        return false;
    };
    processes::process_name(pid).is_some_and(|name| stem_is(&name, process_names(server)))
}

/// The binary to drive, and how it was found. Takes the lock itself, briefly, so it must
/// not be called while the caller holds it.
pub fn locate(db: &Db, server: WebServer) -> Option<(String, Source)> {
    if let Some(path) = running(server).filter(|p| usable(server, p)) {
        if let Ok(conn) = db.0.lock() {
            let _ = conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (setting_key(server), &path),
            );
        }
        return Some((path, Source::Running));
    }
    let saved = db
        .0
        .lock()
        .ok()
        .and_then(|conn| crate::modules::settings::get(&conn, setting_key(server)));
    if let Some(path) = saved.filter(|p| usable(server, p)) {
        return Some((path, Source::Saved));
    }
    let found = match server {
        WebServer::Nginx => nginx::find_binary(),
        WebServer::Apache => apache::find_binary(),
    };
    found.map(|path| (path, Source::Search))
}

pub fn detect_nginx(db: &Db) -> NginxDetection {
    let located = locate(db, WebServer::Nginx);
    let mut detection = nginx::detect_with(located.as_ref().map(|(path, _)| path.clone()));
    detection.source = located.map(|(_, source)| source);
    detection
}

pub fn detect_apache(db: &Db) -> ApacheDetection {
    let located = locate(db, WebServer::Apache);
    let mut detection = apache::detect_with(located.as_ref().map(|(path, _)| path.clone()));
    detection.source = located.map(|(_, source)| source);
    detection
}

/// Drops the remembered path, so the next detection starts over from the running
/// process and the usual locations — for when the remembered install was replaced.
#[tauri::command(async)]
pub fn nginx_ssl_forget_server_path(db: tauri::State<Db>, server: WebServer) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM settings WHERE key = ?1", [setting_key(server)])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(port: u16, name: &str, pid: u32) -> PortEntry {
        PortEntry {
            port,
            address: "0.0.0.0".to_string(),
            pid: Some(pid),
            process_name: Some(name.to_string()),
            command: None,
        }
    }

    #[test]
    fn names_are_matched_without_extension_or_case() {
        assert!(stem_is(r"C:\xampp\apache\bin\httpd.exe", &["httpd"]));
        assert!(stem_is("NGINX.EXE", &["nginx"]));
        assert!(stem_is("/usr/sbin/apache2", &["httpd", "apache2"]));
        assert!(!stem_is(r"C:\Windows\System32\cmd.exe", &["nginx"]));
    }

    #[test]
    fn prefers_the_server_holding_80_or_443() {
        // Laragon's own nginx on 8080 next to a second nginx serving the sites.
        let listeners = vec![listener(8080, "nginx.exe", 1), listener(443, "nginx.exe", 2)];
        let path = running_from(&listeners, WebServer::Nginx, |pid| Some(format!(r"C:\n{pid}\nginx.exe")));
        assert_eq!(path.as_deref(), Some(r"C:\n2\nginx.exe"));
    }

    #[test]
    fn ignores_other_programs_and_wrongly_named_executables() {
        let listeners = vec![listener(80, "skype.exe", 7), listener(80, "nginx.exe", 8)];
        // pid 8 claims to be nginx but its image is something else entirely.
        let path = running_from(&listeners, WebServer::Nginx, |_| Some(r"C:\evil\payload.exe".to_string()));
        assert_eq!(path, None);
    }

    #[test]
    fn a_path_must_exist_and_be_named_like_the_server() {
        assert!(!usable(WebServer::Nginx, r"C:\definitely\not\here\nginx.exe"));
        let me = std::env::current_exe().unwrap().to_string_lossy().to_string();
        assert!(!usable(WebServer::Nginx, &me), "an existing file with the wrong name is refused");
    }

    #[test]
    fn a_missing_empty_or_dead_pid_file_is_not_running() {
        let dir = std::env::temp_dir().join(format!("devspro-pid-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("httpd.pid");
        assert!(!pid_is_live(&file, WebServer::Apache), "no file");
        fs::write(&file, "").unwrap();
        assert!(!pid_is_live(&file, WebServer::Apache), "empty, as Laragon leaves nginx.pid");
        // This test's own process is alive but is not Apache — a pid reused by
        // something else must not count.
        fs::write(&file, std::process::id().to_string()).unwrap();
        assert!(!pid_is_live(&file, WebServer::Apache), "live, but not Apache");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_running_httpd_is_driven_directly_on_windows() {
        let listeners = vec![listener(80, "httpd.exe", 17940)];
        let exe = r"C:\xampp\apache\bin\httpd.exe";
        assert_eq!(
            running_from(&listeners, WebServer::Apache, |_| Some(exe.to_string())).as_deref(),
            Some(exe)
        );
    }
}
