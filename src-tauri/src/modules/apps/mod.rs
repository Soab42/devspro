//! A catalog of common CLI/service dev tools with batch install. Linux/Debian-
//! oriented for v1, consistent with the rest of this app's scope so far. GUI
//! apps (editors/IDEs) are deliberately out of scope here — this catalog is
//! CLI tools and background services only. Three install mechanisms, each used
//! for what actually needs it:
//!
//! - **Apt** (elevated): system packages — nginx, Docker, MySQL, Postgres, PHP,
//!   Python, git, build-essential, Node.js. Every selected app using this method
//!   is combined into ONE `apt-get install -y ...` call, so a batch of several
//!   apt-based tools costs exactly one admin-password prompt, not one each.
//! - **Git / npm-global** (unprivileged, on purpose): nvm installs into
//!   `~/.nvm` and npm global packages (Claude Code, Codex CLI) install under
//!   whatever `npm`/node the invoking user's own PATH resolves to (commonly
//!   nvm-managed, in which case running these as root would write to the wrong
//!   place entirely — root's npm prefix, not the user's).
//! - **Binary** (unprivileged): a single static binary pulled from the vendor's
//!   release page into `~/.local/bin`. Used for cloudflared, which no distro
//!   ships — the vendor's own apt repo would mean permanently installing a GPG
//!   key and an apt source, and adding a privileged operation to the elevate
//!   helper to do it. One downloaded file the user can delete is a far smaller
//!   concession for one CLI tool.

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy)]
enum InstallMethod {
    Apt,
    NpmGlobal,
    Git,
    /// A single prebuilt static binary fetched straight from the vendor's
    /// release page into `~/.local/bin`. Unprivileged like Git/NpmGlobal.
    Binary,
}

struct AppDefinition {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    description: &'static str,
    method: InstallMethod,
    /// Only meaningful for Apt.
    apt_packages: &'static [&'static str],
    /// Only meaningful for NpmGlobal.
    npm_package: Option<&'static str>,
    /// Only meaningful for Binary. `{arch}` is substituted with the Go-style
    /// architecture name the vendor uses in its release asset filenames.
    binary_url: Option<&'static str>,
    /// Checked with `which` — special-cased for a couple of entries below.
    binary: Option<&'static str>,
}

