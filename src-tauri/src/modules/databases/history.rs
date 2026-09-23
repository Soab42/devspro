//! The query log behind the console's History list.
//!
//! Every statement the console runs is recorded — including the ones that
//! failed, since "what did I run just before it broke" is most of the value.
//! The log is capped per connection on insert rather than swept on a timer, so
//! it can never grow without bound and there is no background job to own.

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

/// Appended to `db::later_migrations()` — see the note there on why new
/// migrations never go in a per-module list.
pub const MIGRATION_DB_QUERY_HISTORY: &str = r#"
CREATE TABLE db_query_history (
    id TEXT PRIMARY KEY,
    connection TEXT NOT NULL,
    database TEXT NOT NULL,
    sql TEXT NOT NULL,
    ok INTEGER NOT NULL,
    rows INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    ran_at TEXT NOT NULL
);
CREATE INDEX db_query_history_connection ON db_query_history(connection, ran_at DESC);
"#;

/// How many statements are kept per connection. Enough to scroll back through
/// a session's work, small enough that the list stays readable.
pub const KEEP_PER_CONNECTION: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub connection: String,
    pub database: String,
    pub sql: String,
    pub ok: bool,
    pub rows: i64,
    pub duration_ms: i64,
    pub ran_at: String,
}

const COLUMNS: &str = "id, connection, database, sql, ok, rows, duration_ms, ran_at";

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        connection: row.get(1)?,
        database: row.get(2)?,
        sql: row.get(3)?,
        ok: row.get::<_, i64>(4)? != 0,
        rows: row.get(5)?,
        duration_ms: row.get(6)?,
        ran_at: row.get(7)?,
    })
}

pub fn record(
    conn: &Connection,
    connection: &str,
    database: &str,
    sql: &str,
    ok: bool,
    rows: i64,
    duration_ms: i64,
) -> Result<(), String> {
    conn.execute(
        &format!("INSERT INTO db_query_history ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
        (
            uuid::Uuid::new_v4().to_string(),
            connection,
            database,
            sql,
            i64::from(ok),
            rows,
            duration_ms,
            chrono::Utc::now().to_rfc3339(),
        ),
    )
    .map_err(|e| e.to_string())?;
    prune(conn, connection)
}

/// Drops everything past the newest `KEEP_PER_CONNECTION` for one connection.
fn prune(conn: &Connection, connection: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM db_query_history WHERE connection = ?1 AND id NOT IN \
         (SELECT id FROM db_query_history WHERE connection = ?1 ORDER BY ran_at DESC, rowid DESC LIMIT ?2)",
        (connection, KEEP_PER_CONNECTION as i64),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list(conn: &Connection, connection: &str, limit: i64) -> Result<Vec<HistoryEntry>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM db_query_history WHERE connection = ?1 ORDER BY ran_at DESC, rowid DESC LIMIT ?2"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map((connection, limit.clamp(1, 500)), row_to_entry).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn databases_history_list(
    db: State<crate::db::Db>,
    connection: String,
    limit: Option<i64>,
) -> Result<Vec<HistoryEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list(&conn, &connection, limit.unwrap_or(50))
}

#[tauri::command(async)]
pub fn databases_history_clear(db: State<crate::db::Db>, connection: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM db_query_history WHERE connection = ?1", [&connection])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn a_failed_statement_is_recorded_too() {
        let conn = memory_db();
        record(&conn, "shop", "shop", "SELECT boom", false, 0, 3).unwrap();
        let all = list(&conn, "shop", 10).unwrap();
        assert_eq!(all.len(), 1);
        assert!(!all[0].ok);
        assert_eq!(all[0].sql, "SELECT boom");
    }

    #[test]
    fn history_is_newest_first_and_scoped_to_one_connection() {
        let conn = memory_db();
        record(&conn, "shop", "shop", "SELECT 1", true, 1, 1).unwrap();
        record(&conn, "shop", "shop", "SELECT 2", true, 1, 1).unwrap();
        record(&conn, "other", "other", "SELECT 3", true, 1, 1).unwrap();

        let shop = list(&conn, "shop", 10).unwrap();
        assert_eq!(shop.iter().map(|e| e.sql.as_str()).collect::<Vec<_>>(), vec!["SELECT 2", "SELECT 1"]);
        assert_eq!(list(&conn, "other", 10).unwrap().len(), 1);
    }

    #[test]
    fn the_log_is_capped_per_connection_so_it_cannot_grow_without_bound() {
        let conn = memory_db();
        for i in 0..(KEEP_PER_CONNECTION + 25) {
            record(&conn, "shop", "shop", &format!("SELECT {i}"), true, 1, 1).unwrap();
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM db_query_history WHERE connection='shop'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count as usize, KEEP_PER_CONNECTION);
        // The newest survived the pruning, not the oldest.
        assert_eq!(list(&conn, "shop", 1).unwrap()[0].sql, format!("SELECT {}", KEEP_PER_CONNECTION + 24));
    }
}
