//! Windows. Three things differ from every Linux package, and none of them is just a
//! matter of which directory to look in:
//!
//! 1. **The config path nginx reports is relative.** The official builds are configured
//!    `--prefix= --conf-path=conf/nginx.conf`, so `nginx -V` hands back `conf/nginx.conf`
//!    with nothing to anchor it to.
//! 2. **nginx resolves that against the current working directory, not its own
//!    executable.** Run `nginx -t` from `D:\` and it looks for `D:\/conf/nginx.conf` and
//!    declares the whole configuration broken. Hence `prefix_for` — nginx's `-p` is not
//!    optional here, it is the only thing that makes an invocation from anywhere else work.
//! 3. **There is no `conf.d`.** No such directory ships, and nothing in the shipped
//!    nginx.conf includes one — so the Linux approach of dropping in a file has nowhere to
//!    drop it. DevsPro instead splices one marker-wrapped `include` line into `http{}`.

use std::fs;
use std::path::Path;

use crate::elevate::Operation;

use super::{MARKER_BEGIN, MARKER_END};

/// Directories that hold `nginx.exe` directly.
const DIRECT_ROOTS: &[&str] = &[r"C:\nginx", r"C:\Program Files\nginx"];

/// Directories holding one versioned subdirectory per install, each with its own
/// `nginx.exe` — Laragon's `nginx-1.22.0`, Chocolatey's `nginx-1.25.3`.
const VERSIONED_ROOTS: &[&str] = &[r"C:\laragon\bin\nginx", r"C:\tools"];

pub fn find_binary() -> Option<String> {
    // PATH comes first here, the opposite of the Unix backend's order, because Windows has
    // no canonical install location for nginx at all — so a copy the user has put on PATH
    // is a deliberate choice, and the scan below is only a fallback for bundles (Laragon,
    // Chocolatey) that don't add themselves. It also settles which install wins when
    // several are present, without this module having to rank version numbers.
    if let Some(found) = super::which_nginx() {
        return Some(found);
    }

    for root in DIRECT_ROOTS {
        let candidate = Path::new(root).join("nginx.exe");
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    VERSIONED_ROOTS.iter().find_map(|root| first_nested_nginx(Path::new(root)))
}

fn first_nested_nginx(root: &Path) -> Option<String> {
    fs::read_dir(root).ok()?.flatten().find_map(|entry| {
        let candidate = entry.path().join("nginx.exe");
        candidate
            .is_file()
            .then(|| candidate.to_string_lossy().to_string())
    })
}

/// Anchors the relative `--conf-path` from `nginx -V` to the install directory.
///
/// nginx itself would anchor it to the working directory, which is meaningless for reading
/// the file here — but it is the same answer, because every invocation this module makes
/// passes `prefix_for()` as `-p`, and that prefix *is* the install directory.
pub fn resolve_config_path(reported: &str, binary_path: &str) -> Option<String> {
    let reported_path = Path::new(reported);
    if reported_path.is_absolute() {
        return Some(reported.to_string());
    }
    let install_dir = Path::new(binary_path).parent()?;
    Some(install_dir.join(reported_path).to_string_lossy().to_string())
}

/// nginx's `-p`: the install directory, forward-slashed and with a trailing slash, which
/// is the form nginx concatenates its relative paths onto.
pub fn prefix_for(binary_path: &str) -> Option<String> {
    let install_dir = Path::new(binary_path).parent()?;
    let mut prefix = to_config_path(&install_dir.to_string_lossy());
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    Some(prefix)
}

/// One operation: splice `include <our sites>;` into `http{}`, replacing it if already
/// there.
///
/// No `conf.d` step and no legacy-cleanup step, unlike Unix. There is no `conf.d` to write
/// into, and there is no legacy block to clean up — the append-at-the-end bug that
/// `RemoveMarkedBlock` undoes on Linux predates DevsPro running on Windows at all, so
/// emitting it here would only find our own freshly-written block and delete it.
pub fn wire_in_sites_ops(config_path: &str, sites_glob: &str) -> Result<Vec<Operation>, String> {
    Ok(vec![Operation::EnsureIncludeInHttpBlock {
        config_path: config_path.to_string(),
        marker_begin: MARKER_BEGIN.to_string(),
        marker_end: MARKER_END.to_string(),
        include_line: format!("include {};", super::super::quoted_config_path(sites_glob)),
    }])
}

/// `winget`, when it is present — Windows 10 1809 and later ship it, older builds don't.
/// Reported as `None` when it is missing so the UI offers a copyable command instead of a
/// button that would fail.
pub fn package_manager() -> Option<String> {
    crate::modules::tools::is_available("winget").then(|| "winget".to_string())
}

/// nginx Inc's own build. Not `nginx.nginx` — the id that would mirror every other
/// vendor's naming simply doesn't exist in the winget repository, and `freenginx.nginx` is
/// the fork rather than nginx itself.
pub fn install_packages() -> Vec<String> {
    vec!["nginxinc.nginx".to_string()]
}

/// Rewrites a Windows path into the form nginx wants inside a config file.
///
/// nginx treats a backslash as an escape character when parsing directive arguments, so
/// `C:\Users\me\certs\site.pem` reaches it with the escapes consumed and the path broken.
/// Forward slashes are what nginx's own Windows documentation uses and it accepts them
/// everywhere a path is expected.
pub fn to_config_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_a_relative_conf_path_to_the_install_directory() {
        // Exactly what `nginx -V` reports on the official Windows builds.
        assert_eq!(
            resolve_config_path("conf/nginx.conf", r"C:\nginx\nginx.exe").as_deref(),
            Some(r"C:\nginx\conf/nginx.conf")
        );
    }

    #[test]
    fn leaves_an_absolute_conf_path_alone() {
        assert_eq!(
            resolve_config_path(r"C:\custom\nginx.conf", r"C:\nginx\nginx.exe").as_deref(),
            Some(r"C:\custom\nginx.conf")
        );
    }

    /// A missing or wrong `-p` is the difference between `nginx -t` passing and it
    /// reporting the entire configuration broken, so the exact shape matters.
    #[test]
    fn prefix_is_the_forward_slashed_install_directory_with_a_trailing_slash() {
        assert_eq!(
            prefix_for(r"C:\laragon\bin\nginx\nginx-1.22.0\nginx.exe").as_deref(),
            Some("C:/laragon/bin/nginx/nginx-1.22.0/")
        );
    }

    #[test]
    fn config_paths_are_forward_slashed_so_nginx_does_not_read_them_as_escapes() {
        assert_eq!(
            to_config_path(r"C:\Users\me\AppData\Roaming\DevsPro\certs\app.test.pem"),
            "C:/Users/me/AppData/Roaming/DevsPro/certs/app.test.pem"
        );
        // Already-forward-slashed input is left as it is.
        assert_eq!(to_config_path("C:/nginx/conf"), "C:/nginx/conf");
    }

    #[test]
    fn wiring_is_a_single_include_spliced_into_the_http_block() {
        let ops = wire_in_sites_ops(
            r"C:\nginx\conf\nginx.conf",
            r"C:\Users\me\AppData\Roaming\DevsPro\nginx\sites\*.conf",
        )
        .expect("windows wiring never fails to build");
        assert_eq!(ops.len(), 1, "no conf.d step and no legacy cleanup on Windows");
        match &ops[0] {
            Operation::EnsureIncludeInHttpBlock { include_line, .. } => assert_eq!(
                include_line,
                r#"include "C:/Users/me/AppData/Roaming/DevsPro/nginx/sites/*.conf";"#
            ),
            other => panic!("expected an http-block include, got {other:?}"),
        }
    }
}