fn catalog() -> Vec<AppDefinition> {
    vec![
        AppDefinition {
            id: "nginx",
            name: "nginx",
            category: "Web server",
            description: "HTTP server and reverse proxy.",
            method: InstallMethod::Apt,
            apt_packages: &["nginx"],
            npm_package: None,
            binary_url: None,
            binary: Some("nginx"),
        },
        AppDefinition {
            id: "docker",
            name: "Docker",
            category: "Containers",
            description: "Container runtime + Compose.",
            method: InstallMethod::Apt,
            apt_packages: &["docker.io", "docker-compose"],
            npm_package: None,
            binary_url: None,
            binary: Some("docker"),
        },
        AppDefinition {
            id: "node",
            name: "Node.js",
            category: "Runtime",
            description: "JavaScript runtime + npm (system default version — for a specific version, use nvm below).",
            method: InstallMethod::Apt,
            apt_packages: &["nodejs", "npm"],
            npm_package: None,
            binary_url: None,
            binary: Some("node"),
        },
        AppDefinition {
            id: "nvm",
            name: "nvm",
            category: "Runtime",
            description: "Node Version Manager — installs into ~/.nvm, deliberately no sudo. Pick a version to also install that Node release right away.",
            method: InstallMethod::Git,
            apt_packages: &[],
            npm_package: None,
            binary_url: None,
            binary: None,
        },
        AppDefinition {
            id: "mysql",
            name: "MySQL",
            category: "Database",
            description: "MySQL server (this machine's apt only has one installable version — no vendor repo configured for others).",
            method: InstallMethod::Apt,
            apt_packages: &["mysql-server"],
            npm_package: None,
            binary_url: None,
            binary: Some("mysql"),
        },
        AppDefinition {
            id: "postgres",
            name: "PostgreSQL",
            category: "Database",
            description: "PostgreSQL server.",
            method: InstallMethod::Apt,
            apt_packages: &["postgresql", "postgresql-contrib"],
            npm_package: None,
            binary_url: None,
            binary: Some("psql"),
        },
        AppDefinition {
            id: "php",
            name: "PHP",
            category: "Runtime",
            description: "PHP + FPM + common extensions.",
            method: InstallMethod::Apt,
            apt_packages: &[
                "php",
                "php-fpm",
                "php-cli",
                "php-mysql",
                "php-pgsql",
                "php-xml",
                "php-curl",
                "php-mbstring",
                "php-zip",
            ],
            npm_package: None,
            binary_url: None,
            binary: Some("php"),
        },
        AppDefinition {
            id: "python",
            name: "Python",
            category: "Runtime",
            description: "Python 3 + pip + venv.",
            method: InstallMethod::Apt,
            apt_packages: &["python3", "python3-pip", "python3-venv"],
            npm_package: None,
            binary_url: None,
            binary: Some("python3"),
        },
        AppDefinition {
            id: "git",
            name: "Git",
            category: "Essentials",
            description: "Version control.",
            method: InstallMethod::Apt,
            apt_packages: &["git"],
            npm_package: None,
            binary_url: None,
            binary: Some("git"),
        },
        AppDefinition {
            id: "build-essential",
            name: "Build tools",
            category: "Essentials",
            description: "gcc/make and friends — needed to build native modules for some npm/pip packages.",
            method: InstallMethod::Apt,
            apt_packages: &["build-essential"],
            npm_package: None,
            binary_url: None,
            binary: Some("gcc"),
        },
        AppDefinition {
            id: "claude-code",
            name: "Claude Code",
            category: "AI coding tool",
            description: "Anthropic's CLI coding agent.",
            method: InstallMethod::NpmGlobal,
            apt_packages: &[],
            npm_package: Some("@anthropic-ai/claude-code"),
            binary_url: None,
            binary: Some("claude"),
        },
        AppDefinition {
            id: "codex",
            name: "Codex CLI",
            category: "AI coding tool",
            description: "OpenAI's CLI coding agent.",
            method: InstallMethod::NpmGlobal,
            apt_packages: &[],
            npm_package: Some("@openai/codex"),
            binary_url: None,
            binary: Some("codex"),
        },
        AppDefinition {
            id: "cloudflared",
            name: "cloudflared",
            category: "Networking",
            description: "Cloudflare Tunnel client — powers the Tunnel button on a site, exposing it over a public HTTPS URL. No account needed for quick tunnels.",
            method: InstallMethod::Binary,
            apt_packages: &[],
            npm_package: None,
            // Deliberately not apt: cloudflared is in no distro repo, and the
            // vendor's own repo would mean permanently adding a GPG key and an
            // apt source. It ships as one static binary, so the unprivileged
            // path is both simpler and far less invasive.
            binary_url: Some(
                "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-{arch}",
            ),
            binary: Some("cloudflared"),
        },
    ]
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

/// `~/.local/bin` — already on PATH on every mainstream desktop distro, and the
/// conventional home for user-installed binaries.
fn user_bin_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".local/bin"))
}

/// Downloads a single static binary into `~/.local/bin`, with no elevation.
///
/// The alternative — Cloudflare's apt repository — would require a new
/// privileged operation in the elevate helper to add a GPG key and a sources
/// entry, permanently altering how every future apt run on the machine
/// resolves packages. That is a large, hard-to-reverse concession for one CLI
/// tool. This writes exactly one file the user can delete.
fn install_binary_release(name: &str, url_template: &str) -> Result<String, String> {
    let arch = go_arch()?;
    let url = url_template.replace("{arch}", arch);
    let dir = user_bin_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let final_path = dir.join(name);
    // Download beside the target, then rename: a half-written file is never
    // left sitting on PATH under the real name if the transfer dies midway.
    let temp_path = dir.join(format!(".{name}.download"));

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
    if !which(name) {
        message.push_str(&format!(
            " — note that {} is not on this app's PATH, so open a new terminal or add it",
            dir.display()
        ));
    }
    Ok(message)
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var("HOME").map(PathBuf::from).map_err(|_| "could not determine home directory".to_string())
}

