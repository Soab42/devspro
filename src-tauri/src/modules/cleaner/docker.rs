//! Docker tab: dangling images, stopped containers, build cache, unused
//! volumes. A later task fills this in.

use super::{CleanOutcome, CleanerState, Finding};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerScan {
    /// False when Docker is missing or its daemon is not running.
    pub available: bool,
    /// Why it is unavailable.
    pub message: Option<String>,
    pub findings: Vec<Finding>,
}

#[tauri::command(async)]
pub fn cleaner_scan_docker(
    _app: AppHandle,
    _state: State<CleanerState>,
    _db: State<Db>,
    _scan_id: String,
) -> Result<DockerScan, String> {
    Err("not implemented yet".to_string())
}

/// `confirmed` lists the ids whose typed confirmation text matched.
#[tauri::command(async)]
pub fn cleaner_clean_docker(
    _app: AppHandle,
    _db: State<Db>,
    _ids: Vec<String>,
    _confirmed: Vec<String>,
) -> Result<Vec<CleanOutcome>, String> {
    Err("not implemented yet".to_string())
}
