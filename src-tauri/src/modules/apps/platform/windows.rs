//! Windows, on `winget`. Every identifier below was checked against `winget show --id …
//! --exact --source winget` on a real machine rather than guessed — several of the obvious
//! guesses (`nginx.nginx`, `PostgreSQL.PostgreSQL`, `PHP.PHP`) do not exist, and a wrong id
//! only fails at the moment the user clicks Install.
//!
//! Two shapes differ from apt in ways that matter here:
//!
//! - **Versions are separate packages, not version-suffixed names.** apt has
//!   `php8.3-fpm`, `php8.3-cli` and friends under one version; winget has a single
//!   `PHP.PHP.8.3`. That maps cleanly onto the existing version dropdown — it is only the
//!   substitution rule that changes.
//! - **The catalog is the same everywhere.** apt versions are probed with `apt-cache`
//!   because whether `php8.3-common` exists depends on which PPAs *this machine* has
//!   configured. winget's community repository is identical for every machine, so there is
//!   nothing machine-specific to discover and the lists below are stated directly. They
//!   need updating when a new PHP or PostgreSQL ships — exactly like the candidate lists
//!   the Linux backend filters.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;

use super::super::InstallMethod;
use super::InstallPlan;

/// Keeps detection from flashing a console window each time the Apps page opens.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn package_manager_label() -> &'static str {
    "winget"
}

/// Newest of each versioned family, used when the user picks no specific version.
const DEFAULT_PHP: &str = "8.4";
const DEFAULT_POSTGRES: &str = "17";
const DEFAULT_PYTHON: &str = "3.12";

pub fn install_plan(id: &str) -> Option<InstallPlan> {
    use InstallMethod::{Binary, NpmGlobal, PackageManager};
    Some(match id {
        // `nginxinc.nginx` is nginx Inc's own build. (`nginx.nginx` — the id that mirrors
        // every other vendor's naming — does not exist; `freenginx.nginx` is the fork.)
        "nginx" => InstallPlan::packages(PackageManager, &["nginxinc.nginx"]),
        "docker" => InstallPlan::packages(PackageManager, &["Docker.DockerDesktop"]).describe(
            "Docker Desktop — container runtime and Compose. Needs to be running before the Docker and Databases pages can do anything.",
        ),
        "node" => InstallPlan::packages(PackageManager, &["OpenJS.NodeJS"]),
        // nvm-windows is a different program from the nvm this catalog installs on Linux —
        // not a shell script that can be cloned, but an installed application. So it comes
        // from winget like anything else, and the Node release itself is left to the user:
        // `nvm install` needs the PATH entry the installer just added, which this process
        // will not see until it is restarted.
        "nvm" => InstallPlan::packages(PackageManager, &["CoreyButler.NVMforWindows"]).describe(
            "nvm for Windows — Node Version Manager. Once it's installed, open a new terminal and run `nvm install 22` to add a Node release.",
        ),
        "mysql" => InstallPlan::packages(PackageManager, &["Oracle.MySQL"]).describe("MySQL server."),
        "postgres" => InstallPlan::packages(
            PackageManager,
            &[postgres_package(DEFAULT_POSTGRES)],
        ),
        // No FPM here: Windows PHP has no process manager, and DevsPro runs `php-cgi.exe`
        // itself for Laravel sites (see nginx_ssl::php_fpm). The winget package is the
        // whole distribution, extensions included, so there is no list to enumerate.
        "php" => InstallPlan::packages(PackageManager, &[php_package(DEFAULT_PHP)])
            .describe("PHP, including the php-cgi.exe that DevsPro runs for Laravel sites."),
        "python" => InstallPlan::packages(PackageManager, &[python_package(DEFAULT_PYTHON)]),
        "git" => InstallPlan::packages(PackageManager, &["Git.Git"]),
        // The Windows counterpart of build-essential: the MSVC toolchain that native npm
        // and pip modules compile against.
        "build-essential" => {
            InstallPlan::packages(PackageManager, &["Microsoft.VisualStudio.2022.BuildTools"]).describe(
                "Visual Studio Build Tools — the MSVC compiler and Windows SDK, needed to build native modules for some npm/pip packages.",
            )
        }
        "claude-code" => InstallPlan::packages(NpmGlobal, &[]),
        "codex" => InstallPlan::packages(NpmGlobal, &[]),
        // Kept as a plain download even though `Cloudflare.cloudflared` exists in winget,
        // for the same reason the Linux backend avoids a vendor apt repo: this is one
        // static binary the user can delete, and it needs no elevation at all.
        "cloudflared" => InstallPlan {
            method: Binary,
            packages: Vec::new(),
            binary_url: Some(
                "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-{arch}.exe",
            ),
            description: None,
        },
        _ => return None,
    })
}