fn is_installed(def: &AppDefinition) -> bool {
    if def.id == "nvm" {
        return home_dir().map(|h| h.join(".nvm/nvm.sh").exists()).unwrap_or(false);
    }
    def.binary.map(which).unwrap_or(false)
}

fn apt_candidate_available(pkg: &str) -> bool {
    Command::new("apt-cache")
        .args(["policy", pkg])
        .output()
        .map(|o| {
            if !o.status.success() {
                return false;
            }
            let text = String::from_utf8_lossy(&o.stdout);
            text.contains("Candidate:") && !text.contains("Candidate: (none)")
        })
        .unwrap_or(false)
}

/// Real per-version apt packages only exist for a few of these (PHP via the
/// ondrej PPA convention, PostgreSQL via PGDG, Python via deadsnakes or the
/// distro's own backport) — and only if the machine actually has the relevant
/// repo configured. Detected live rather than assumed, so a plain Ubuntu box
/// with none of those PPAs just shows the one default version with no dropdown.
/// MySQL/Node/Docker/etc. have no standard multi-version apt path without
/// adding a new repo, so they're left single-version.
fn candidate_versions(id: &str) -> Vec<String> {
    match id {
        "php" => ["8.5", "8.4", "8.3", "8.2", "8.1", "8.0", "7.4"]
            .iter()
            .filter(|v| apt_candidate_available(&format!("php{v}-common")))
            .map(|v| v.to_string())
            .collect(),
        "postgres" => ["17", "16", "15", "14", "13"]
            .iter()
            .filter(|v| apt_candidate_available(&format!("postgresql-{v}")))
            .map(|v| v.to_string())
            .collect(),
        "python" => ["3.13", "3.12", "3.11", "3.10", "3.9"]
            .iter()
            .filter(|v| apt_candidate_available(&format!("python{v}")))
            .map(|v| v.to_string())
            .collect(),
        // Not apt-detected — nvm doesn't touch apt at all. These are just the
        // current Node major release lines; nvm resolves whichever's latest
        // patch within that major from its own remote index at install time,
        // so there's no staleness risk in naming them here.
        "nvm" => vec!["22".to_string(), "20".to_string(), "18".to_string()],
        _ => vec![],
    }
}

/// Substitutes a chosen version into that app's package list — same Debian PHP
/// packaging convention (`php{v}-*`) for every extension; just the one package
/// for PostgreSQL/Python, since versioned pip/venv/contrib companions aren't
/// reliably named the same way across every repo that provides them.
fn versioned_packages(id: &str, version: &str) -> Vec<String> {
    match id {
        "php" => {
            const EXTENSIONS: &[&str] = &["", "-fpm", "-cli", "-mysql", "-pgsql", "-xml", "-curl", "-mbstring", "-zip"];
            EXTENSIONS.iter().map(|ext| format!("php{version}{ext}")).collect()
        }
        "postgres" => vec![format!("postgresql-{version}")],
        "python" => vec![format!("python{version}")],
        _ => vec![],
    }
}

fn method_label(method: InstallMethod) -> &'static str {
    match method {
        InstallMethod::Apt => "apt",
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
    /// Non-empty only where this machine's apt actually has more than one real
    /// version available (see `candidate_versions`) — empty means "installs
    /// whatever the default package resolves to, no choice to offer".
    pub available_versions: Vec<String>,
}

