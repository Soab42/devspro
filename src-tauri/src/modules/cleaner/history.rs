//! What the Cleaner removed, and its settings. Both live in SQLite; settings
//! are one JSON document so a new field never needs a migration.

use super::{CleanerSettings, HistoryEntry};
use rusqlite::{params, Connection};

/// Appended to `db::later_migrations()` — see the note there on why new
/// migrations never go in a per-module list.
pub const MIGRATION_CLEANER: &str = r#"
CREATE TABLE cleaner_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    tab TEXT NOT NULL,
    label TEXT NOT NULL,
    bytes_freed INTEGER NOT NULL DEFAULT 0,
    ok INTEGER NOT NULL DEFAULT 1,
    message TEXT NOT NULL DEFAULT ''
);
CREATE TABLE cleaner_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const SETTINGS_KEY: &str = "settings";

/// Newest first is the order the History view wants.
const KEEP: i64 = 5000;

pub fn record(
    conn: &Connection,
    tab: &str,
    label: &str,
    bytes_freed: u64,
    ok: bool,
    message: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO cleaner_history (at, tab, label, bytes_freed, ok, message) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![chrono::Utc::now().to_rfc3339(), tab, label, bytes_freed as i64, ok, message],
    )
    .map_err(|e| e.to_string())?;
    // Bounded, so a long-lived install does not grow without limit.
    conn.execute(
        "DELETE FROM cleaner_history WHERE id <= (SELECT MAX(id) FROM cleaner_history) - ?1",
        params![KEEP],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list(conn: &Connection, limit: u32) -> Result<Vec<HistoryEntry>, String> {
    let mut stmt = conn
        .prepare("SELECT id, at, tab, label, bytes_freed, ok, message FROM cleaner_history ORDER BY id DESC LIMIT ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit as i64], |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                at: r.get(1)?,
                tab: r.get(2)?,
                label: r.get(3)?,
                bytes_freed: r.get::<_, i64>(4)?.max(0) as u64,
                ok: r.get(5)?,
                message: r.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

pub fn clear(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM cleaner_history", []).map_err(|e| e.to_string())?;
    Ok(())
}

/// Defaults when nothing is stored or the stored value no longer parses.
pub fn load_settings(conn: &Connection) -> CleanerSettings {
    conn.query_row("SELECT value FROM cleaner_settings WHERE key = ?1", params![SETTINGS_KEY], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .and_then(|v| serde_json::from_str(&v).ok())
    .unwrap_or_default()
}

pub fn save_settings(conn: &Connection, settings: &CleanerSettings) -> Result<(), String> {
    let json = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO cleaner_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SETTINGS_KEY, json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(MIGRATION_CLEANER).unwrap();
        c
    }

    #[test]
    fn records_and_lists_newest_first() {
        let c = conn();
        record(&c, "caches", "npm", 10, true, "ok").unwrap();
        record(&c, "docker", "images", 20, false, "boom").unwrap();
        let all = list(&c, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].label, "images");
        assert!(!all[0].ok);
        assert_eq!(all[1].bytes_freed, 10);
        assert_eq!(list(&c, 1).unwrap().len(), 1);
        assert!(chrono::DateTime::parse_from_rfc3339(&all[0].at).is_ok());
        clear(&c).unwrap();
        assert!(list(&c, 10).unwrap().is_empty());
    }

    #[test]
    fn settings_round_trip_and_default() {
        let c = conn();
        assert_eq!(load_settings(&c), CleanerSettings::default());
        let s = CleanerSettings { exclusions: vec!["/x".into()], project_idle_days: 7, ..Default::default() };
        save_settings(&c, &s).unwrap();
        save_settings(&c, &s).unwrap();
        assert_eq!(load_settings(&c), s);
    }
}
