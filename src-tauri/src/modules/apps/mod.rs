//! A catalog of common CLI/service dev tools with batch install. GUI apps
//! (editors/IDEs) are deliberately out of scope here — this catalog is CLI tools and
//! background services only. Three install mechanisms, each used for what actually needs
//! it:
//!
//! - **Package manager** (elevated): system packages — nginx, Docker, MySQL, Postgres,
//!   PHP, Python, git, build tools, Node.js. Every selected app using this method is
//!   handed to the helper in ONE manifest, so a batch of several costs exactly one
//!   admin-password prompt, not one each.
//! - **Git / npm-global** (unprivileged, on purpose): npm global packages (Claude Code,
//!   Codex CLI) install under whatever `npm`/node the invoking user's own PATH resolves to
//!   (commonly nvm-managed, in which case running these as root would write to the wrong
//!   place entirely — root's npm prefix, not the user's).
//! - **Binary** (unprivileged): a single static binary pulled from the vendor's release
//!   page into a user-writable directory. Used for cloudflared, which no distro ships —
//!   the vendor's own apt repo would mean permanently installing a GPG key and an apt
//!   source, and adding a privileged operation to the elevate helper to do it. One
//!   downloaded file the user can delete is a far smaller concession for one CLI tool.
//!
//! *Which* packages each of those means is per-OS and lives in `platform/`: apt names on
//! Linux, winget ids on Windows.

mod platform;

use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstallMethod {
    /// This OS's elevated package manager — apt/dnf/pacman on Linux, winget on Windows.
    PackageManager,
    NpmGlobal,
    /// Cloned from the vendor's repository. Only Linux and macOS use this, for nvm — the
    /// Windows Node version manager is a different program installed through winget — so
    /// the variant is legitimately unconstructed on some targets.
    #[allow(dead_code)]
    Git,
    /// A single prebuilt static binary fetched straight from the vendor's release page
    /// into a user-writable directory. Unprivileged like Git/NpmGlobal.
    Binary,
}

/// What an app *is*, independent of how any particular OS installs it. Kept in one place so
/// adding a tool is one entry rather than one per platform, and so the wording a user reads
/// doesn't drift between them.
struct CatalogEntry {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    /// The platform's plan may replace this where that OS does something different enough
    /// to make the shared wording wrong — see `InstallPlan::description`.
    description: &'static str,
    /// Looked up on PATH to decide whether it's already installed. `None` for entries with
    /// no binary of their own, like nvm.
    binary: Option<&'static str>,
    npm_package: Option<&'static str>,
}

const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "nginx",
        name: "nginx",
        category: "Web server",
        description: "HTTP server and reverse proxy.",
        binary: Some("nginx"),
        npm_package: None,
    },
    CatalogEntry {
        id: "docker",
        name: "Docker",
        category: "Containers",
        description: "Container runtime + Compose.",
        binary: Some("docker"),
        npm_package: None,
    },
    CatalogEntry {
        id: "node",
        name: "Node.js",
        category: "Runtime",
        description: "JavaScript runtime + npm (system default version — for a specific version, use nvm below).",
        binary: Some("node"),
        npm_package: None,
    },
    CatalogEntry {
        id: "nvm",
        name: "nvm",
        category: "Runtime",
        description: "Node Version Manager.",
        binary: None,
        npm_package: None,
    },
    CatalogEntry {
        id: "mysql",
        name: "MySQL",
        category: "Database",
        description: "MySQL server.",
        binary: Some("mysql"),
        npm_package: None,
    },
    CatalogEntry {
        id: "postgres",
        name: "PostgreSQL",
        category: "Database",
        description: "PostgreSQL server.",
        binary: Some("psql"),
        npm_package: None,
    },
    CatalogEntry {
        id: "php",
        name: "PHP",
        category: "Runtime",
        description: "PHP and its common extensions.",
        binary: Some("php"),
        npm_package: None,
    },
    CatalogEntry {
        id: "python",
        name: "Python",
        category: "Runtime",
        description: "Python 3 + pip + venv.",
        binary: Some("python3"),
        npm_package: None,
    },
    CatalogEntry {
        id: "git",
        name: "Git",
        category: "Essentials",
        description: "Version control.",
        binary: Some("git"),
        npm_package: None,
    },
    CatalogEntry {
        id: "build-essential",
        name: "Build tools",
        category: "Essentials",
        description: "A C/C++ toolchain — needed to build native modules for some npm/pip packages.",
        binary: Some("gcc"),
        npm_package: None,
    },
    CatalogEntry {
        id: "claude-code",
        name: "Claude Code",
        category: "AI coding tool",
        description: "Anthropic's CLI coding agent.",
        binary: Some("claude"),
        npm_package: Some("@anthropic-ai/claude-code"),
    },
    CatalogEntry {
        id: "codex",
        name: "Codex CLI",
        category: "AI coding tool",
        description: "OpenAI's CLI coding agent.",
        binary: Some("codex"),
        npm_package: Some("@openai/codex"),
    },
    CatalogEntry {
        id: "cloudflared",
        name: "cloudflared",
        category: "Networking",
        description: "Cloudflare Tunnel client — powers the Tunnel button on a site, exposing it over a public HTTPS URL. No account needed for quick tunnels.",
        binary: Some("cloudflared"),
        npm_package: None,
    },
];