/// Probes every catalog entry in parallel.
///
/// This is the route's whole load cost, and it is all subprocesses: one `which`
/// per app to answer "installed?", plus an `apt-cache policy` per candidate
/// version (7 for PHP, 5 each for PostgreSQL and Python). Run in series that is
/// ~1s of dead time on every visit to the Apps page — measured at 0.96s for the
/// 17 `apt-cache` calls alone on a warm cache — and a sync `#[tauri::command]`
/// blocks, so the window is unresponsive for the duration.
///
/// Scoped threads rather than a runtime or a new dependency: the work is pure
/// process-spawn latency, so it parallelises almost perfectly, and the borrowed
/// catalog needs no `'static` bound or cloning. Order is preserved by writing
/// each result into its own slot rather than collecting as threads finish.
#[tauri::command]
pub fn apps_list() -> Vec<AppStatus> {
    let defs = catalog();
    // Built with a loop, not `vec![None; n]` — that needs `AppStatus: Clone`,
    // which it has no other reason to be.
    let mut slots: Vec<Option<AppStatus>> = (0..defs.len()).map(|_| None).collect();

    std::thread::scope(|scope| {
        for (slot, def) in slots.iter_mut().zip(defs.iter()) {
            scope.spawn(move || {
                *slot = Some(app_status(def));
            });
        }
    });

    // Every slot was written by exactly one thread that has since been joined.
    slots.into_iter().flatten().collect()
}

fn app_status(def: &AppDefinition) -> AppStatus {
    // Every remaining method is unprivileged-or-elevated-but-always-runnable;
    // there's no "manual, no installer" entry left in this CLI-only catalog.
    let can_auto_install = true;
    let homepage = def
        .npm_package
        .map(|p| format!("https://www.npmjs.com/package/{p}"))
        .or_else(|| {
            // Point at the vendor's project, not the raw asset URL.
            def.binary_url
                .map(|_| "https://github.com/cloudflare/cloudflared".to_string())
                .filter(|_| def.id == "cloudflared")
        });
    AppStatus {
        id: def.id.to_string(),
        name: def.name.to_string(),
        category: def.category.to_string(),
        description: def.description.to_string(),
        method: method_label(def.method).to_string(),
        homepage,
        installed: is_installed(def),
        can_auto_install,
        available_versions: candidate_versions(def.id),
    }
}

fn install_nvm(node_version: Option<&str>) -> Result<String, String> {
    let home = home_dir()?;
    let nvm_dir = home.join(".nvm");
    let mut messages = Vec::new();

    if nvm_dir.join("nvm.sh").exists() {
        messages.push("nvm is already installed".to_string());
    } else {
        let output = Command::new("git")
            .args(["clone", "--depth", "1", "https://github.com/nvm-sh/nvm.git", &nvm_dir.to_string_lossy()])
            .output()
            .map_err(|e| format!("failed to run git: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        messages.push(format!("cloned nvm into {}", nvm_dir.display()));
    }

    match node_version {
        Some(version) => {
            let script = format!(
                "export NVM_DIR=\"{}\"; . \"$NVM_DIR/nvm.sh\"; nvm install {version} && nvm alias default {version}",
                nvm_dir.display()
            );
            let output = Command::new("bash")
                .args(["-c", &script])
                .output()
                .map_err(|e| format!("failed to run nvm install: {e}"))?;
            if output.status.success() {
                messages.push(format!("installed Node {version} via nvm and set it as default"));
            } else {
                let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                messages.push(format!("nvm is set up, but installing Node {version} failed: {err}"));
            }
        }
        None => messages.push("open a new terminal (or run `source ~/.nvm/nvm.sh`) to start using it".to_string()),
    }

    Ok(messages.join(" — "))
}

/// Actual per-project Node version switching in this app stops at nvm + `.nvmrc`
/// — DevsPro doesn't run Node apps itself the way it runs PHP-FPM-backed sites,
/// so there's no "Node site" runtime to point at a version. What's real: which
/// Node releases nvm has actually installed under `~/.nvm/versions/node`, and
/// writing the project's `.nvmrc` so `nvm use` (run by the user, or their editor's
/// nvm integration) picks it up.
fn nvm_versions_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".nvm/versions/node"))
}

