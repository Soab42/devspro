//! Caches tab: package-manager and tool caches, thumbnails, trash. A later
//! task fills this in; the commands exist so the contract is fixed.

use super::{CleanOutcome, CleanerState, Finding};
use crate::db::Db;
use tauri::{AppHandle, State};

#[tauri::command(async)]
pub fn cleaner_scan_caches(
    _app: AppHandle,
    _state: State<CleanerState>,
    _db: State<Db>,
    _scan_id: String,
) -> Result<Vec<Finding>, String> {
    Err("not implemented yet".to_string())
}

#[tauri::command(async)]
pub fn cleaner_clean_caches(
    _app: AppHandle,
    _db: State<Db>,
    _ids: Vec<String>,
) -> Result<Vec<CleanOutcome>, String> {
    Err("not implemented yet".to_string())
}
