//! Builds the /etc/hosts sync operation from the full current set of site hostnames.
//! Deliberately full-sync rather than incremental add/remove: the entire devsuite
//! block is recomputed and replaced every time a site is added/removed, so there's
//! no drift between "what sites exist" and "what's in the marked block" to track by
//! hand.

pub const MARKER_BEGIN: &str = "# >>> devsuite (do not edit this block manually) >>>";
pub const MARKER_END: &str = "# <<< devsuite <<<";

/// Where this OS keeps the hosts file. Same format and same semantics everywhere; only the
/// location differs.
///
/// Windows' copy lives under `System32`, and its location is read from `%SystemRoot%`
/// rather than hardcoded to `C:\Windows`: the drive and directory are chosen at install
/// time, and a machine imaged with Windows somewhere else would otherwise have DevsPro
/// silently editing a path that doesn't exist.
pub fn hosts_file() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        std::path::Path::new(&system_root)
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from("/etc/hosts")
    }
}

pub fn sync_op(hostnames: &[String]) -> crate::elevate::Operation {
    let block_lines = hostnames
        .iter()
        .map(|h| format!("127.0.0.1 {h}"))
        .collect();
    crate::elevate::Operation::SyncMarkedBlock {
        file_path: hosts_file().to_string_lossy().to_string(),
        marker_begin: MARKER_BEGIN.to_string(),
        marker_end: MARKER_END.to_string(),
        block_lines,
    }
}

/// True if `hostname` already resolves via the hosts file to something other than our
/// own devsuite-managed block — a conflict the UI should warn about before adding.
pub fn resolves_elsewhere(hostname: &str) -> bool {
    let Ok(contents) = std::fs::read_to_string(hosts_file()) else {
        return false;
    };
    let mut in_devsuite_block = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == MARKER_BEGIN {
            in_devsuite_block = true;
            continue;
        }
        if trimmed == MARKER_END {
            in_devsuite_block = false;
            continue;
        }
        if in_devsuite_block || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.split_whitespace().skip(1).any(|h| h == hostname) {
            return true;
        }
    }
    false
}