fn php_package(version: &str) -> &'static str {
    // Returned as a `&'static str` so it can sit in the same array shape the other entries
    // use; the set is closed and known, so a lookup beats formatting a string here.
    match version {
        "8.5" => "PHP.PHP.8.5",
        "8.4" => "PHP.PHP.8.4",
        "8.3" => "PHP.PHP.8.3",
        "8.2" => "PHP.PHP.8.2",
        "8.1" => "PHP.PHP.8.1",
        _ => "PHP.PHP.8.4",
    }
}

fn postgres_package(version: &str) -> &'static str {
    match version {
        "18" => "PostgreSQL.PostgreSQL.18",
        "17" => "PostgreSQL.PostgreSQL.17",
        "16" => "PostgreSQL.PostgreSQL.16",
        "15" => "PostgreSQL.PostgreSQL.15",
        _ => "PostgreSQL.PostgreSQL.17",
    }
}

fn python_package(version: &str) -> &'static str {
    match version {
        "3.13" => "Python.Python.3.13",
        "3.12" => "Python.Python.3.12",
        "3.11" => "Python.Python.3.11",
        "3.10" => "Python.Python.3.10",
        _ => "Python.Python.3.12",
    }
}

pub fn home_dir() -> Result<PathBuf, String> {
    // `USERPROFILE` is the Windows equivalent of `HOME`, which is only set here when
    // something like Git Bash has been through the environment first — and when it is, it
    // may point somewhere else entirely.
    std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .map_err(|_| "could not determine home directory".to_string())
}

/// Windows has no `~/.local/bin` convention and nothing user-writable on PATH by default,
/// so this is a directory of DevsPro's own under `%LOCALAPPDATA%`. It will not be on PATH;
/// the caller already says so when the tool it just installed can't be found there.
pub fn user_bin_dir() -> Result<PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|_| home_dir().map(|home| home.join("AppData").join("Local")))
        .map_err(|_: String| "could not determine the local application data directory".to_string())?;
    Ok(local.join("DevsPro").join("bin"))
}

/// Windows will not run a file without a recognised extension, whatever its contents.
pub fn executable_file_name(name: &str) -> String {
    if name.ends_with(".exe") {
        name.to_string()
    } else {
        format!("{name}.exe")
    }
}

/// Stated rather than probed — see this module's header. `winget` publishes the same
/// catalog to every machine, so there is no per-machine repository configuration to
/// discover the way `apt-cache` discovers a PPA.
pub fn candidate_versions(id: &str) -> Vec<String> {
    match id {
        "php" => ["8.5", "8.4", "8.3", "8.2", "8.1"].iter().map(|v| v.to_string()).collect(),
        "postgres" => ["18", "17", "16", "15"].iter().map(|v| v.to_string()).collect(),
        "python" => ["3.13", "3.12", "3.11", "3.10"].iter().map(|v| v.to_string()).collect(),
        // No dropdown for nvm here, unlike Linux: nvm-windows is installed by an installer
        // that adds itself to PATH, and this process cannot see that PATH entry until it
        // restarts — so offering to install a Node release in the same click would promise
        // something this code cannot deliver.
        _ => vec![],
    }
}

pub fn versioned_packages(id: &str, version: &str) -> Vec<String> {
    let package = match id {
        "php" => php_package(version),
        "postgres" => postgres_package(version),
        "python" => python_package(version),
        _ => return vec![],
    };
    vec![package.to_string()]
}

/// Three of these can't be answered by looking up the catalog's binary name on PATH here,
/// and each fails in a different direction.
pub fn is_installed_override(id: &str) -> Option<bool> {
    match id {
        // nvm-windows installs a real `nvm.exe`, unlike the shell function on Linux.
        "nvm" => Some(crate::modules::tools::is_available("nvm")),

        // `python3` on PATH proves nothing on Windows 11: the OS ships a zero-byte App
        // Execution Alias by that exact name whose only job is to open the Microsoft Store.
        // It resolves for every user on every machine, so trusting it would mark Python
        // installed everywhere. The real interpreter answers to `python`, and is recognised
        // by *not* living in the WindowsApps alias directory.
        "python" => Some(
            crate::modules::tools::find("python")
                .map(|path| !path.to_ascii_lowercase().contains("\\windowsapps\\"))
                .unwrap_or(false),
        ),

        // The MSVC toolchain is never on PATH outside a Developer Command Prompt — neither
        // `cl` nor anything else — so a PATH check reports it missing on a machine that has
        // it. `vswhere.exe` is the supported way to ask, ships at a fixed location with the
        // VS installer, and is what the Rust and Node toolchains use for the same question.
        "build-essential" => Some(msvc_build_tools_present()),

        _ => None,
    }
}

