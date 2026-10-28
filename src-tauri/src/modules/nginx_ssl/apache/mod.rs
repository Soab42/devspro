//! Apache as the web server behind DevsPro's sites — the counterpart of `nginx`, used when
//! the user picks Apache instead (see `web_server`). Detection, the elevated operations
//! that wire DevsPro's config in, test it and reload, and the install command all mirror
//! the nginx module so the Sites flows stay server-agnostic.
//!
//! What DevsPro owns, under the app data directory:
//!
//! - `apache/devspro.conf` — the one file Apache is pointed at: the modules the vhosts
//!   need, any `Listen` the user's config lacks, and an `IncludeOptional` of the sites.
//! - `apache/sites/<slug>.conf` — one vhost per site.
//!
//! The user's own config only ever gains one `Include` of `devspro.conf`: a drop-in file
//! where the distribution has a drop-in directory, a marked block otherwise.

pub mod config_scan;
pub mod platform;
pub mod render;

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The modules DevsPro's vhosts use, with the file each is built as. The last four are
/// only used inside `<IfModule>` guards, so they are loaded when found but their absence
/// is not an error.
const MODULES: &[(&str, &str, bool)] = &[
    ("ssl_module", "mod_ssl.so", true),
    ("proxy_module", "mod_proxy.so", true),
    ("proxy_http_module", "mod_proxy_http.so", true),
    ("proxy_fcgi_module", "mod_proxy_fcgi.so", true),
    ("headers_module", "mod_headers.so", false),
    ("alias_module", "mod_alias.so", false),
    ("dir_module", "mod_dir.so", false),
    ("rewrite_module", "mod_rewrite.so", false),
];

#[derive(Debug, Serialize, Clone)]
pub struct ApacheDetection {
    pub found: bool,
    /// What every invocation goes through: `apachectl`/`apache2ctl` on Unix, `httpd.exe`
    /// on Windows.
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub config_path: Option<String>,
    pub server_root: Option<String>,
    /// `-d` for every invocation — set on Windows only.
    #[serde(skip)]
    pub server_root_arg: Option<String>,
    pub pid_file: Option<String>,
    /// Whether the pid file exists right now — Apache is up.
    pub running: bool,
    pub package_manager: Option<String>,
    /// Shown when nothing was found and there is no install button to offer instead.
    pub install_hint: String,
    /// How `binary_path` was found — see `locate`. Filled in by the caller.
    pub source: Option<super::locate::Source>,
}

/// Every place Apache is usually installed on this OS. `locate` tries the running
/// process and the remembered path first, and falls back to this.
pub fn find_binary() -> Option<String> {
    platform::find_binary()
}

/// Everything about the Apache at `binary_path`, or "not found" for `None`.
pub fn detect_with(binary_path: Option<String>) -> ApacheDetection {
    let server_root_arg = binary_path.as_deref().and_then(platform::server_root_arg);
    let mut detection = ApacheDetection {
        found: binary_path.is_some(),
        binary_path: binary_path.clone(),
        version: None,
        config_path: None,
        server_root: None,
        server_root_arg: server_root_arg.clone(),
        pid_file: None,
        running: false,
        package_manager: platform::package_manager(),
        install_hint: platform::manual_install_hint().to_string(),
        source: None,
    };
    let Some(bin) = binary_path else {
        return detection;
    };

    let root = server_root_arg.as_deref();
    detection.version = run(&bin, root, &["-v"]).and_then(|text| parse_version(&text));
    let compiled = run(&bin, root, &["-V"]).map(|text| parse_compiled(&text)).unwrap_or_default();
    let run_cfg = run(&bin, root, &["-t", "-D", "DUMP_RUN_CFG"])
        .map(|text| parse_run_cfg(&text))
        .unwrap_or_default();

    let server_root = run_cfg
        .server_root
        .map(PathBuf::from)
        .or_else(|| platform::fallback_server_root(compiled.httpd_root.as_deref(), &bin));
    detection.config_path = compiled.config_file.map(|file| {
        let file = PathBuf::from(file);
        match (&server_root, file.is_absolute()) {
            (Some(root), false) => root.join(file).to_string_lossy().to_string(),
            _ => file.to_string_lossy().to_string(),
        }
    });
    detection.server_root = server_root.map(|r| r.to_string_lossy().to_string());
    detection.running = run_cfg.pid_file.as_deref().is_some_and(|f| Path::new(f).exists());
    detection.pid_file = run_cfg.pid_file;
    detection
}

/// stdout and stderr together: `httpd -v` prints to stdout, `-t -D DUMP_RUN_CFG` to
/// stderr, and which one varies between builds.
fn run(bin: &str, server_root_arg: Option<&str>, args: &[&str]) -> Option<String> {
    let mut command = crate::modules::tools::command(bin);
    if let Some(root) = server_root_arg {
        command.args(["-d", root]);
    }
    let output = command.args(args).output().ok()?;
    Some(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn parse_version(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("Server version:"))
        .map(|v| v.trim().to_string())
}

#[derive(Default)]
struct Compiled {
    httpd_root: Option<String>,
    config_file: Option<String>,
}

/// The `-D HTTPD_ROOT="..."` and `-D SERVER_CONFIG_FILE="..."` lines of `-V`.
fn parse_compiled(text: &str) -> Compiled {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix(&format!("-D {key}="))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
    };
    Compiled {
        httpd_root: value("HTTPD_ROOT"),
        config_file: value("SERVER_CONFIG_FILE"),
    }
}

