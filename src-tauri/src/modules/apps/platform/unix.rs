//! Linux/Debian-oriented, as this catalog has been from the start. Every package name,
//! version probe and install mechanism below is the one the module used before the
//! platform split — moved, not rewritten.

use std::path::PathBuf;
use std::process::Command;

use super::super::InstallMethod;
use super::InstallPlan;

pub fn package_manager_label() -> &'static str {
    "apt"
}

pub fn install_plan(id: &str) -> Option<InstallPlan> {
    use InstallMethod::{Binary, Git, NpmGlobal, PackageManager};
    Some(match id {
        "nginx" => InstallPlan::packages(PackageManager, &["nginx"]),
        "docker" => InstallPlan::packages(PackageManager, &["docker.io", "docker-compose"]),
        "node" => InstallPlan::packages(PackageManager, &["nodejs", "npm"]),
        "nvm" => InstallPlan::packages(Git, &[]).describe(
            "Node Version Manager — installs into ~/.nvm, deliberately no sudo. Pick a version to also install that Node release right away.",
        ),
        "mysql" => InstallPlan::packages(PackageManager, &["mysql-server"]).describe(
            "MySQL server (this machine's apt only has one installable version — no vendor repo configured for others).",
        ),
        "postgres" => InstallPlan::packages(PackageManager, &["postgresql", "postgresql-contrib"]),
        "php" => InstallPlan::packages(
            PackageManager,
            &[
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
        )
        .describe("PHP + FPM + common extensions."),
        "python" => InstallPlan::packages(PackageManager, &["python3", "python3-pip", "python3-venv"]),
        "git" => InstallPlan::packages(PackageManager, &["git"]),
        "build-essential" => InstallPlan::packages(PackageManager, &["build-essential"]).describe(
            "gcc/make and friends — needed to build native modules for some npm/pip packages.",
        ),
        "claude-code" => InstallPlan::packages(NpmGlobal, &[]),
        "codex" => InstallPlan::packages(NpmGlobal, &[]),
        // Deliberately not apt: cloudflared is in no distro repo, and the vendor's own
        // repo would mean permanently adding a GPG key and an apt source. It ships as one
        // static binary, so the unprivileged path is both simpler and far less invasive.
        "cloudflared" => InstallPlan {
            method: Binary,
            packages: Vec::new(),
            binary_url: Some(
                "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-{arch}",
            ),
            description: None,
        },
        _ => return None,
    })
}

pub fn home_dir() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "could not determine home directory".to_string())
}

/// `~/.local/bin` — already on PATH on every mainstream desktop distro, and the
/// conventional home for user-installed binaries.
pub fn user_bin_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".local/bin"))
}

/// Unix executables carry no extension.
pub fn executable_file_name(name: &str) -> String {
    name.to_string()
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
/// None here: apt ships one Docker per distribution release, so there is no
/// list worth fetching.
pub fn live_versions_package(_id: &str) -> Option<&'static str> {
    None
}

pub fn published_versions(_package: &str) -> Result<Vec<String>, String> {
    Err("this package manager doesn't list versions".to_string())
}

pub fn installed_version(_package: &str) -> Option<String> {
    None
}

