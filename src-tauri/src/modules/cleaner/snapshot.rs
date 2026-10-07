//! The last results of each Cleaner tab, so they survive a restart. One JSON
//! document per tab; the frontend owns its shape, this only stores it.
//!
//! A snapshot is a display cache. Cleaning never reads it: every clean command
//! re-resolves its ids against a fresh look at the disk.

use crate::db::Db;
use rusqlite::{params, Connection, OptionalExtension};
use tauri::State;

/// Appended to `db::later_migrations()`.
pub const MIGRATION_CLEANER_SNAPSHOTS: &str = r#"
CREATE TABLE cleaner_snapshots (
    tab TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    saved_at TEXT NOT NULL
);
"#;

/// Larger snapshots are refused; the frontend then keeps them in memory only.
pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;

fn check_tab(tab: &str) -> Result<(), String> {
    let ok = !tab.is_empty() && tab.len() <= 32 && tab.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!("Not a Cleaner tab: {tab:?}"))
    }
}

pub fn get(conn: &Connection, tab: &str) -> Result<Option<String>, String> {
    check_tab(tab)?;
    conn.query_row("SELECT json FROM cleaner_snapshots WHERE tab = ?1", params![tab], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

/// Stores the snapshot. `Ok(false)` means it was too large and nothing was
/// written; the previous snapshot, if any, is left as it was.
pub fn set(conn: &Connection, tab: &str, json: &str) -> Result<bool, String> {
    check_tab(tab)?;
    if json.len() > MAX_SNAPSHOT_BYTES {
        return Ok(false);
    }
    serde_json::from_str::<serde::de::IgnoredAny>(json).map_err(|e| format!("Snapshot is not valid JSON: {e}"))?;
    conn.execute(
        "INSERT INTO cleaner_snapshots (tab, json, saved_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(tab) DO UPDATE SET json = excluded.json, saved_at = excluded.saved_at",
        params![tab, json, chrono::Utc::now().to_rfc3339()],
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// One tab's snapshot, or every tab's when `tab` is `None`.
pub fn clear(conn: &Connection, tab: Option<&str>) -> Result<(), String> {
    match tab {
        Some(t) => {
            check_tab(t)?;
            conn.execute("DELETE FROM cleaner_snapshots WHERE tab = ?1", params![t])
        }
        None => conn.execute("DELETE FROM cleaner_snapshots", []),
    }
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn cleaner_snapshot_get(db: State<Db>, tab: String) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    get(&conn, &tab)
}

#[tauri::command(async)]
pub fn cleaner_snapshot_set(db: State<Db>, tab: String, json: String) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    set(&conn, &tab, &json)
}

#[tauri::command(async)]
pub fn cleaner_snapshot_clear(db: State<Db>, tab: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    clear(&conn, tab.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(MIGRATION_CLEANER_SNAPSHOTS).unwrap();
        c
    }

    #[test]
    fn get_is_none_until_set_then_round_trips_and_overwrites() {
        let c = conn();
        assert_eq!(get(&c, "caches").unwrap(), None);
        assert!(set(&c, "caches", r#"{"a":1}"#).unwrap());
        assert_eq!(get(&c, "caches").unwrap().as_deref(), Some(r#"{"a":1}"#));
        assert!(set(&c, "caches", r#"{"a":2}"#).unwrap());
        assert_eq!(get(&c, "caches").unwrap().as_deref(), Some(r#"{"a":2}"#));
        assert_eq!(get(&c, "docker").unwrap(), None);
    }

    #[test]
    fn clear_removes_one_tab_or_all() {
        let c = conn();
        set(&c, "caches", "{}").unwrap();
        set(&c, "docker", "{}").unwrap();
        clear(&c, Some("caches")).unwrap();
        assert_eq!(get(&c, "caches").unwrap(), None);
        assert!(get(&c, "docker").unwrap().is_some());
        clear(&c, None).unwrap();
        assert_eq!(get(&c, "docker").unwrap(), None);
    }

    #[test]
    fn oversized_snapshot_is_refused_and_keeps_the_old_one() {
        let c = conn();
        set(&c, "projects", r#"{"ok":true}"#).unwrap();
        let big = format!("\"{}\"", "x".repeat(MAX_SNAPSHOT_BYTES));
        assert!(!set(&c, "projects", &big).unwrap());
        assert_eq!(get(&c, "projects").unwrap().as_deref(), Some(r#"{"ok":true}"#));
        let edge = format!("\"{}\"", "x".repeat(MAX_SNAPSHOT_BYTES - 2));
        assert_eq!(edge.len(), MAX_SNAPSHOT_BYTES);
        assert!(set(&c, "projects", &edge).unwrap());
    }

    #[test]
    fn rejects_bad_json_and_bad_tab_names() {
        let c = conn();
        assert!(set(&c, "caches", "{nope").is_err());
        assert!(set(&c, "", "{}").is_err());
        assert!(set(&c, "a b", "{}").is_err());
        assert!(get(&c, "../x").is_err());
        assert!(clear(&c, Some("x'; --")).is_err());
    }
}
