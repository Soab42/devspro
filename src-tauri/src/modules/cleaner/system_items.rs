//! System tab: the apt package cache, the systemd journal and disabled snap
//! revisions. Scanning is unprivileged and read-only; cleaning goes through the
//! elevated helper as three fixed operations (`AptClean`, `VacuumJournal`,
//! `RemoveSnapRevision`), all selected items in one call so there is one
//! password prompt.

use super::measure::{disk_usage, free_bytes};
use super::{history, CleanOutcome, CleanerState, Finding, Risk, Scan};
use crate::db::Db;
use crate::elevate::{self, Operation, OperationManifest};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, State};

const TAB: &str = "system";
const GROUP: &str = "System";
const APT_DIR: &str = "/var/cache/apt/archives";
const SNAP_DIR: &str = "/var/lib/snapd/snaps";
const MIB: u64 = 1024 * 1024;
/// Suggested when the apt cache is at least this big.
const APT_SUGGEST_BYTES: u64 = 200 * MIB;
/// Suggested when the journal is over this size...
const JOURNAL_SUGGEST_BYTES: u64 = 500 * MIB;
/// ...and then vacuumed down to this.
const JOURNAL_TARGET_BYTES: u64 = 200 * MIB;
const PROMPT_NOTE: &str = "an admin password prompt will appear";

pub const ID_APT: &str = "system:apt";
pub const ID_JOURNAL: &str = "system:journal";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemItem {
    Apt,
    Journal,
    Snap { name: String, revision: String },
}

// The same rules the helper enforces; checking here too gives a clear message
// before the password prompt rather than after.
fn is_valid_snap_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_valid_snap_revision(rev: &str) -> bool {
    !rev.is_empty() && rev.len() <= 9 && !rev.starts_with('0') && rev.chars().all(|c| c.is_ascii_digit())
}

pub fn snap_id(name: &str, revision: &str) -> String {
    format!("system:snap:{name}:{revision}")
}

/// Strict: only the three id shapes this scan produces.
pub fn parse_id(id: &str) -> Result<SystemItem, String> {
    match id {
        ID_APT => return Ok(SystemItem::Apt),
        ID_JOURNAL => return Ok(SystemItem::Journal),
        _ => {}
    }
    let bad = || format!("{id:?} is not a system item id");
    let rest = id.strip_prefix("system:snap:").ok_or_else(bad)?;
    let (name, revision) = rest.split_once(':').ok_or_else(bad)?;
    if !is_valid_snap_name(name) || !is_valid_snap_revision(revision) {
        return Err(bad());
    }
    Ok(SystemItem::Snap { name: name.to_string(), revision: revision.to_string() })
}

pub fn to_operation(item: &SystemItem) -> Operation {
    match item {
        SystemItem::Apt => Operation::AptClean,
        SystemItem::Journal => Operation::VacuumJournal { max_bytes: JOURNAL_TARGET_BYTES },
        SystemItem::Snap { name, revision } => {
            Operation::RemoveSnapRevision { name: name.clone(), revision: revision.clone() }
        }
    }
}

/// `journalctl --disk-usage`: "Archived and active journals take up 1.2G in the
/// file system." (older versions: "Journals take up 96.0M on disk."). Units are
/// powers of 1024.
pub fn parse_journal_disk_usage(out: &str) -> Option<u64> {
    let rest = out.split("take up").nth(1)?.trim_start();
    let token = rest.split_whitespace().next()?;
    let split = token.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = token.split_at(split);
    let num: f64 = num.parse().ok()?;
    let shift = match unit {
        "B" => 0,
        "K" => 10,
        "M" => 20,
        "G" => 30,
        "T" => 40,
        "P" => 50,
        _ => return None,
    };
    Some((num * (1u64 << shift) as f64) as u64)
}