fn semver_key(v: &str) -> (u32, u32, u32) {
    let mut parts = v.trim_start_matches('v').split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

fn list_installed_nvm_versions() -> Vec<String> {
    let Ok(dir) = nvm_versions_dir() else {
        return Vec::new();
    };
    let mut versions: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(|s| s.trim_start_matches('v').to_string()))
                .collect()
        })
        .unwrap_or_default();
    versions.sort_by(|a, b| semver_key(b).cmp(&semver_key(a)));
    versions
}

#[tauri::command]
pub fn apps_list_nvm_versions() -> Vec<String> {
    list_installed_nvm_versions()
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
    if !which("npm") {
        return Err("npm was not found on this account's PATH — install Node.js first".to_string());
    }
    let output = Command::new("npm")
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
    let mut apt_pkgs: Vec<String> = Vec::new();
    let mut apt_ids: Vec<String> = Vec::new();

    for selection in &selections {
        let id = &selection.id;
        let Some(def) = defs.iter().find(|d| d.id == *id) else {
            continue;
        };
        let packages: Vec<String> = match &selection.version {
            Some(v) if !candidate_versions(def.id).is_empty() => versioned_packages(def.id, v),
            _ => def.apt_packages.iter().map(|p| p.to_string()).collect(),
        };
        match def.method {
            InstallMethod::Apt => {
                apt_pkgs.extend(packages);
                apt_ids.push(id.clone());
            }
            InstallMethod::Git => {
                let result = install_nvm(selection.version.as_deref());
                outcomes.push(InstallOutcome {
                    id: id.clone(),
                    success: result.is_ok(),
                    message: result.unwrap_or_else(|e| e),
                });
            }
            InstallMethod::Binary => {
                let result = match def.binary_url {
                    Some(url) => install_binary_release(def.binary.unwrap_or(def.id), url),
                    None => Err("no release URL configured for this entry".to_string()),
                };
                outcomes.push(InstallOutcome {
                    id: id.clone(),
                    success: result.is_ok(),
                    message: result.unwrap_or_else(|e| e),
                });
            }
            InstallMethod::NpmGlobal => {
                let result = match def.npm_package {
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

    if !apt_pkgs.is_empty() {
        apt_pkgs.sort();
        apt_pkgs.dedup();
        let manager = crate::modules::nginx_ssl::nginx::linux_package_manager().unwrap_or_else(|| "apt".to_string());
        let manifest = crate::elevate::OperationManifest {
            allowed_write_prefix: String::new(),
            operations: vec![crate::elevate::Operation::InstallPackage {
                manager,
                packages: apt_pkgs,
            }],
        };
        let (success, message) = match crate::elevate::run(&app, manifest) {
            Ok(results) => match results.first() {
                Some(r) => (r.success, r.message.clone()),
                None => (false, "helper returned no result".to_string()),
            },
            Err(e) => (false, e),
        };
        for id in apt_ids {
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
        for def in catalog() {
            if matches!(def.method, InstallMethod::Binary) {
                assert!(def.binary_url.is_some(), "{} has no binary_url", def.id);
                assert!(def.binary.is_some(), "{} has no binary name", def.id);
                assert!(
                    def.binary_url.unwrap().contains("{arch}"),
                    "{} must parameterise the architecture, or it only installs on one machine type",
                    def.id
                );
            }
        }
    }

    #[test]
    fn binary_downloads_are_https_only() {
        // The whole point of fetching outside a signed package repo is that we
        // carry the transport guarantee ourselves.
        for def in catalog() {
            if let Some(url) = def.binary_url {
                assert!(url.starts_with("https://"), "{} is not https", def.id);
            }
        }
    }

    #[test]
    fn cloudflared_is_offered_and_matches_the_tunnel_module() {
        // The tunnel module looks for exactly this binary name on PATH; if the
        // catalog installs it under any other name the Tunnel button stays
        // disabled even after a successful install.
        let def = catalog()
            .into_iter()
            .find(|d| d.id == "cloudflared")
            .expect("cloudflared should be in the catalog");
        assert_eq!(def.binary, Some(crate::modules::tunnel::BINARY));
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
