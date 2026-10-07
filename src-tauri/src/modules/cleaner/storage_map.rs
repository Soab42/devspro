//! Storage map tab: a size tree of a folder and a breakdown by file extension.
//! A later task fills this in.

use super::CleanerState;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtStat {
    pub ext: String,
    pub kind: String,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTree {
    pub root: TreeNode,
    pub extensions: Vec<ExtStat>,
    pub total_bytes: u64,
    pub files: u64,
    /// Cancelled or cut off before it finished.
    pub truncated: bool,
}

#[tauri::command(async)]
pub fn cleaner_scan_tree(
    _app: AppHandle,
    _state: State<CleanerState>,
    _scan_id: String,
    _root: String,
) -> Result<StorageTree, String> {
    Err("not implemented yet".to_string())
}
