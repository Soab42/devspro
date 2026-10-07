//! Project junk tab: node_modules, target, vendor and friends in projects that
//! have sat idle. A later task fills this in.

use super::{CleanOutcome, CleanerState};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectJunk {
    pub project_path: String,
    pub project_name: String,
    pub artifact_path: String,
    pub artifact_kind: String,
    pub bytes: u64,
    /// Unix seconds of the project's last change.
    pub last_activity: Option<i64>,
    pub idle_days: Option<u32>,
    pub suggested: bool,
}

#[tauri::command(async)]
pub fn cleaner_scan_projects(
    _app: AppHandle,
    _state: State<CleanerState>,
    _db: State<Db>,
    _scan_id: String,
) -> Result<Vec<ProjectJunk>, String> {
    Err("not implemented yet".to_string())
}

#[tauri::command(async)]
pub fn cleaner_clean_projects(
    _db: State<Db>,
    _artifact_paths: Vec<String>,
) -> Result<Vec<CleanOutcome>, String> {
    Err("not implemented yet".to_string())
}