pub fn candidate_versions(id: &str) -> Vec<String> {
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
pub fn versioned_packages(id: &str, version: &str) -> Vec<String> {
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

/// nvm is the only entry here that isn't found by looking for a binary on PATH: it is a
/// shell function sourced from a script, so nothing named `nvm` exists to find. Every other
/// catalogued tool installs a real executable under the name the catalog gives, so the
/// shared PATH lookup is the right answer.
pub fn is_installed_override(id: &str) -> Option<bool> {
    match id {
        "nvm" => Some(home_dir().map(|h| h.join(".nvm/nvm.sh").exists()).unwrap_or(false)),
        _ => None,
    }
}

pub fn install_nvm(node_version: Option<&str>) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins every apt package name and its order.
    ///
    /// These are what actually reaches `apt-get install` on a user's machine, and a typo or
    /// a dropped extension here is invisible until an install silently comes up short — the
    /// PHP list in particular is nine packages whose absence only shows up later as a
    /// missing extension. Written out in full rather than sampled, because the point is to
    /// notice any change at all.
    #[test]
    fn apt_package_lists_are_exactly_what_they_have_always_been() {
        let packages = |id: &str| install_plan(id).map(|plan| plan.packages).unwrap_or_default();

        assert_eq!(packages("nginx"), ["nginx"]);
        assert_eq!(packages("docker"), ["docker.io", "docker-compose"]);
        assert_eq!(packages("node"), ["nodejs", "npm"]);
        assert_eq!(packages("mysql"), ["mysql-server"]);
        assert_eq!(packages("postgres"), ["postgresql", "postgresql-contrib"]);
        assert_eq!(
            packages("php"),
            [
                "php",
                "php-fpm",
                "php-cli",
                "php-mysql",
                "php-pgsql",
                "php-xml",
                "php-curl",
                "php-mbstring",
                "php-zip",
            ]
        );
        assert_eq!(packages("python"), ["python3", "python3-pip", "python3-venv"]);
        assert_eq!(packages("git"), ["git"]);
        assert_eq!(packages("build-essential"), ["build-essential"]);

        // The unprivileged methods carry no package names at all.
        assert!(packages("nvm").is_empty());
        assert!(packages("claude-code").is_empty());
        assert!(packages("codex").is_empty());
        assert!(packages("cloudflared").is_empty());
    }

    /// Which mechanism each entry uses is a deliberate decision documented at length in the
    /// module header — nvm and the npm tools stay unprivileged on purpose, and cloudflared
    /// is a download rather than a vendor apt repo. Moving one to the elevated path would
    /// be a change in kind, not a detail.
    #[test]
    fn install_mechanisms_are_the_ones_the_module_argues_for() {
        let method = |id: &str| install_plan(id).map(|plan| plan.method);

        assert_eq!(method("nvm"), Some(InstallMethod::Git));
        assert_eq!(method("claude-code"), Some(InstallMethod::NpmGlobal));
        assert_eq!(method("codex"), Some(InstallMethod::NpmGlobal));
        assert_eq!(method("cloudflared"), Some(InstallMethod::Binary));
        for id in ["nginx", "docker", "node", "mysql", "postgres", "php", "python", "git", "build-essential"] {
            assert_eq!(method(id), Some(InstallMethod::PackageManager), "{id}");
        }
    }

    #[test]
    fn every_catalogued_app_can_be_installed() {
        for id in crate::modules::apps::catalog_ids() {
            assert!(install_plan(id).is_some(), "{id} has no install plan");
        }
    }

    /// The versioned substitution follows Debian's `php{v}-*` convention exactly; getting it
    /// wrong installs the default version while telling the user it installed theirs.
    #[test]
    fn versioned_php_keeps_every_extension() {
        assert_eq!(
            versioned_packages("php", "8.3"),
            [
                "php8.3",
                "php8.3-fpm",
                "php8.3-cli",
                "php8.3-mysql",
                "php8.3-pgsql",
                "php8.3-xml",
                "php8.3-curl",
                "php8.3-mbstring",
                "php8.3-zip",
            ]
        );
        assert_eq!(versioned_packages("postgres", "16"), ["postgresql-16"]);
        assert_eq!(versioned_packages("python", "3.12"), ["python3.12"]);
    }
}

/// Node releases nvm has actually installed, newest first. Directory names carry a
/// leading `v` that callers don't want.
pub fn installed_node_versions() -> Vec<String> {
    let Ok(home) = home_dir() else {
        return Vec::new();
    };
    std::fs::read_dir(home.join(".nvm/versions/node"))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(|s| s.trim_start_matches('v').to_string()))
                .collect()
        })
        .unwrap_or_default()
}
