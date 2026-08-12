//! PHP-FPM socket detection, for serving Laravel projects directly (no `artisan
//! serve`, no port to manage). Always re-detected fresh at add/enable time rather
//! than cached — the installed PHP version can change independently of this app.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PhpFpmDetection {
    pub found: bool,
    pub socket_path: Option<String>,
    pub version: Option<String>,
}

const SOCK_DIR: &str = "/run/php";

pub fn detect() -> PhpFpmDetection {
    let mut candidates: Vec<(u32, u32, String, String)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(SOCK_DIR) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(version) = name.strip_prefix("php").and_then(|s| s.strip_suffix("-fpm.sock")) {
                let (major, minor) = version_key(version);
                candidates.push((major, minor, version.to_string(), entry.path().to_string_lossy().to_string()));
            }
        }
    }
    // Highest version wins when more than one PHP-FPM pool is installed.
    candidates.sort();

    match candidates.pop() {
        Some((_, _, version, path)) => PhpFpmDetection {
            found: true,
            socket_path: Some(path),
            version: Some(version),
        },
        None => PhpFpmDetection {
            found: false,
            socket_path: None,
            version: None,
        },
    }
}

fn version_key(v: &str) -> (u32, u32) {
    let mut parts = v.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

pub fn require_socket() -> Result<String, String> {
    detect().socket_path.ok_or_else(|| {
        "PHP-FPM was not found (looked in /run/php) — install it, e.g. `sudo apt install php8.3-fpm`, then try again"
            .to_string()
    })
}

#[tauri::command]
pub fn nginx_ssl_detect_php_fpm() -> PhpFpmDetection {
    detect()
}