/// Disabled revisions from `snap list --all`, as (name, revision). The Notes
/// column is the last one and holds a comma-separated list.
pub fn parse_snap_list(out: &str) -> Vec<(String, String)> {
    out.lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 6 || !cols.last()?.split(',').any(|n| n == "disabled") {
                return None;
            }
            let (name, rev) = (cols[0], cols[2]);
            (is_valid_snap_name(name) && is_valid_snap_revision(rev))
                .then(|| (name.to_string(), rev.to_string()))
        })
        .collect()
}

fn finding(id: String, label: String, reason: String, method: String, paths: Vec<String>, bytes: u64, suggested: bool) -> Finding {
    Finding {
        id,
        tab: TAB.to_string(),
        group: GROUP.to_string(),
        label,
        reason,
        method: format!("{method}; {PROMPT_NOTE}"),
        paths,
        bytes,
        risk: Risk::Review,
        suggested,
        blocked_by: None,
        needs_confirm_text: None,
    }
}

fn apt_finding(bytes: u64) -> Finding {
    finding(
        ID_APT.to_string(),
        "apt package cache".to_string(),
        "Downloaded .deb files; apt fetches them again if a reinstall needs them.".to_string(),
        "apt-get clean".to_string(),
        vec![APT_DIR.to_string()],
        bytes,
        bytes >= APT_SUGGEST_BYTES,
    )
}

fn journal_finding(bytes: u64) -> Finding {
    finding(
        ID_JOURNAL.to_string(),
        "systemd journal".to_string(),
        format!("Old system logs; vacuuming keeps the newest {} MiB.", JOURNAL_TARGET_BYTES / MIB),
        format!("journalctl --vacuum-size={}M", JOURNAL_TARGET_BYTES / MIB),
        vec!["/var/log/journal".to_string()],
        bytes,
        bytes > JOURNAL_SUGGEST_BYTES,
    )
}

fn snap_finding(name: &str, revision: &str, bytes: u64) -> Finding {
    finding(
        snap_id(name, revision),
        format!("{name} (old revision {revision})"),
        "A disabled revision snap keeps after an update; the active one is untouched.".to_string(),
        format!("snap remove {name} --revision={revision}"),
        vec![snap_file(name, revision).display().to_string()],
        bytes,
        true,
    )
}

fn snap_file(name: &str, revision: &str) -> PathBuf {
    Path::new(SNAP_DIR).join(format!("{name}_{revision}.snap"))
}

fn tool_present(tool: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()))
        .unwrap_or(false)
}

