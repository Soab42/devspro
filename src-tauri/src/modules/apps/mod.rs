//! A catalog of common CLI/service dev tools with batch install. Linux/Debian-
//! oriented for v1, consistent with the rest of this app's scope so far. GUI
//! apps (editors/IDEs) are deliberately out of scope here — this catalog is
//! CLI tools and background services only. Two install mechanisms, each used
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

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy)]
enum InstallMethod {
    Apt,
    NpmGlobal,
    Git,
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
            binary: Some("codex"),
        },
    ]
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

#[tauri::command]
pub fn apps_list() -> Vec<AppStatus> {
    catalog()
        .into_iter()
        .map(|def| {
            // Every remaining method is unprivileged-or-elevated-but-always-runnable;
            // there's no "manual, no installer" entry left in this CLI-only catalog.
            let can_auto_install = true;
            let homepage = def.npm_package.map(|p| format!("https://www.npmjs.com/package/{p}"));
            AppStatus {
                id: def.id.to_string(),
                name: def.name.to_string(),
                category: def.category.to_string(),
                description: def.description.to_string(),
                method: method_label(def.method).to_string(),
                homepage,
                installed: is_installed(&def),
                can_auto_install,
                available_versions: candidate_versions(def.id),
            }
        })
        .collect()
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
pub fn apps_install_batch(selections: Vec<InstallSelection>) -> Vec<InstallOutcome> {
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
        let (success, message) = match crate::elevate::run(manifest) {
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
