pub mod platform;
pub mod render;

use serde::Serialize;

pub use platform::to_config_path;

/// A path as a single nginx directive argument: converted by `to_config_path`, then
/// double-quoted. Unquoted, a space splits it into two arguments — and on Windows the
/// app's own data directory sits under `C:/Users/<name>/`, where a space in the account
/// name is ordinary, so `include`, `ssl_certificate` and the logs all break at once.
/// Inside quotes nginx still treats `\` as an escape, so it and `"` are escaped too.
pub fn quoted_config_path(path: &str) -> String {
    let path = to_config_path(path).replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{path}\"")
}

#[derive(Debug, Serialize, Clone)]
pub struct NginxDetection {
    pub found: bool,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub config_path: Option<String>,
    /// The elevated package manager that can install nginx here — `apt`/`dnf`/`pacman` on
    /// Linux, `winget` on Windows — which lets the UI offer a real "Install nginx" button
    /// instead of just a copyable command. `None` on macOS, where brew must not run as
    /// root, and on a Windows old enough to predate winget.
    pub package_manager: Option<String>,
    /// How `binary_path` was found — see `locate`. Filled in by the caller.
    pub source: Option<super::locate::Source>,
}

/// Every place nginx is usually installed on this OS. `locate` tries the running
/// process and the remembered path first, and falls back to this.
pub fn find_binary() -> Option<String> {
    platform::find_binary()
}

/// Everything about the nginx at `binary_path`, or "not found" for `None`.
pub fn detect_with(binary_path: Option<String>) -> NginxDetection {
    let version = binary_path.as_deref().and_then(nginx_version);
    let config_path = binary_path.as_deref().and_then(nginx_config_path);
    NginxDetection {
        found: binary_path.is_some(),
        binary_path,
        version,
        config_path,
        package_manager: platform::package_manager(),
        source: None,
    }
}

pub(crate) fn package_manager() -> Option<String> {
    platform::package_manager()
}

fn nginx_version(bin: &str) -> Option<String> {
    let output = crate::modules::tools::command(bin).arg("-v").output().ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// The `--conf-path=` nginx was built with, made openable.
///
/// Splitting the reported value from the resolved one is what the Windows builds force:
/// they report `conf/nginx.conf`, relative to a prefix nginx only computes at run time, so
/// the raw value can't be opened or handed to `conf.d` discovery as-is.
fn nginx_config_path(bin: &str) -> Option<String> {
    let output = crate::modules::tools::command(bin).arg("-V").output().ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    let reported = text
        .split_whitespace()
        .find_map(|tok| tok.strip_prefix("--conf-path="))?;
    platform::resolve_config_path(reported, bin)
}

/// Every elevated operation needed to make nginx include DevsPro's site files, in the
/// order they must be applied. What that takes is genuinely different per OS — a drop-in
/// file in `conf.d` on Unix, a spliced `include` inside `http{}` on Windows — so the whole
/// list comes from `platform` rather than being assembled by the caller.
pub fn wire_in_sites_ops(
    config_path: &str,
    sites_glob: &str,
) -> Result<Vec<crate::elevate::Operation>, String> {
    platform::wire_in_sites_ops(config_path, sites_glob)
}

/// `nginx -t` followed by a reload, the pair that ends every site-changing manifest.
/// Grouped here because both need the same `-p` prefix, and forgetting it on Windows makes
/// the test report the user's entire configuration broken.
pub fn test_and_reload_ops(binary_path: &str) -> Vec<crate::elevate::Operation> {
    let prefix = platform::prefix_for(binary_path);
    vec![
        crate::elevate::Operation::RunNginxTest {
            binary_path: binary_path.to_string(),
            prefix: prefix.clone(),
        },
        crate::elevate::Operation::ReloadNginx {
            binary_path: binary_path.to_string(),
            prefix,
        },
    ]
}

#[tauri::command(async)]
pub fn nginx_ssl_detect_nginx(db: tauri::State<crate::db::Db>) -> NginxDetection {
    super::locate::detect_nginx(&db)
}

#[tauri::command(async)]
pub fn nginx_ssl_install_nginx(app: tauri::AppHandle) -> Result<String, String> {
    let manager = platform::package_manager()
        .ok_or_else(|| "no supported package manager (apt/dnf/pacman/winget) was found".to_string())?;

    let manifest = crate::elevate::OperationManifest {
        allowed_write_prefix: super::app_root_dir(&app)?.to_string_lossy().to_string(),
        operations: vec![crate::elevate::Operation::InstallPackage {
            manager,
            // The package is named differently in every repository — `nginx` for apt,
            // `nginxinc.nginx` for winget — so it comes from the platform, not from here.
            packages: platform::install_packages(),
        }],
    };

    let results = crate::elevate::run(&app, manifest)?;
    match results.first() {
        Some(r) if r.success => Ok(r.message.clone()),
        Some(r) => Err(r.message.clone()),
        None => Err("helper returned no results".to_string()),
    }
}

#[cfg(test)]
mod quoting_tests {
    use super::*;

    #[test]
    fn a_path_with_a_space_stays_one_argument() {
        assert_eq!(
            quoted_config_path("C:/Users/MD. Talha/AppData/Roaming/com.soab.devspro/nginx/sites/*.conf"),
            r#""C:/Users/MD. Talha/AppData/Roaming/com.soab.devspro/nginx/sites/*.conf""#
        );
    }

    #[test]
    fn a_quote_in_the_path_cannot_end_the_argument_early() {
        assert_eq!(quoted_config_path(r#"/srv/a"b"#), r#""/srv/a\"b""#);
    }
}
