//! Linux and macOS. Every distribution's Apache already includes a drop-in directory —
//! `conf.d` on RHEL/Fedora, `conf-enabled` on Debian/Ubuntu, `other` for the Apache macOS
//! ships — so, as with nginx, DevsPro writes one file there and never edits the main
//! config. Homebrew's Apache is the exception with no drop-in directory; there the
//! include is a marked block at the end of `httpd.conf`, where Apache accepts it.

use std::fs;
use std::path::{Path, PathBuf};

use crate::elevate::Operation;
use crate::modules::nginx_ssl::nginx::platform::{MARKER_BEGIN, MARKER_END};

pub fn find_binary() -> Option<String> {
    const CANDIDATES: &[&str] = &[
        "/usr/sbin/apache2ctl",
        "/usr/sbin/apachectl",
        "/usr/bin/apachectl",
        "/opt/homebrew/bin/apachectl",
        "/usr/local/bin/apachectl",
    ];
    if let Some(found) = CANDIDATES.iter().find(|p| Path::new(p).exists()) {
        return Some(found.to_string());
    }
    crate::modules::tools::find("apache2ctl").or_else(|| crate::modules::tools::find("apachectl"))
}

/// The control script already knows the root, so no `-d`.
pub fn server_root_arg(_binary_path: &str) -> Option<String> {
    None
}

/// What `-V` reports as `HTTPD_ROOT` is real on every Unix package.
pub fn fallback_server_root(httpd_root: Option<&str>, _binary_path: &str) -> Option<PathBuf> {
    httpd_root.map(PathBuf::from)
}

pub fn to_config_path(path: &str) -> String {
    path.to_string()
}

/// Where packaged Apache keeps its modules, for a module whose `LoadModule` line appears
/// nowhere in the config — Debian only names a module in `mods-available/` once it exists.
pub fn known_module_dirs() -> Vec<PathBuf> {
    [
        "/usr/lib/apache2/modules",
        "/usr/lib64/httpd/modules",
        "/usr/lib/httpd/modules",
        "/usr/libexec/apache2",
        "/opt/homebrew/lib/httpd/modules",
        "/usr/local/lib/httpd/modules",
    ]
    .iter()
    .map(PathBuf::from)
    .collect()
}

/// The drop-in directory next to the main config, but only one the config actually
/// includes — a `conf.d` that nothing reads would make the sites silently vanish.
fn drop_in_dir(config_path: &str) -> Option<PathBuf> {
    let config_dir = Path::new(config_path).parent()?;
    let text = fs::read_to_string(config_path).ok()?;
    ["conf.d", "conf-enabled", "other"].iter().find_map(|name| {
        let dir = config_dir.join(name);
        let included = text.lines().any(|line| {
            let line = line.trim();
            !line.starts_with('#')
                && line.to_ascii_lowercase().starts_with("include")
                && line.contains(&format!("{name}/"))
        });
        (included && dir.is_dir()).then_some(dir)
    })
}

pub fn wire_in_ops(config_path: &str, include_path: &str) -> Vec<Operation> {
    match drop_in_dir(config_path) {
        Some(dir) => vec![Operation::EnsureApacheConfDInclude {
            conf_d_dir: dir.to_string_lossy().to_string(),
            include_path: include_path.to_string(),
        }],
        None => vec![Operation::SyncMarkedBlock {
            file_path: config_path.to_string(),
            marker_begin: MARKER_BEGIN.to_string(),
            marker_end: MARKER_END.to_string(),
            block_lines: vec![super::super::include_line(include_path)],
        }],
    }
}

pub fn reload_ops(binary_path: &str, pid_file: Option<&str>) -> Vec<Operation> {
    vec![Operation::ReloadApache {
        binary_path: binary_path.to_string(),
        pid_file: pid_file.map(str::to_string),
    }]
}

/// The reload already happened inside the manifest.
pub fn after_apply(_binary_path: &str, _server_root: Option<&str>, _pid_file: Option<&str>) -> Result<Option<String>, String> {
    Ok(None)
}

/// The forms Apache's own `mod_proxy_fcgi` documentation gives: a socket needs the
/// `|fcgi://localhost/` suffix to name the protocol, a TCP listener does not.
pub fn fcgi_handler(fastcgi_pass: &str) -> String {
    match fastcgi_pass.strip_prefix("unix:") {
        Some(socket) => format!("proxy:unix:{socket}|fcgi://localhost/"),
        None => format!("proxy:fcgi://{fastcgi_pass}"),
    }
}

/// PHP-FPM reads the script path Apache derives itself.
pub fn script_filename_fix() -> Option<&'static str> {
    None
}

pub fn package_manager() -> Option<String> {
    crate::modules::nginx_ssl::nginx::package_manager()
}

/// The package is `apache2` on Debian, `httpd` on Fedora (whose TLS module is a separate
/// package) and `apache` on Arch.
pub fn install_packages(manager: &str) -> Vec<String> {
    match manager {
        "apt" => vec!["apache2".to_string()],
        "dnf" => vec!["httpd".to_string(), "mod_ssl".to_string()],
        _ => vec!["apache".to_string()],
    }
}

pub fn manual_install_hint() -> &'static str {
    "Install it manually, then recheck — e.g. sudo apt install apache2 on Debian/Ubuntu, or brew install httpd on macOS."
}
