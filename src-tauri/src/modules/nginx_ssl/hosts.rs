//! Builds the /etc/hosts sync operation from the full current set of site hostnames.
//! Deliberately full-sync rather than incremental add/remove: the entire devsuite
//! block is recomputed and replaced every time a site is added/removed, so there's
//! no drift between "what sites exist" and "what's in the marked block" to track by
//! hand.

pub const HOSTS_FILE: &str = "/etc/hosts";
pub const MARKER_BEGIN: &str = "# >>> devsuite (do not edit this block manually) >>>";
pub const MARKER_END: &str = "# <<< devsuite <<<";

pub fn sync_op(hostnames: &[String]) -> crate::elevate::Operation {
    let block_lines = hostnames
        .iter()
        .map(|h| format!("127.0.0.1 {h}"))
        .collect();
    crate::elevate::Operation::SyncMarkedBlock {
        file_path: HOSTS_FILE.to_string(),
        marker_begin: MARKER_BEGIN.to_string(),
        marker_end: MARKER_END.to_string(),
        block_lines,
    }
}

/// True if `hostname` already resolves via /etc/hosts to something other than our
/// own devsuite-managed block — a conflict the UI should warn about before adding.
pub fn resolves_elsewhere(hostname: &str) -> bool {
    let Ok(contents) = std::fs::read_to_string(HOSTS_FILE) else {
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
