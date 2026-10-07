//! Large files tab: big files, and a folder-by-folder size view. A later task
//! fills this in.

use super::{CleanOutcome, CleanerState};
use crate::db::Db;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFile {
    pub path: String,
    pub bytes: u64,
    /// Unix seconds.
    pub modified: i64,
    /// Unix seconds.
    pub accessed: i64,
    /// video, disk-image, archive, db-dump, log, core-dump or other.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEntry {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub is_dir: bool,
    /// Entries inside, for a directory.
    pub entries: u64,
}

#[tauri::command(async)]
pub fn cleaner_scan_large_files(
    _app: AppHandle,
    _state: State<CleanerState>,
    _db: State<Db>,
    _scan_id: String,
    _min_bytes: u64,
    _older_than_days: Option<u32>,
) -> Result<Vec<LargeFile>, String> {
    Err("not implemented yet".to_string())
}

#[tauri::command(async)]
pub fn cleaner_folder_sizes(
    _app: AppHandle,
    _state: State<CleanerState>,
    _scan_id: String,
    _path: String,
) -> Result<Vec<FolderEntry>, String> {
    Err("not implemented yet".to_string())
}

#[tauri::command(async)]
pub fn cleaner_remove_files(
    _db: State<Db>,
    _paths: Vec<String>,
    _permanent: bool,
) -> Result<Vec<CleanOutcome>, String> {
    Err("not implemented yet".to_string())
}