/// Every catalogued id, whether or not this OS can install it. Used by the platform
/// backends' own tests to prove they cover the whole catalog.
#[cfg(test)]
pub(crate) fn catalog_ids() -> impl Iterator<Item = &'static str> {
    CATALOG.iter().map(|entry| entry.id)
}

/// One catalogued app together with how this OS installs it. Entries this OS has no way to
/// install are left out entirely rather than listed as permanently unavailable.
struct App {
    entry: &'static CatalogEntry,
    plan: platform::InstallPlan,
}

impl App {
    fn description(&self) -> &str {
        self.plan.description.unwrap_or(self.entry.description)
    }
}

fn catalog() -> Vec<App> {
    CATALOG
        .iter()
        .filter_map(|entry| platform::install_plan(entry.id).map(|plan| App { entry, plan }))
        .collect()
}

/// Vendor release assets are named with Go's architecture strings, not Rust's.
fn go_arch() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        "x86" => Ok("386"),
        "arm" => Ok("arm"),
        other => Err(format!("no prebuilt binary is published for this architecture ({other})")),
    }
}

/// Downloads a single static binary into the user's own bin directory, with no elevation.
///
/// The alternative — the vendor's package repository — would require a new privileged
/// operation in the elevate helper to add a signing key and a source entry, permanently
/// altering how every future package resolution on the machine works. That is a large,
/// hard-to-reverse concession for one CLI tool. This writes exactly one file the user can
/// delete.
fn install_binary_release(name: &str, url_template: &str) -> Result<String, String> {
    let arch = go_arch()?;
    let url = url_template.replace("{arch}", arch);
    let dir = platform::user_bin_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    // Windows will not execute a file without a recognised extension, whatever its
    // contents, so the name on disk is not always the name looked up on PATH.
    let final_path = dir.join(platform::executable_file_name(name));
    // Download beside the target, then rename: a half-written file is never
    // left sitting on PATH under the real name if the transfer dies midway.
    let temp_path = dir.join(format!(".{name}.download"));

    // curl rather than an HTTP crate, and this is not a Unix assumption — Windows 10 and 11
    // ship curl.exe in System32.
    let output = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            // Refuse a plaintext or downgraded hop, including on redirects.
            "--proto",
            "=https",
            "--tlsv1.2",
            "--output",
            &temp_path.to_string_lossy(),
            &url,
        ])
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;

    if !output.status.success() {
        let _ = std::fs::remove_file(&temp_path);
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() { format!("download failed from {url}") } else { err });
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("could not make {name} executable: {e}"))?;
    }

    std::fs::rename(&temp_path, &final_path)
        .map_err(|e| format!("could not move {name} into {}: {e}", dir.display()))?;

    // Prove the thing we just downloaded actually runs on this machine, rather
    // than reporting success for a 200 response that returned an error page.
    let version = Command::new(&final_path)
        .arg("--version")
        .output()
        .map_err(|e| format!("{name} was downloaded but would not run: {e}"))?;
    if !version.status.success() {
        return Err(format!("{name} was downloaded but exited non-zero when run"));
    }
    let reported = String::from_utf8_lossy(&version.stdout).trim().to_string();

    let mut message = format!("installed {} to {}", reported, final_path.display());
    if !crate::modules::tools::is_available(name) {
        message.push_str(&format!(
            " — note that {} is not on this app's PATH, so open a new terminal or add it",
            dir.display()
        ));
    }
    Ok(message)
}

fn is_installed(app: &App) -> bool {
    // Looking the catalog's binary up on PATH is right for most entries on most systems,
    // but not all of them: some tools install nothing under that name, and Windows ships
    // decoy executables that resolve whether or not the real thing is there. The platform
    // answers for those, and defers here for the rest.
    if let Some(known) = platform::is_installed_override(app.entry.id) {
        return known;
    }
    app.entry.binary.map(crate::modules::tools::is_available).unwrap_or(false)
}

fn method_label(method: InstallMethod) -> &'static str {
    match method {
        InstallMethod::PackageManager => platform::package_manager_label(),
        InstallMethod::NpmGlobal => "npm",
        InstallMethod::Git => "git",
        InstallMethod::Binary => "binary",
    }
}

