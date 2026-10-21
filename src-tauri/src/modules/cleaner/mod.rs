//! Cleaner — finds what is taking disk space and removes it, always showing
//! the user what first. Each tab owns one file; the shared contract lives here.
//!
//! - `measure`      — disk usage (blocks, hard links once, one filesystem), free
//!   space, and the cancellable, throttled progress handle every scan carries.
//! - `guard`        — `check_removable`, the one gate every removal passes, and
//!   `remove_path` (trash by default).
//! - `history`      — what was cleaned and when, and the settings table.
//! - `snapshot`     — each tab's last results as JSON, so they survive a restart.
//! - `caches`, `system_items`, `docker`, `large_files`, `projects` — the tabs.
//!
//! Elevated cleanup, where a tab needs it, goes through the helper.
//!
//! The tabs run on Linux and Windows. What differs — the protected folders,
//! the System items, the folders other tabs own — is chosen with `cfg` inside
//! each tab, next to the Linux version it mirrors.

pub mod caches;
pub mod docker;
pub mod guard;
pub mod history;
pub mod large_files;
pub mod measure;
pub mod projects;
pub mod snapshot;
pub mod storage_map;
pub mod system_items;

use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tauri::State;

pub use measure::{CleanerState, Scan};

/// Event name for scan progress, payload `ScanProgress`.
pub const PROGRESS_EVENT: &str = "cleaner://progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Regenerates by itself; nothing is lost.
    Safe,
    /// Probably fine, but the user should look.
    Review,
    /// Can lose data. Never pre-selected.
    Danger,
}

/// One thing a scan found that could be removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Stable within a scan; clean commands take these back.
    pub id: String,
    /// The tab that produced it: "caches", "system", "docker", ...
    pub tab: String,
    pub group: String,
    pub label: String,
    /// Why it is safe, or why it needs a look.
    pub reason: String,
    /// What cleaning does: "trash", "delete", "docker prune", ...
    pub method: String,
    pub paths: Vec<String>,
    pub bytes: u64,
    pub risk: Risk,
    /// Pre-ticked in the table.
    pub suggested: bool,
    /// Set when it cannot be cleaned right now (a running process holds it).
    pub blocked_by: Option<String>,
    /// The user must type this exactly before it is cleaned.
    pub needs_confirm_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanOutcome {
    pub id: String,
    pub ok: bool,
    pub message: String,
    pub bytes_freed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CleanerSettings {
    pub scan_roots: Vec<String>,
    pub project_roots: Vec<String>,
    pub exclusions: Vec<String>,
    pub large_file_min_bytes: u64,
    pub project_idle_days: u32,
}

impl Default for CleanerSettings {
    fn default() -> Self {
        CleanerSettings {
            scan_roots: home_dir().map(|h| h.to_string_lossy().into_owned()).into_iter().collect(),
            project_roots: Vec::new(),
            exclusions: Vec::new(),
            large_file_min_bytes: 500 * 1024 * 1024,
            project_idle_days: 30,
        }
    }
}

/// The user's home: `$HOME` on Unix, `%USERPROFILE%` on Windows (where HOME
/// is normally unset). `None` when unset or not absolute.
pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).filter(|h| h.is_absolute())
}

/// `Command::new(program)` that opens no console window on Windows, where a
/// GUI app spawning `git` or `docker` would otherwise flash one per call.
pub fn quiet_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    /// RFC3339.
    pub at: String,
    pub tab: String,
    pub label: String,
    pub bytes_freed: u64,
    pub ok: bool,
    pub message: String,
}

/// Emitted as `cleaner://progress`, at most about every 250 ms per scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_id: String,
    pub tab: String,
    pub entries: u64,
    pub bytes: u64,
    /// What it is looking at now.
    pub current: String,
}

#[tauri::command(async)]
pub fn cleaner_settings_get(db: State<Db>) -> Result<CleanerSettings, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(history::load_settings(&conn))
}

#[tauri::command(async)]
pub fn cleaner_settings_set(db: State<Db>, settings: CleanerSettings) -> Result<CleanerSettings, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    history::save_settings(&conn, &settings)?;
    Ok(settings)
}

#[tauri::command(async)]
pub fn cleaner_history_list(db: State<Db>, limit: Option<u32>) -> Result<Vec<HistoryEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    history::list(&conn, limit.unwrap_or(200))
}

#[tauri::command(async)]
pub fn cleaner_history_clear(db: State<Db>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    history::clear(&conn)
}

/// Asks a running scan to stop; it returns what it has so far. An unknown or
/// finished id is not an error.
#[tauri::command]
pub fn cleaner_cancel_scan(state: State<CleanerState>, scan_id: String) -> Result<(), String> {
    state.cancel(&scan_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Risk::Safe).unwrap(), "\"safe\"");
        assert_eq!(serde_json::to_string(&Risk::Review).unwrap(), "\"review\"");
        assert_eq!(serde_json::to_string(&Risk::Danger).unwrap(), "\"danger\"");
    }

    #[test]
    fn default_settings() {
        let s = CleanerSettings::default();
        assert_eq!(s.large_file_min_bytes, 500 * 1024 * 1024);
        assert_eq!(s.project_idle_days, 30);
        assert!(s.project_roots.is_empty() && s.exclusions.is_empty());
    }

    #[test]
    fn partial_settings_json_fills_defaults() {
        let s: CleanerSettings = serde_json::from_str(r#"{"exclusions":["/a"]}"#).unwrap();
        assert_eq!(s.exclusions, vec!["/a".to_string()]);
        assert_eq!(s.project_idle_days, 30);
    }
}
