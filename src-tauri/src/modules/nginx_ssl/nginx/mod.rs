pub mod render;

use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Serialize, Clone)]
pub struct NginxDetection {
    pub found: bool,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub config_path: Option<String>,
    /// Present only on Linux, when a supported package manager was found — lets the
    /// UI offer a real "Install nginx" button instead of just a copyable command.
    /// Never set on macOS (brew must not run as root) or Windows (no such step).
    pub linux_package_manager: Option<String>,
}

/// The exact marker block DevsPro adds to the user's real nginx.conf — one `include`
/// line, wrapped so it can be found and removed cleanly again. Never touches
/// anything else in that file.
pub const MARKER_BEGIN: &str = "# >>> devspro (do not edit this block manually) >>>";
pub const MARKER_END: &str = "# <<< devspro <<<";

/// Finds nginx's `conf.d` directory next to its main config — every standard
/// package (Debian/Ubuntu, RHEL/Fedora) already `include`s `conf.d/*.conf` from
/// inside `http{}`, so dropping one file there wires our sites in without ever
/// touching the user's real nginx.conf.
pub fn conf_d_dir(config_path: &str) -> Option<std::path::PathBuf> {
    let dir = Path::new(config_path).parent()?.join("conf.d");
    dir.is_dir().then_some(dir)
}

/// Builds the "make sure our sites are included" operation. Applied idempotently
/// on every add/remove (cheap to rewrite the same one line every time).
pub fn include_op(config_path: &str, sites_glob: &str) -> Result<crate::elevate::Operation, String> {
    let dir = conf_d_dir(config_path).ok_or_else(|| {
        format!(
            "could not find a conf.d directory next to {config_path} — this nginx installation doesn't use \
             the standard layout, so automatic site setup isn't supported for it yet"
        )
    })?;
    Ok(crate::elevate::Operation::EnsureNginxConfDInclude {
        conf_d_dir: dir.to_string_lossy().to_string(),
        sites_glob: sites_glob.to_string(),
    })
}

/// One-time cleanup for the earlier (buggy) approach, which appended the include
/// line at the very end of nginx.conf — landing *after* `http{}`'s closing brace,
/// where a `server{}` block is a syntax error. Safe to call unconditionally on
/// every add/remove: a no-op once the stray block is gone.
pub fn cleanup_legacy_include_block_op(config_path: &str) -> crate::elevate::Operation {
    crate::elevate::Operation::RemoveMarkedBlock {
        file_path: config_path.to_string(),
        marker_begin: MARKER_BEGIN.to_string(),
        marker_end: MARKER_END.to_string(),
    }
}

pub fn detect() -> NginxDetection {
    let binary_path = find_binary();
    let version = binary_path.as_deref().and_then(nginx_version);
    let config_path = binary_path.as_deref().and_then(nginx_config_path);
    let linux_package_manager = if cfg!(target_os = "linux") {
        linux_package_manager()
    } else {
        None
    };
    NginxDetection {
        found: binary_path.is_some(),
        binary_path,
        version,
        config_path,
        linux_package_manager,
    }
}

/// Which package manager (if any) is available on this Linux machine to install
/// nginx with. macOS's `brew` deliberately never goes through this path — Homebrew
/// refuses to run as root, so there's no elevated install step for it to plug into.
pub(crate) fn linux_package_manager() -> Option<String> {
    const CANDIDATES: &[(&str, &str)] = &[
        ("apt", "/usr/bin/apt-get"),
        ("dnf", "/usr/bin/dnf"),
        ("pacman", "/usr/bin/pacman"),
    ];
    CANDIDATES
        .iter()
        .find(|(_, path)| Path::new(path).exists())
        .map(|(name, _)| name.to_string())
}

fn find_binary() -> Option<String> {
    const CANDIDATES: &[&str] = &[
        "/usr/sbin/nginx",
        "/usr/bin/nginx",
        "/usr/local/sbin/nginx",
        "/usr/local/bin/nginx",
        "/opt/homebrew/bin/nginx",
        "/opt/homebrew/opt/nginx/bin/nginx",
    ];
    if let Some(found) = CANDIDATES.iter().find(|p| Path::new(p).exists()) {
        return Some(found.to_string());
    }
    let output = Command::new("which").arg("nginx").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn nginx_version(bin: &str) -> Option<String> {
    let output = Command::new(bin).arg("-v").output().ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn nginx_config_path(bin: &str) -> Option<String> {
    let output = Command::new(bin).arg("-V").output().ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    text.split_whitespace()
        .find_map(|tok| tok.strip_prefix("--conf-path=").map(str::to_string))
}

#[tauri::command(async)]
pub fn nginx_ssl_detect_nginx() -> NginxDetection {
    detect()
}

#[tauri::command(async)]
pub fn nginx_ssl_install_nginx(app: tauri::AppHandle) -> Result<String, String> {
    let manager = linux_package_manager()
        .ok_or_else(|| "no supported package manager (apt/dnf/pacman) was found".to_string())?;

    let manifest = crate::elevate::OperationManifest {
        allowed_write_prefix: super::app_root_dir(&app)?.to_string_lossy().to_string(),
        operations: vec![crate::elevate::Operation::InstallPackage {
            manager,
            packages: vec!["nginx".to_string()],
        }],
    };

    let results = crate::elevate::run(&app, manifest)?;
    match results.first() {
        Some(r) if r.success => Ok(r.message.clone()),
        Some(r) => Err(r.message.clone()),
        None => Err("helper returned no results".to_string()),
    }
}