#[derive(Debug, Serialize)]
pub struct AppStatus {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub method: String,
    pub homepage: Option<String>,
    pub installed: bool,
    pub can_auto_install: bool,
    /// Non-empty only where this OS actually has more than one real version available —
    /// empty means "installs whatever the default resolves to, no choice to offer".
    pub available_versions: Vec<String>,
}

#[tauri::command]
pub fn apps_list() -> Vec<AppStatus> {
    catalog()
        .into_iter()
        .map(|app| {
            // Anything still in the list has a plan for this OS, so it is installable;
            // entries this OS can't handle were filtered out by `catalog()`.
            let can_auto_install = true;
            let homepage = app
                .entry
                .npm_package
                .map(|p| format!("https://www.npmjs.com/package/{p}"))
                .or_else(|| {
                    // Point at the vendor's project, not the raw asset URL.
                    app.plan
                        .binary_url
                        .map(|_| "https://github.com/cloudflare/cloudflared".to_string())
                        .filter(|_| app.entry.id == "cloudflared")
                });
            AppStatus {
                id: app.entry.id.to_string(),
                name: app.entry.name.to_string(),
                category: app.entry.category.to_string(),
                description: app.description().to_string(),
                method: method_label(app.plan.method).to_string(),
                homepage,
                installed: is_installed(&app),
                can_auto_install,
                available_versions: platform::candidate_versions(app.entry.id),
            }
        })
        .collect()
}

