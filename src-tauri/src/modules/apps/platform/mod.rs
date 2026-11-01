//! How each catalogued app is installed on this OS.
//!
//! The catalog itself — which tools are offered, what they are called, what they are for —
//! is shared, because that is a product decision rather than a platform one. What differs
//! is everything about *installing* them: the package identifiers (`php8.3-fpm` against
//! `PHP.PHP.8.3`), which mechanism applies at all, and where an unprivileged install puts
//! its files.
//!
//! | | |
//! | --- | --- |
//! | `install_plan()` | How to install one catalogued app here, or `None` if this OS can't. |
//! | `package_manager_label()` | The name the UI shows for the elevated mechanism. |
//! | `home_dir()` | The user's home, from whichever variable this OS uses. |
//! | `user_bin_dir()` | Where an unprivileged single-binary install lands. |
//! | `executable_file_name()` | A bare tool name as it must be written on disk to run. |
//! | `candidate_versions()` | Versions this OS can actually offer for one app. |
//! | `live_versions_package()` | The package whose every published version is asked for when the picker opens, for apps with too many releases to list here. |
//! | `published_versions()` | Those versions, newest first, straight from the package manager. |
//! | `installed_version()` | The version of a package this machine has, to prove a pinned install landed. |
//! | `is_installed_override()` | Detection for the entries where "is the binary on PATH" is the wrong question here. |
//! | `install_nvm()` | Set up the Node version manager this OS uses. |
//! | `installed_node_versions()` | Node releases that version manager has installed. |

use super::InstallMethod;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// How one app gets installed here.
pub struct InstallPlan {
    pub method: InstallMethod,
    /// Identifiers for this OS's package manager. Only meaningful for
    /// `InstallMethod::PackageManager`.
    pub packages: Vec<String>,
    /// Only meaningful for `InstallMethod::Binary`. `{arch}` is substituted with the
    /// Go-style architecture name the vendor uses in its release asset filenames.
    pub binary_url: Option<&'static str>,
    /// Replaces the catalog's shared description where this OS does something different
    /// enough that the shared wording would be wrong.
    pub description: Option<&'static str>,
}

impl InstallPlan {
    pub fn packages(method: InstallMethod, packages: &[&str]) -> Self {
        InstallPlan {
            method,
            packages: packages.iter().map(|p| p.to_string()).collect(),
            binary_url: None,
            description: None,
        }
    }

    pub fn describe(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }
}
