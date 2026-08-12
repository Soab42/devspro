//! PHP-FPM socket detection, for serving Laravel projects directly (no `artisan
//! serve`, no port to manage). Always re-detected fresh at add/enable time rather
//! than cached — the installed PHP version can change independently of this app.
//!
//! A machine can have several PHP-FPM pools installed side by side (Debian/Ubuntu's
//! `phpX.Y-fpm` packaging convention, or the ondrej/sury PPA that adds even more
//! versions) but only some of them actually running. Only versions with a live
//! socket under `/run/php` count as "available" — a package being installed per
//! dpkg doesn't mean its pool is up, and pointing a site at a dead socket is worse
//! than pointing it at nothing.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PhpFpmDetection {
    pub found: bool,
    pub socket_path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhpFpmVersion {
    pub version: String,
    pub socket_path: String,
}

const SOCK_DIR: &str = "/run/php";

/// Parses a `/run/php` entry name like `php8.3-fpm.sock` into its version string,
/// plus a sortable (major, minor) key. Pulled out on its own so the parsing logic
/// is testable without touching the filesystem.
fn parse_sock_filename(name: &str) -> Option<(u32, u32, String)> {
    let version = name.strip_prefix("php")?.strip_suffix("-fpm.sock")?;
    // Reject a bare "php-fpm.sock" (no version segment) rather than letting it
    // parse as version "" / (0, 0) — that's not a real per-version pool name.
    if version.is_empty() || !version.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let (major, minor) = version_key(version);
    Some((major, minor, version.to_string()))
}

fn version_key(v: &str) -> (u32, u32) {
    let mut parts = v.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

fn scan_sockets() -> Vec<(u32, u32, String, String)> {
    let mut candidates: Vec<(u32, u32, String, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(SOCK_DIR) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some((major, minor, version)) = parse_sock_filename(&name) {
                candidates.push((major, minor, version, entry.path().to_string_lossy().to_string()));
            }
        }
    }
    candidates.sort();
    candidates
}

pub fn detect() -> PhpFpmDetection {
    // Highest version wins when more than one PHP-FPM pool is installed.
    match scan_sockets().pop() {
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

/// Every PHP-FPM pool currently running on this machine, highest version first.
pub fn list_available_versions() -> Vec<PhpFpmVersion> {
    let mut candidates = scan_sockets();
    candidates.sort();
    candidates.reverse();
    candidates
        .into_iter()
        .map(|(_, _, version, socket_path)| PhpFpmVersion { version, socket_path })
        .collect()
}

/// Pure lookup against an already-fetched version list — split out from
/// `socket_for_version` so the matching logic is testable without depending on
/// what's actually running on the machine executing the test.
fn pick_socket(versions: &[PhpFpmVersion], want: &str) -> Option<String> {
    versions.iter().find(|v| v.version == want).map(|v| v.socket_path.clone())
}

pub fn socket_for_version(version: &str) -> Option<String> {
    pick_socket(&list_available_versions(), version)
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

#[tauri::command]
pub fn nginx_ssl_list_php_versions() -> Vec<PhpFpmVersion> {
    list_available_versions()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_socket_names() {
        assert_eq!(parse_sock_filename("php8.3-fpm.sock"), Some((8, 3, "8.3".to_string())));
        assert_eq!(parse_sock_filename("php8.1-fpm.sock"), Some((8, 1, "8.1".to_string())));
        assert_eq!(parse_sock_filename("php7.4-fpm.sock"), Some((7, 4, "7.4".to_string())));
    }

    #[test]
    fn rejects_unrelated_filenames() {
        assert_eq!(parse_sock_filename("php-fpm.sock"), None);
        assert_eq!(parse_sock_filename("php8.3-fpm.pid"), None);
        assert_eq!(parse_sock_filename("nginx.sock"), None);
        assert_eq!(parse_sock_filename("php8.3.sock"), None);
    }

    #[test]
    fn version_key_orders_major_then_minor() {
        assert!(version_key("8.3") > version_key("8.1"));
        assert!(version_key("8.10") > version_key("8.9"));
        assert!(version_key("7.4") < version_key("8.0"));
    }

    #[test]
    fn version_key_defaults_unparsable_parts_to_zero() {
        assert_eq!(version_key("garbage"), (0, 0));
        assert_eq!(version_key("8"), (8, 0));
    }

    #[test]
    fn pick_socket_matches_exact_version() {
        let versions = vec![
            PhpFpmVersion { version: "8.3".to_string(), socket_path: "/run/php/php8.3-fpm.sock".to_string() },
            PhpFpmVersion { version: "8.1".to_string(), socket_path: "/run/php/php8.1-fpm.sock".to_string() },
        ];
        assert_eq!(pick_socket(&versions, "8.1"), Some("/run/php/php8.1-fpm.sock".to_string()));
        assert_eq!(pick_socket(&versions, "8.3"), Some("/run/php/php8.3-fpm.sock".to_string()));
    }

    #[test]
    fn pick_socket_returns_none_for_missing_version() {
        let versions = vec![PhpFpmVersion { version: "8.3".to_string(), socket_path: "/run/php/php8.3-fpm.sock".to_string() }];
        assert_eq!(pick_socket(&versions, "8.1"), None);
        assert_eq!(pick_socket(&[], "8.1"), None);
    }
}