fn semver_key(v: &str) -> (u32, u32, u32) {
    let mut parts = v.trim_start_matches('v').split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

/// Actual per-project Node version switching in this app stops at nvm + `.nvmrc` — DevsPro
/// doesn't run Node apps itself the way it runs PHP-backed sites, so there's no "Node site"
/// runtime to point at a version. What's real: which Node releases the version manager has
/// actually installed, and writing the project's `.nvmrc` so `nvm use` (run by the user, or
/// their editor's nvm integration) picks it up.
#[tauri::command]
pub fn apps_list_nvm_versions() -> Vec<String> {
    let mut versions = platform::installed_node_versions();
    versions.sort_by(|a, b| semver_key(b).cmp(&semver_key(a)));
    versions
}

#[tauri::command]
pub fn apps_write_nvmrc(project_path: String, version: String) -> Result<(), String> {
    let path = std::path::Path::new(&project_path);
    if !path.is_dir() {
        return Err(format!("{project_path} is not a directory"));
    }
    let version = version.trim();
    if version.is_empty() {
        return Err("a Node version is required".to_string());
    }
    std::fs::write(path.join(".nvmrc"), format!("{version}\n")).map_err(|e| e.to_string())
}

fn npm_install_global(package: &str) -> Result<String, String> {
    // Resolved rather than merely checked for: npm is `npm.cmd` on Windows, which a bare
    // `npm` cannot start — the check would pass and the install would fail. Unchanged on
    // Unix, where this is still the bare name.
    let npm = crate::modules::tools::program("npm")
        .ok_or("npm was not found on this account's PATH — install Node.js first")?;
    let output = Command::new(&npm)
        .args(["install", "-g", package])
        .output()
        .map_err(|e| format!("failed to run npm: {e}"))?;
    if output.status.success() {
        Ok(format!("installed {package}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct InstallOutcome {
    pub id: String,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSelection {
    pub id: String,
    pub version: Option<String>,
}

#[tauri::command]
pub fn apps_install_batch(app: tauri::AppHandle, selections: Vec<InstallSelection>) -> Vec<InstallOutcome> {
    let defs = catalog();
    let mut outcomes: Vec<InstallOutcome> = Vec::new();
    let mut managed_packages: Vec<String> = Vec::new();
    let mut managed_ids: Vec<String> = Vec::new();

    for selection in &selections {
        let id = &selection.id;
        let Some(def) = defs.iter().find(|d| d.entry.id == *id) else {
            continue;
        };
        let packages: Vec<String> = match &selection.version {
            Some(v) if !platform::candidate_versions(def.entry.id).is_empty() => {
                platform::versioned_packages(def.entry.id, v)
            }
            _ => def.plan.packages.clone(),
        };
        match def.plan.method {
            InstallMethod::PackageManager => {
                managed_packages.extend(packages);
                managed_ids.push(id.clone());
            }
            InstallMethod::Git => {
                let result = platform::install_nvm(selection.version.as_deref());
                outcomes.push(InstallOutcome {
                    id: id.clone(),
                    success: result.is_ok(),
                    message: result.unwrap_or_else(|e| e),
                });
            }
            InstallMethod::Binary => {
                let result = match def.plan.binary_url {
                    Some(url) => install_binary_release(def.entry.binary.unwrap_or(def.entry.id), url),
                    None => Err("no release URL configured for this entry".to_string()),
                };
                outcomes.push(InstallOutcome {
                    id: id.clone(),
                    success: result.is_ok(),
                    message: result.unwrap_or_else(|e| e),
                });
            }
            InstallMethod::NpmGlobal => {
                let result = match def.entry.npm_package {
                    Some(pkg) => npm_install_global(pkg),
                    None => Err("no npm package configured for this entry".to_string()),
                };
                outcomes.push(InstallOutcome {
                    id: id.clone(),
                    success: result.is_ok(),
                    message: result.unwrap_or_else(|e| e),
                });
            }
        }
    }

    if !managed_packages.is_empty() {
        managed_packages.sort();
        managed_packages.dedup();
        let manager = crate::modules::nginx_ssl::nginx::package_manager()
            .unwrap_or_else(|| platform::package_manager_label().to_string());
        let manifest = crate::elevate::OperationManifest {
            allowed_write_prefix: String::new(),
            operations: vec![crate::elevate::Operation::InstallPackage {
                manager,
                packages: managed_packages,
            }],
        };
        let (success, message) = match crate::elevate::run(&app, manifest) {
            Ok(results) => match results.first() {
                Some(r) => (r.success, r.message.clone()),
                None => (false, "helper returned no result".to_string()),
            },
            Err(e) => (false, e),
        };
        for id in managed_ids {
            outcomes.push(InstallOutcome {
                id,
                success,
                message: message.clone(),
            });
        }
    }

    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_entries_carry_everything_the_installer_needs() {
        // install_binary_release() needs both a URL and a filename to write. A
        // Binary entry missing either would only fail at click time.
        for app in catalog() {
            if app.plan.method == InstallMethod::Binary {
                assert!(app.plan.binary_url.is_some(), "{} has no binary_url", app.entry.id);
                assert!(app.entry.binary.is_some(), "{} has no binary name", app.entry.id);
                assert!(
                    app.plan.binary_url.unwrap().contains("{arch}"),
                    "{} must parameterise the architecture, or it only installs on one machine type",
                    app.entry.id
                );
            }
        }
    }

    #[test]
    fn binary_downloads_are_https_only() {
        // The whole point of fetching outside a signed package repo is that we
        // carry the transport guarantee ourselves.
        for app in catalog() {
            if let Some(url) = app.plan.binary_url {
                assert!(url.starts_with("https://"), "{} is not https", app.entry.id);
            }
        }
    }

    #[test]
    fn cloudflared_is_offered_and_matches_the_tunnel_module() {
        // The tunnel module looks for exactly this binary name on PATH; if the
        // catalog installs it under any other name the Tunnel button stays
        // disabled even after a successful install.
        let app = catalog()
            .into_iter()
            .find(|a| a.entry.id == "cloudflared")
            .expect("cloudflared should be in the catalog");
        assert_eq!(app.entry.binary, Some(crate::modules::tunnel::BINARY));
    }

    /// A package-manager entry naming no packages would produce an install that silently
    /// does nothing and still reports success.
    #[test]
    fn package_manager_entries_name_at_least_one_package() {
        for app in catalog() {
            if app.plan.method == InstallMethod::PackageManager {
                assert!(
                    !app.plan.packages.is_empty(),
                    "{} installs via the package manager but names no package",
                    app.entry.id
                );
            }
        }
    }

    /// Every entry the UI lists has to be one `apps_install_batch` can act on — an id with
    /// no match there is a row whose Install button does nothing.
    #[test]
    fn every_listed_app_is_installable_by_id() {
        let ids: Vec<String> = apps_list().into_iter().map(|status| status.id).collect();
        assert!(!ids.is_empty(), "this OS should offer some apps");
        for id in ids {
            assert!(
                catalog().iter().any(|app| app.entry.id == id),
                "{id} is listed but has no catalog entry to install"
            );
        }
    }

    #[test]
    fn go_arch_resolves_on_this_machine() {
        assert!(go_arch().is_ok(), "unsupported build architecture");
    }

    #[test]
    fn semver_key_orders_major_minor_patch() {
        assert!(semver_key("20.11.1") > semver_key("20.9.0"));
        assert!(semver_key("20.0.0") > semver_key("18.99.99"));
        assert!(semver_key("18.20.4") < semver_key("20.0.0"));
    }

    #[test]
    fn semver_key_strips_leading_v() {
        assert_eq!(semver_key("v20.11.1"), semver_key("20.11.1"));
    }

    #[test]
    fn semver_key_defaults_unparsable_parts_to_zero() {
        assert_eq!(semver_key("garbage"), (0, 0, 0));
        assert_eq!(semver_key("20"), (20, 0, 0));
        assert_eq!(semver_key("20.11"), (20, 11, 0));
    }
}