fn msvc_build_tools_present() -> bool {
    let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") else {
        return false;
    };
    let vswhere = PathBuf::from(program_files_x86)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.is_file() {
        return false;
    }

    std::process::Command::new(vswhere)
        .args([
            "-latest",
            // Build Tools and the full IDE are both fine; what matters is the C++ toolset.
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|out| out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Never called: the Windows catalog installs nvm through winget, so no entry uses the
/// `Git` method. Present because the contract is shared, and honest about it rather than
/// pretending to clone a shell script that would not run here anyway.
pub fn install_nvm(_node_version: Option<&str>) -> Result<String, String> {
    Err("nvm for Windows is installed through winget, not by cloning — this path should not be reached".to_string())
}

/// nvm-windows keeps its Node releases in `%APPDATA%\nvm`, one `vX.Y.Z` directory each,
/// rather than Linux nvm's `~/.nvm/versions/node`. `NVM_HOME` overrides it, and is what the
/// installer sets.
pub fn installed_node_versions() -> Vec<String> {
    let root = std::env::var("NVM_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("APPDATA").map(|appdata| PathBuf::from(appdata).join("nvm")));
    let Ok(root) = root else {
        return Vec::new();
    };
    std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_str()?.to_string();
                    // Only the version directories; the installer's own files sit alongside.
                    let version = name.strip_prefix('v')?;
                    version
                        .starts_with(|c: char| c.is_ascii_digit())
                        .then_some(version.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every id in the shared catalog must have a plan here, or that app silently vanishes
    /// from the Apps page on Windows.
    #[test]
    fn every_catalogued_app_can_be_installed() {
        for id in crate::modules::apps::catalog_ids() {
            assert!(install_plan(id).is_some(), "{id} has no Windows install plan");
        }
    }

    /// A wrong winget id compiles, lists, and only fails when the user clicks Install — so
    /// the ones that were checked against the live repository are pinned here. The three
    /// commented-out guesses are the ones that do *not* exist, kept as a warning.
    #[test]
    fn package_ids_are_the_ones_that_exist_in_winget() {
        let packages = |id: &str| install_plan(id).map(|plan| plan.packages).unwrap_or_default();

        // Not `nginx.nginx`, which does not exist.
        assert_eq!(packages("nginx"), vec!["nginxinc.nginx"]);
        // Not `PostgreSQL.PostgreSQL` or `PHP.PHP` — both are versioned families.
        assert_eq!(packages("postgres"), vec!["PostgreSQL.PostgreSQL.17"]);
        assert_eq!(packages("php"), vec!["PHP.PHP.8.4"]);
        assert_eq!(packages("docker"), vec!["Docker.DockerDesktop"]);
        assert_eq!(packages("git"), vec!["Git.Git"]);
        assert_eq!(packages("nvm"), vec!["CoreyButler.NVMforWindows"]);
    }

    /// Every version the dropdown offers has to map to a real package; a version with no
    /// mapping would silently install the default instead of what was asked for.
    #[test]
    fn every_offered_version_maps_to_its_own_package() {
        for id in ["php", "postgres", "python"] {
            let offered = candidate_versions(id);
            assert!(!offered.is_empty(), "{id} offers no versions");

            let mut packages: Vec<String> = offered
                .iter()
                .map(|version| {
                    let packages = versioned_packages(id, version);
                    assert_eq!(packages.len(), 1, "{id} {version} should be one winget package");
                    assert!(
                        packages[0].ends_with(version),
                        "{id} {version} mapped to {} — a version with no mapping falls back to the default",
                        packages[0]
                    );
                    packages.into_iter().next().unwrap()
                })
                .collect();

            let count = packages.len();
            packages.sort();
            packages.dedup();
            assert_eq!(packages.len(), count, "{id} maps two versions to the same package");
        }
    }

    #[test]
    fn downloaded_binaries_get_an_extension_windows_will_run() {
        assert_eq!(executable_file_name("cloudflared"), "cloudflared.exe");
        // Already-suffixed names are left alone rather than doubled.
        assert_eq!(executable_file_name("cloudflared.exe"), "cloudflared.exe");
    }
}
