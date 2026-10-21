//! The PHP FastCGI backend behind a Laravel site — serving the project directly, with no
//! `artisan serve` and no port for the user to manage. Always re-detected fresh at
//! add/enable time rather than cached: the installed PHP versions can change independently
//! of this app.
//!
//! Which PHP that means, and how it is reached, is per-OS and lives in `platform/`: a
//! PHP-FPM pool over a Unix socket on Linux and macOS, `php-cgi.exe` over a loopback port
//! on Windows. Both sides answer the same questions, and both hand back a ready-made
//! `fastcgi_pass` value rather than a path, so nothing above this module needs to know
//! which kind it has.

mod platform;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PhpFpmDetection {
    pub found: bool,
    /// What nginx should be given as its `fastcgi_pass` — `unix:<socket>` on Unix, a
    /// `host:port` on Windows.
    pub fastcgi_pass: Option<String>,
    pub version: Option<String>,
    /// What to tell the user when nothing usable was found, in this OS's terms. Supplied by
    /// the backend rather than written into the UI, which has no way to know whether the
    /// answer is `apt install php8.3-fpm` or "install PHP".
    pub install_hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhpFpmVersion {
    pub version: String,
    pub fastcgi_pass: String,
}

pub fn detect() -> PhpFpmDetection {
    match platform::detect_default() {
        Some(default) => PhpFpmDetection {
            found: true,
            fastcgi_pass: Some(default.fastcgi_pass),
            version: Some(default.version),
            install_hint: platform::install_hint(),
        },
        None => PhpFpmDetection {
            found: false,
            fastcgi_pass: None,
            version: None,
            install_hint: platform::install_hint(),
        },
    }
}

pub fn list_available_versions() -> Vec<PhpFpmVersion> {
    platform::list_available_versions()
}

/// The `fastcgi_pass` for a version the user pinned, or `None` if it can't be used.
pub fn target_for_version(version: &str) -> Option<String> {
    platform::target_for_version(version)
}

/// Why `target_for_version` said no, phrased for this OS.
pub fn missing_version_hint(version: &str) -> String {
    platform::missing_version_hint(version)
}

/// The `fastcgi_pass` for a site that didn't pin a version.
pub fn require_target() -> Result<String, String> {
    detect().fastcgi_pass.ok_or_else(platform::install_hint)
}

#[tauri::command]
pub fn nginx_ssl_detect_php_fpm() -> PhpFpmDetection {
    detect()
}

#[tauri::command]
pub fn nginx_ssl_list_php_versions() -> Vec<PhpFpmVersion> {
    list_available_versions()
}
