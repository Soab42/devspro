//! Linux and macOS. A machine can have several PHP-FPM pools installed side by side
//! (Debian/Ubuntu's `phpX.Y-fpm` packaging convention, or the ondrej/sury PPA that adds
//! even more versions) but only some of them actually running. Only versions with a live
//! socket under `/run/php` count as "available" — a package being installed per dpkg
//! doesn't mean its pool is up, and pointing a site at a dead socket is worse than
//! pointing it at nothing.
//!
//! This is the code the module ran before the platform split, moved rather than rewritten.
//! The one change is where the `unix:` prefix is added: it now goes on here, because it is
//! part of what makes this a *Unix* `fastcgi_pass` value, and the renderer no longer knows
//! which kind it is handling.

use super::super::PhpFpmVersion;

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

/// nginx wants a Unix-socket backend written as `unix:<path>`.
fn fastcgi_pass_for(socket_path: &str) -> String {
    format!("unix:{socket_path}")
}

/// Every PHP-FPM pool currently running on this machine, highest version first.
pub fn list_available_versions() -> Vec<PhpFpmVersion> {
    let mut candidates = scan_sockets();
    candidates.sort();
    candidates.reverse();
    candidates
        .into_iter()
        .map(|(_, _, version, socket_path)| PhpFpmVersion {
            version,
            fastcgi_pass: fastcgi_pass_for(&socket_path),
        })
        .collect()
}

/// Highest version wins when more than one PHP-FPM pool is installed.
pub fn detect_default() -> Option<PhpFpmVersion> {
    scan_sockets().pop().map(|(_, _, version, socket_path)| PhpFpmVersion {
        version,
        fastcgi_pass: fastcgi_pass_for(&socket_path),
    })
}

/// Pure lookup against an already-fetched version list — split out from
/// `target_for_version` so the matching logic is testable without depending on
/// what's actually running on the machine executing the test.
fn pick(versions: &[PhpFpmVersion], want: &str) -> Option<String> {
    versions
        .iter()
        .find(|v| v.version == want)
        .map(|v| v.fastcgi_pass.clone())
}

pub fn target_for_version(version: &str) -> Option<String> {
    pick(&list_available_versions(), version)
}

pub fn install_hint() -> String {
    "PHP-FPM was not found (looked in /run/php) — install it, e.g. `sudo apt install php8.3-fpm`, then try again"
        .to_string()
}

/// Why a version the user pinned isn't usable right now. Phrased for this OS: on Linux a
/// pool is something the system runs, so the fix is to start it.
pub fn missing_version_hint(version: &str) -> String {
    format!(
        "PHP {version} is not currently running (no live socket in /run/php) — install/start php{version}-fpm, or leave the version unset to use the system default"
    )
}

/// The PHP command-line binary to run artisan with. Prefers the version-suffixed binary
/// (`php8.3`) when a version is pinned and Debian/Ubuntu's alternatives layout provides
/// one, so artisan runs under the same PHP the site is served with.
pub fn cli_binary(version: Option<&str>) -> String {
    version
        .map(|v| format!("php{}", v.trim()))
        .filter(|bin| crate::modules::tools::is_available(bin))
        .unwrap_or_else(|| "php".to_string())
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

    fn version(v: &str, socket: &str) -> PhpFpmVersion {
        PhpFpmVersion {
            version: v.to_string(),
            fastcgi_pass: fastcgi_pass_for(socket),
        }
    }

    #[test]
    fn pick_matches_exact_version() {
        let versions = vec![
            version("8.3", "/run/php/php8.3-fpm.sock"),
            version("8.1", "/run/php/php8.1-fpm.sock"),
        ];
        assert_eq!(pick(&versions, "8.1").as_deref(), Some("unix:/run/php/php8.1-fpm.sock"));
        assert_eq!(pick(&versions, "8.3").as_deref(), Some("unix:/run/php/php8.3-fpm.sock"));
    }

    #[test]
    fn pick_returns_none_for_missing_version() {
        let versions = vec![version("8.3", "/run/php/php8.3-fpm.sock")];
        assert_eq!(pick(&versions, "8.1"), None);
        assert_eq!(pick(&[], "8.1"), None);
    }

    #[test]
    fn unix_sockets_are_passed_to_nginx_with_the_unix_scheme() {
        assert_eq!(
            fastcgi_pass_for("/run/php/php8.3-fpm.sock"),
            "unix:/run/php/php8.3-fpm.sock"
        );
    }
}