fn stdout_of(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn journal_bytes() -> Option<u64> {
    if !tool_present("journalctl") {
        return None;
    }
    parse_journal_disk_usage(&stdout_of("journalctl", &["--disk-usage"])?)
}

fn scan_findings(scan: &Scan) -> Vec<Finding> {
    let mut found = Vec::new();
    if tool_present("apt-get") && Path::new(APT_DIR).is_dir() {
        let bytes = disk_usage(Path::new(APT_DIR), scan).bytes;
        if bytes > 0 {
            found.push(apt_finding(bytes));
        }
    }
    if scan.is_cancelled() {
        return found;
    }
    if let Some(bytes) = journal_bytes().filter(|b| *b > 0) {
        found.push(journal_finding(bytes));
    }
    if scan.is_cancelled() || !tool_present("snap") {
        return found;
    }
    if let Some(listing) = stdout_of("snap", &["list", "--all"]) {
        for (name, revision) in parse_snap_list(&listing) {
            let bytes = std::fs::metadata(snap_file(&name, &revision)).map(|m| m.len()).unwrap_or(0);
            found.push(snap_finding(&name, &revision, bytes));
        }
    }
    found
}

#[tauri::command(async)]
pub fn cleaner_scan_system(
    app: AppHandle,
    state: State<CleanerState>,
    _db: State<Db>,
    scan_id: String,
) -> Result<Vec<Finding>, String> {
    let scan = Scan::start(&state, &app, &scan_id, TAB);
    Ok(scan_findings(&scan))
}

/// Size of what an item occupies now, used to split the freed space between
/// items that went through one elevated call.
fn item_bytes(item: &SystemItem) -> u64 {
    match item {
        SystemItem::Apt => disk_usage(Path::new(APT_DIR), &Scan::detached()).bytes,
        SystemItem::Journal => journal_bytes().unwrap_or(0).saturating_sub(JOURNAL_TARGET_BYTES),
        SystemItem::Snap { name, revision } => {
            std::fs::metadata(snap_file(name, revision)).map(|m| m.len()).unwrap_or(0)
        }
    }
}

/// Splits `total` across items in proportion to `weights`; all-zero weights
/// give the lot to the first. The parts always sum to `total`.
pub fn split_freed(total: u64, weights: &[u64]) -> Vec<u64> {
    let sum: u128 = weights.iter().map(|w| *w as u128).sum();
    let mut parts: Vec<u64> = if sum == 0 {
        let mut p = vec![0; weights.len()];
        if let Some(first) = p.first_mut() {
            *first = total;
        }
        return p;
    } else {
        weights.iter().map(|w| (total as u128 * *w as u128 / sum) as u64).collect()
    };
    let assigned: u64 = parts.iter().sum();
    if let Some(last) = parts.last_mut() {
        *last += total - assigned;
    }
    parts
}

fn label_of(item: &SystemItem) -> String {
    match item {
        SystemItem::Apt => "apt package cache".to_string(),
        SystemItem::Journal => "systemd journal".to_string(),
        SystemItem::Snap { name, revision } => format!("{name} (old revision {revision})"),
    }
}

#[tauri::command(async)]
pub fn cleaner_clean_system(
    app: AppHandle,
    db: State<Db>,
    ids: Vec<String>,
) -> Result<Vec<CleanOutcome>, String> {
    // (id, parsed) in request order; a bad id fails alone, nothing else changes.
    let mut seen = HashSet::new();
    let requested: Vec<(String, Result<SystemItem, String>)> = ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .map(|id| {
            let parsed = parse_id(&id);
            (id, parsed)
        })
        .collect();
    let items: Vec<SystemItem> = requested.iter().filter_map(|(_, p)| p.as_ref().ok().cloned()).collect();

    let mut ran: Vec<(bool, String)> = Vec::new();
    let mut freed: Vec<u64> = vec![0; items.len()];
    if !items.is_empty() {
        let weights: Vec<u64> = items.iter().map(item_bytes).collect();
        let before = free_bytes(Path::new("/var")).ok();
        let results = elevate::run(
            &app,
            OperationManifest {
                allowed_write_prefix: String::new(),
                operations: items.iter().map(to_operation).collect(),
            },
        );
        match results {
            Ok(r) if r.len() == items.len() => ran = r.into_iter().map(|o| (o.success, o.message)).collect(),
            Ok(_) => ran = items.iter().map(|_| (false, "helper returned an unexpected number of results".to_string())).collect(),
            Err(e) => ran = items.iter().map(|_| (false, e.clone())).collect(),
        }
        if let (Some(before), Ok(after)) = (before, free_bytes(Path::new("/var"))) {
            // Shared among the items that succeeded only.
            let ok_idx: Vec<usize> = (0..ran.len()).filter(|i| ran[*i].0).collect();
            let ok_weights: Vec<u64> = ok_idx.iter().map(|i| weights[*i]).collect();
            for (i, part) in ok_idx.into_iter().zip(split_freed(after.saturating_sub(before), &ok_weights)) {
                freed[i] = part;
            }
        }
    }

    let mut outcomes = Vec::new();
    let mut next = 0;
    for (id, parsed) in requested {
        let outcome = match parsed {
            Err(message) => CleanOutcome { id, ok: false, message, bytes_freed: 0 },
            Ok(item) => {
                let (ok, message) = ran[next].clone();
                let bytes_freed = freed[next];
                next += 1;
                if let Ok(conn) = db.0.lock() {
                    let _ = history::record(&conn, TAB, &label_of(&item), bytes_freed, ok, &message);
                }
                CleanOutcome { id, ok, message, bytes_freed }
            }
        };
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_disk_usage_is_parsed() {
        assert_eq!(
            parse_journal_disk_usage("Archived and active journals take up 1.2G in the file system.\n"),
            Some((1.2 * (1u64 << 30) as f64) as u64)
        );
        assert_eq!(parse_journal_disk_usage("Journals take up 96.0M on disk."), Some(96 * MIB));
        assert_eq!(parse_journal_disk_usage("Archived and active journals take up 512K in the file system."), Some(512 * 1024));
        assert_eq!(parse_journal_disk_usage("take up 2T in the file system"), Some(2 << 40));
        assert_eq!(parse_journal_disk_usage("No journal files were found."), None);
        assert_eq!(parse_journal_disk_usage("journals take up lots"), None);
        assert_eq!(parse_journal_disk_usage(""), None);
    }

    #[test]
    fn snap_list_yields_only_disabled_revisions() {
        let out = "Name              Version        Rev    Tracking         Publisher   Notes\n\
core20            20240111       2182   latest/stable    canonical**  base,disabled\n\
core20            20240227       2264   latest/stable    canonical**  base\n\
firefox           123.0          3836   latest/stable    mozilla**    disabled\n\
gnome-42-2204     0+git.510a2d1  141    latest/stable/…  canonical**  -\n\
bad_name          1              7      latest/stable    x            disabled\n";
        assert_eq!(
            parse_snap_list(out),
            vec![("core20".to_string(), "2182".to_string()), ("firefox".to_string(), "3836".to_string())]
        );
        assert!(parse_snap_list("").is_empty());
        assert!(parse_snap_list("Name Version Rev Tracking Publisher Notes\n").is_empty());
    }

    #[test]
    fn ids_are_validated_strictly() {
        assert_eq!(parse_id("system:apt"), Ok(SystemItem::Apt));
        assert_eq!(parse_id("system:journal"), Ok(SystemItem::Journal));
        assert_eq!(
            parse_id("system:snap:core20:2182"),
            Ok(SystemItem::Snap { name: "core20".into(), revision: "2182".into() })
        );
        for bad in [
            "", "system", "system:", "system:apt ", "system:Apt", "system:snap:", "system:snap:core20",
            "system:snap:core20:", "system:snap::12", "system:snap:Core:12", "system:snap:core20:12x",
            "system:snap:core20:-1", "system:snap:core20:12:3", "system:snap:-x:12", "system:snap:a b:12",
            "system:snap:../x:12", "system:snap:core20:0", "caches:foo", "/etc/passwd",
        ] {
            assert!(parse_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ids_map_to_fixed_operations() {
        assert!(matches!(to_operation(&SystemItem::Apt), Operation::AptClean));
        assert!(matches!(
            to_operation(&SystemItem::Journal),
            Operation::VacuumJournal { max_bytes } if max_bytes == 200 * MIB
        ));
        let op = to_operation(&parse_id("system:snap:core20:2182").unwrap());
        assert!(matches!(&op, Operation::RemoveSnapRevision { name, revision } if name == "core20" && revision == "2182"));
        assert_eq!(snap_id("core20", "2182"), "system:snap:core20:2182");
    }

    #[test]
    fn findings_and_suggestions() {
        let small = apt_finding(199 * MIB);
        let big = apt_finding(200 * MIB);
        assert!(!small.suggested && big.suggested);
        assert_eq!((big.tab.as_str(), big.group.as_str(), big.risk), ("system", "System", Risk::Review));
        assert!(big.method.contains("password"));
        assert!(!journal_finding(500 * MIB).suggested);
        assert!(journal_finding(500 * MIB + 1).suggested);
        let snap = snap_finding("core20", "2182", 5);
        assert!(snap.suggested && snap.id == "system:snap:core20:2182" && snap.method.contains("password"));
    }

    #[test]
    fn freed_space_is_split_by_weight() {
        assert_eq!(split_freed(100, &[1, 3]), vec![25, 75]);
        assert_eq!(split_freed(10, &[1, 1, 1]).iter().sum::<u64>(), 10);
        assert_eq!(split_freed(50, &[0, 0]), vec![50, 0]);
        assert_eq!(split_freed(50, &[]), Vec::<u64>::new());
    }
}
