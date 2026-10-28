//! Windows. Three things differ from Unix:
//!
//! 1. **The compiled-in root is fictional.** Apache Lounge builds report
//!    `HTTPD_ROOT="/apache"`; the real root is the directory above `bin\httpd.exe`, so
//!    every invocation passes it as `-d`.
//! 2. **There is no drop-in directory** DevsPro can rely on, so the include is a marked
//!    block at the end of `httpd.conf` — a valid place for it in Apache, unlike inside
//!    nginx's `http{}`.
//! 3. **`httpd -k restart` only reaches a service.** Laragon and XAMPP run Apache as a
//!    plain process, so reloading means ending that process and starting it again — done
//!    here, unelevated, so the new Apache belongs to the same user Laragon does and its
//!    own Stop button keeps working.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::elevate::Operation;
use crate::modules::nginx_ssl::nginx::platform::{MARKER_BEGIN, MARKER_END};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Directories that hold `bin\httpd.exe` directly.
const DIRECT_ROOTS: &[&str] = &[r"C:\Apache24", r"C:\xampp\apache", r"C:\Program Files\Apache24"];

/// Directories holding one versioned install per subdirectory — Laragon's
/// `httpd-2.4.54-win64-VS16`, WampServer's `apache2.4.54`.
const VERSIONED_ROOTS: &[&str] = &[r"C:\laragon\bin\apache", r"C:\wamp64\bin\apache", r"C:\wamp\bin\apache"];

