//! Builds the hosts-file sync operation from the full current set of site hostnames.
//! Deliberately full-sync rather than incremental add/remove: the entire devspro
//! block is recomputed and replaced every time a site is added/removed, so there's
//! no drift between "what sites exist" and "what's in the marked block" to track by
//! hand.

use crate::modules::hosts_file::hosts_path;

pub const MARKER_BEGIN: &str = "# >>> devspro (do not edit this block manually) >>>";
pub const MARKER_END: &str = "# <<< devspro <<<";

pub fn sync_op(hostnames: &[String]) -> crate::elevate::Operation {
    let block_lines = hostnames
        .iter()
        .map(|h| format!("127.0.0.1 {h}"))
        .collect();
    crate::elevate::Operation::SyncMarkedBlock {
        file_path: hosts_path().to_string_lossy().into_owned(),
        marker_begin: MARKER_BEGIN.to_string(),
        marker_end: MARKER_END.to_string(),
        block_lines,
    }
}

/// True if `hostname` already resolves via the hosts file to something other than our
/// own devspro-managed block — a conflict the UI should warn about before adding.
pub fn resolves_elsewhere(hostname: &str) -> bool {
    let Ok(contents) = std::fs::read_to_string(hosts_path()) else {
        return false;
    };
    let mut in_devspro_block = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == MARKER_BEGIN {
            in_devspro_block = true;
            continue;
        }
        if trimmed == MARKER_END {
            in_devspro_block = false;
            continue;
        }
        if in_devspro_block || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.split_whitespace().skip(1).any(|h| h == hostname) {
            return true;
        }
    }
    false
}