#[derive(Default)]
struct RunCfg {
    server_root: Option<String>,
    pid_file: Option<String>,
}

/// `ServerRoot: "..."` and `PidFile: "..."` from `-t -D DUMP_RUN_CFG` — the values after
/// every `Define` and include, which is what Apache will actually use.
fn parse_run_cfg(text: &str) -> RunCfg {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix(&format!("{key}:"))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
    };
    RunCfg {
        server_root: value("ServerRoot"),
        pid_file: value("PidFile"),
    }
}

/// The modules loaded right now, statically or not, as `-M` lists them.
fn loaded_modules(detection: &ApacheDetection) -> Vec<String> {
    let Some(bin) = detection.binary_path.as_deref() else {
        return Vec::new();
    };
    run(bin, detection.server_root_arg.as_deref(), &["-M"])
        .map(|text| {
            text.lines()
                .filter_map(|line| line.split_whitespace().next())
                .filter(|name| name.ends_with("_module"))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The one line DevsPro adds to the user's config, quoted the same way as every path it
/// writes.
pub fn include_line(include_path: &str) -> String {
    format!("Include {}", render::quoted(include_path))
}

/// Renders `devspro.conf` for this Apache: which module files to load, which `Listen`
/// lines the user's config lacks. Errors only for a module the vhosts cannot work without
/// that is neither loaded nor anywhere on disk — on Fedora that means `mod_ssl` is not
/// installed, which is worth saying in so many words.
pub fn render_global_conf(detection: &ApacheDetection, global_conf_path: &Path, sites_glob: &str) -> Result<String, String> {
    let config_path = detection
        .config_path
        .as_deref()
        .ok_or("could not determine Apache's main config file")?;
    let server_root = PathBuf::from(detection.server_root.as_deref().unwrap_or_default());
    let scan = config_scan::scan(Path::new(config_path), &server_root, global_conf_path);

    let mut dirs = scan.module_dirs.clone();
    dirs.push(server_root.join("modules"));
    dirs.extend(platform::known_module_dirs());

    let loaded = loaded_modules(detection);
    let mut modules = Vec::new();
    let mut missing = Vec::new();
    for (name, file, required) in MODULES {
        match dirs.iter().map(|d| d.join(file)).find(|p| p.is_file()) {
            Some(path) => modules.push((name.to_string(), path.to_string_lossy().to_string())),
            None if *required && !loaded.iter().any(|m| m == name) => missing.push(*file),
            None => {}
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "Apache is missing {} — DevsPro's sites need {}. Install the module (on Fedora/RHEL: sudo dnf install mod_ssl) and try again.",
            missing.join(", "),
            if missing.len() == 1 { "it" } else { "them" }
        ));
    }

    let listen: Vec<u16> = [80, 443]
        .into_iter()
        .filter(|port| !scan.listen_ports.contains(port))
        .collect();
    Ok(render::render_global_conf(&modules, &listen, sites_glob))
}

#[tauri::command(async)]
pub fn nginx_ssl_detect_apache(db: tauri::State<crate::db::Db>) -> ApacheDetection {
    super::locate::detect_apache(&db)
}

#[tauri::command(async)]
pub fn nginx_ssl_install_apache(app: tauri::AppHandle) -> Result<String, String> {
    let manager = platform::package_manager()
        .ok_or_else(|| "no supported package manager (apt/dnf/pacman) was found".to_string())?;
    let manifest = crate::elevate::OperationManifest {
        allowed_write_prefix: super::app_root_dir(&app)?.to_string_lossy().to_string(),
        operations: vec![crate::elevate::Operation::InstallPackage {
            packages: platform::install_packages(&manager),
            manager,
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
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_line() {
        assert_eq!(
            parse_version("Server version: Apache/2.4.54 (Win64)\nApache Lounge VS16 Server built:   Jun 22 2022\n").as_deref(),
            Some("Apache/2.4.54 (Win64)")
        );
    }

    #[test]
    fn reads_root_and_config_from_compile_settings() {
        let compiled = parse_compiled(
            "Server compiled with....\n -D HTTPD_ROOT=\"/etc/apache2\"\n -D SERVER_CONFIG_FILE=\"apache2.conf\"\n",
        );
        assert_eq!(compiled.httpd_root.as_deref(), Some("/etc/apache2"));
        assert_eq!(compiled.config_file.as_deref(), Some("apache2.conf"));
    }

    #[test]
    fn reads_root_and_pid_file_from_the_run_config() {
        // Exactly what Laragon's Apache 2.4.54 prints.
        let cfg = parse_run_cfg(
            "ServerRoot: \"C:/laragon/bin/apache/httpd-2.4.54-win64-VS16\"\nMain DocumentRoot: \"C:/laragon/www\"\nPidFile: \"C:/laragon/bin/apache/httpd-2.4.54-win64-VS16/logs/httpd.pid\"\n",
        );
        assert_eq!(cfg.server_root.as_deref(), Some("C:/laragon/bin/apache/httpd-2.4.54-win64-VS16"));
        assert_eq!(
            cfg.pid_file.as_deref(),
            Some("C:/laragon/bin/apache/httpd-2.4.54-win64-VS16/logs/httpd.pid")
        );
    }

    #[test]
    fn the_include_line_is_quoted() {
        assert_eq!(
            include_line("/Users/me/Library/Application Support/devspro/apache/devspro.conf"),
            r#"Include "/Users/me/Library/Application Support/devspro/apache/devspro.conf""#
        );
    }
}
