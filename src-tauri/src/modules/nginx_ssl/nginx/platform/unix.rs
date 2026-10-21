//! Linux and macOS. Every package on both — Debian/Ubuntu, RHEL/Fedora, Homebrew — is
//! built with an absolute `--conf-path=` and already `include`s `conf.d/*.conf` from
//! inside `http{}`, so DevsPro never has to touch the user's real nginx.conf: it drops one
//! file into that directory and nginx picks it up. This is the code the module ran before
//! the platform split.

use std::path::Path;

use crate::elevate::Operation;

use super::{MARKER_BEGIN, MARKER_END};

pub fn find_binary() -> Option<String> {
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
    super::which_nginx()
}

/// What `nginx -V` reports is already absolute here, so it is used as-is.
pub fn resolve_config_path(reported: &str, _binary_path: &str) -> Option<String> {
    Some(reported.to_string())
}

/// nginx resolves its own paths from the absolute `--conf-path` it was built with, so it
/// needs no `-p` and never got one before this split either.
pub fn prefix_for(_binary_path: &str) -> Option<String> {
    None
}

/// Finds nginx's `conf.d` directory next to its main config — every standard
/// package (Debian/Ubuntu, RHEL/Fedora) already `include`s `conf.d/*.conf` from
/// inside `http{}`, so dropping one file there wires our sites in without ever
/// touching the user's real nginx.conf.
pub fn conf_d_dir(config_path: &str) -> Option<std::path::PathBuf> {
    let dir = Path::new(config_path).parent()?.join("conf.d");
    dir.is_dir().then_some(dir)
}

/// The two operations that wire our sites in, in the order they have always been applied.
///
/// The first is a one-time cleanup for the earlier (buggy) approach, which appended the
/// include line at the very end of nginx.conf — landing *after* `http{}`'s closing brace,
/// where a `server{}` block is a syntax error. Safe to keep applying unconditionally on
/// every add/remove: a no-op once the stray block is gone.
pub fn wire_in_sites_ops(config_path: &str, sites_glob: &str) -> Result<Vec<Operation>, String> {
    let dir = conf_d_dir(config_path).ok_or_else(|| {
        format!(
            "could not find a conf.d directory next to {config_path} — this nginx installation doesn't use \
             the standard layout, so automatic site setup isn't supported for it yet"
        )
    })?;
    Ok(vec![
        Operation::RemoveMarkedBlock {
            file_path: config_path.to_string(),
            marker_begin: MARKER_BEGIN.to_string(),
            marker_end: MARKER_END.to_string(),
        },
        Operation::EnsureNginxConfDInclude {
            conf_d_dir: dir.to_string_lossy().to_string(),
            sites_glob: sites_glob.to_string(),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the order and shape of the Linux wiring, which is what the whole `conf.d`
    /// approach depends on: the legacy `include` block has to be removed from nginx.conf
    /// *before* the drop-in file is written, and both have to happen before the `nginx -t`
    /// that follows in `sites`. Reordering them silently produces a config that either
    /// fails the test or, worse, passes with a stale include still in place.
    #[test]
    fn wiring_removes_the_legacy_block_then_writes_the_conf_d_drop_in() {
        // Uses this machine's real nginx layout, so it only asserts once there is one to
        // assert about — the point is the order, not whether nginx is installed here.
        let Some(config_path) = find_binary()
            .as_deref()
            .and_then(|bin| {
                let output = std::process::Command::new(bin).arg("-V").output().ok()?;
                let text = String::from_utf8_lossy(&output.stderr).to_string();
                text.split_whitespace()
                    .find_map(|tok| tok.strip_prefix("--conf-path="))
                    .map(str::to_string)
            })
            .filter(|path| conf_d_dir(path).is_some())
        else {
            return;
        };

        let ops = wire_in_sites_ops(&config_path, "/tmp/devspro-sites/*.conf")
            .expect("a standard layout wires in without error");
        assert_eq!(ops.len(), 2, "one cleanup, one drop-in");
        assert!(
            matches!(ops[0], Operation::RemoveMarkedBlock { .. }),
            "the legacy include must be removed first"
        );
        assert!(
            matches!(ops[1], Operation::EnsureNginxConfDInclude { .. }),
            "then the conf.d drop-in is written"
        );
    }

    /// An installation without the standard `conf.d` layout is refused rather than guessed
    /// at — the Windows backend is the one that splices into `http{}`, and doing that here
    /// would edit a config file this platform has never needed to touch.
    #[test]
    fn wiring_refuses_a_layout_with_no_conf_d() {
        let result = wire_in_sites_ops("/nonexistent/nginx.conf", "/tmp/sites/*.conf");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("conf.d"));
    }
}

/// Paths already look to nginx the way they look here — no separator to translate and no
/// escape character in the way, unlike on Windows.
pub fn to_config_path(path: &str) -> String {
    path.to_string()
}

/// Which package manager (if any) is available on this Linux machine to install
/// nginx with. macOS's `brew` deliberately never goes through this path — Homebrew
/// refuses to run as root, so there's no elevated install step for it to plug into.
pub fn install_packages() -> Vec<String> {
    vec!["nginx".to_string()]
}

pub fn package_manager() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
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
