//! Generic app-preference storage. Reuses the `settings` key/value table the auth
//! module's migration already created for session persistence — no new migration
//! needed, this just adds a second, generic reader/writer over the same table.

use rusqlite::Connection;
use tauri::State;

/// Plain (non-command) reader, for callers that already hold a `Connection`
/// outside of a Tauri command context — e.g. the tray module's window
/// close-handler, which runs in a plain closure, not an `invoke()`-triggered command.
pub fn get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
        .ok()
}

#[tauri::command(async)]
pub fn settings_get(db: State<crate::db::Db>, key: String) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    match conn.query_row("SELECT value FROM settings WHERE key = ?1", [&key], |row| row.get(0)) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command(async)]
pub fn settings_set(db: State<crate::db::Db>, key: String, value: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (&key, &value),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    fn conn_with_settings_table() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        conn
    }

    #[test]
    fn get_on_missing_key_returns_none() {
        let conn = conn_with_settings_table();
        let value: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", ["missing"], |r| r.get(0))
            .ok();
        assert_eq!(value, None);
    }

    #[test]
    fn set_then_get_round_trips_and_upserts() {
        let conn = conn_with_settings_table();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            ("theme", "dark"),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            ("theme", "light"),
        )
        .unwrap();
        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", ["theme"], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "light");
    }
}