pub fn find_binary() -> Option<String> {
    // PATH first, as on the nginx side: Windows has no canonical install location, so a
    // copy the user put on PATH is a deliberate choice.
    if let Some(found) = crate::modules::tools::find("httpd") {
        return Some(found);
    }
    for root in DIRECT_ROOTS {
        let candidate = Path::new(root).join("bin").join("httpd.exe");
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    VERSIONED_ROOTS.iter().find_map(|root| {
        fs::read_dir(root).ok()?.flatten().find_map(|entry| {
            let candidate = entry.path().join("bin").join("httpd.exe");
            candidate.is_file().then(|| candidate.to_string_lossy().to_string())
        })
    })
}

/// On Windows the running `httpd.exe` is also what every invocation goes through.
pub fn control_for_running(exe: &str) -> Option<String> {
    let name = Path::new(exe).file_name()?.to_str()?.to_ascii_lowercase();
    (name == "httpd.exe").then(|| exe.to_string())
}

fn install_dir(binary_path: &str) -> Option<PathBuf> {
    Some(Path::new(binary_path).parent()?.parent()?.to_path_buf())
}

/// `-d <install dir>`, forward-slashed as Apache's own Windows docs write it.
pub fn server_root_arg(binary_path: &str) -> Option<String> {
    install_dir(binary_path).map(|dir| to_config_path(&dir.to_string_lossy()))
}

/// `HTTPD_ROOT` is never the real one here — see the module docs.
pub fn fallback_server_root(_httpd_root: Option<&str>, binary_path: &str) -> Option<PathBuf> {
    install_dir(binary_path)
}

/// Backslashes would be read as escapes inside a quoted argument; Apache takes forward
/// slashes everywhere a path is expected on Windows.
pub fn to_config_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// The modules always sit in `<root>\modules`, which `config_scan` finds anyway.
pub fn known_module_dirs() -> Vec<PathBuf> {
    Vec::new()
}

pub fn wire_in_ops(config_path: &str, include_path: &str) -> Vec<Operation> {
    vec![Operation::SyncMarkedBlock {
        file_path: config_path.to_string(),
        marker_begin: MARKER_BEGIN.to_string(),
        marker_end: MARKER_END.to_string(),
        block_lines: vec![super::super::include_line(include_path)],
    }]
}

/// Nothing to do inside the elevated manifest — see `after_apply`.
pub fn reload_ops(_binary_path: &str, _pid_file: Option<&str>) -> Vec<Operation> {
    Vec::new()
}

/// Restarts a running Apache so it reads the new config. `Ok(Some(note))` carries what
/// the user should know (e.g. that Apache isn't running); `Err` means the restart failed
/// and the sites are not being served.
pub fn after_apply(binary_path: &str, server_root: Option<&str>, pid_file: Option<&str>) -> Result<Option<String>, String> {
    let not_running = || {
        Ok(Some(
            "The configuration is in place, but Apache is not running — start it (in Laragon: Start All) to serve the site."
                .to_string(),
        ))
    };
    let Some(pid) = pid_file.and_then(|f| fs::read_to_string(f).ok()).and_then(|t| t.trim().parse::<u32>().ok()) else {
        return not_running();
    };
    if !is_running(pid) {
        return not_running();
    }

    let kill = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run taskkill to restart Apache: {e}"))?;
    if !kill.status.success() {
        return Err(format!(
            "Apache (PID {pid}) could not be restarted — it is probably running as a Windows service or as another user. Restart it yourself to load the change, e.g. `httpd -k restart` from an administrator prompt. ({})",
            String::from_utf8_lossy(&kill.stderr).trim()
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while is_running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }

    let mut command = Command::new(binary_path);
    if let Some(root) = server_root {
        command.args(["-d", root]);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Apache was stopped but could not be started again: {e}"))?;

    // A port clash or a module that won't load ends the process within a moment; one
    // that is still up after this long has bound its ports and is serving. The handle is
    // then dropped, which on Windows does not end the process.
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
            }
            return Err(format!(
                "Apache was stopped but failed to start again ({status}): {}",
                stderr.trim()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(Some("Restarted Apache.".to_string()))
}

fn is_running(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\"")))
        .unwrap_or(false)
}

/// The trailing slash is load-bearing: without it Apache appends the script's `C:/...`
/// path straight after the port and cannot parse the URL ("URI cannot be parsed").
pub fn fcgi_handler(fastcgi_pass: &str) -> String {
    format!("proxy:fcgi://{fastcgi_pass}/")
}

/// `mod_proxy_fcgi` hands PHP `SCRIPT_FILENAME=/C:/...`, which `php-cgi.exe` cannot open
/// ("No input file specified"); rebuilding it from the document root gives the real path.
pub fn script_filename_fix() -> Option<&'static str> {
    Some(r#"ProxyFCGISetEnvIf "true" SCRIPT_FILENAME "%{DOCUMENT_ROOT}%{reqenv:SCRIPT_NAME}""#)
}

/// winget carries no Apache package DevsPro could name with confidence, so installing is
/// left to the user.
pub fn package_manager() -> Option<String> {
    None
}

pub fn install_packages(_manager: &str) -> Vec<String> {
    Vec::new()
}

pub fn manual_install_hint() -> &'static str {
    "Install it manually, then recheck — unzip the Apache Lounge build (apachelounge.com/download) to C:\\Apache24, or use the one Laragon or XAMPP ships."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_the_directory_above_bin() {
        assert_eq!(
            server_root_arg(r"C:\laragon\bin\apache\httpd-2.4.54-win64-VS16\bin\httpd.exe").as_deref(),
            Some("C:/laragon/bin/apache/httpd-2.4.54-win64-VS16")
        );
    }

    #[test]
    fn wiring_is_one_marked_block_at_the_end_of_httpd_conf() {
        let ops = wire_in_ops(r"C:\Apache24\conf\httpd.conf", "C:/Users/MD. Talha/AppData/Roaming/x/apache/devspro.conf");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            Operation::SyncMarkedBlock { block_lines, .. } => assert_eq!(
                block_lines,
                &vec![r#"Include "C:/Users/MD. Talha/AppData/Roaming/x/apache/devspro.conf""#.to_string()]
            ),
            other => panic!("expected a marked block, got {other:?}"),
        }
    }
}
