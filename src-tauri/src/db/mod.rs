//! One SQLite file, one migrations pipeline. Each module contributes its own ordered
//! migration SQL via a `migrations()` fn (see `crate::auth::migrations`); this module
//! just concatenates them in a fixed order and applies whatever hasn't run yet,
//! tracked via SQLite's built-in `PRAGMA user_version`.

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Db(pub Mutex<Connection>);

/// Migrations added after the per-module lists were frozen. **Append only** — a
/// new entry goes at the end, and nothing is ever inserted or removed.
///
/// This list exists because the per-module scheme below is unsafe to extend. A
/// migration's identity is its *position* in the concatenated list, so adding one
/// to the end of `nginx_ssl::migrations()` shifts every `workers` migration up by
/// one index — and an already-migrated database then re-runs them, failing with
/// "table workers already exists". That is not hypothetical; it is exactly what
/// happened when the site-limits migration was first added in the module list.
///
/// One flat list, not a per-module `later_migrations()` each: per-module lists
/// would reintroduce the same shifting problem the moment a second module added
/// one.
fn later_migrations() -> Vec<&'static str> {
    vec![
        crate::modules::nginx_ssl::sites::MIGRATION_SITE_LIMITS,
        crate::modules::scheduled::MIGRATION_SCHEDULED_TASKS,
        crate::modules::ssh::hosts::MIGRATION_SSH_HOSTS,
        crate::modules::http_client::MIGRATION_HTTP_REQUESTS,
        crate::modules::databases::connections::MIGRATION_DB_CONNECTIONS,
        crate::modules::browser::MIGRATION_BROWSER,
        crate::modules::databases::history::MIGRATION_DB_QUERY_HISTORY,
        crate::modules::remote_desktop::connections::MIGRATION_REMOTE_DESKTOPS,
    ]
}

fn all_migrations() -> Vec<&'static str> {
    let mut migrations = Vec::new();
    // FROZEN. These three lists are position-indexed against databases already in
    // the wild; do not add, remove, or reorder entries within them.
    migrations.extend(crate::auth::migrations());
    migrations.extend(crate::modules::nginx_ssl::migrations());
    migrations.extend(crate::modules::workers::migrations());
    // New migrations — from any module — go at the end of this one list.
    migrations.extend(later_migrations());
    migrations
}

pub fn open(app_data_dir: &Path) -> rusqlite::Result<Connection> {
    std::fs::create_dir_all(app_data_dir).expect("failed to create app data directory");
    let conn = Connection::open(app_data_dir.join("app.db"))?;
    // WAL + NORMAL: a write no longer fsyncs the journal twice, so the many
    // small writes (browser history on every page load, query history, task
    // logs) stop stalling whichever thread holds the connection. Durability
    // is still guaranteed across application crashes; only an OS crash can
    // lose the last few transactions, which is fine for a local dev tool.
    // busy_timeout covers the helper or a second instance touching the file.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let current_version = current_version as usize;

    for (index, sql) in all_migrations().into_iter().enumerate() {
        let version = index + 1;
        if version > current_version {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", version as i64)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_applies_all_migrations_and_sets_user_version() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version as usize, all_migrations().len());
    }

    /// Upgrading from *any* older version must succeed — the case the two tests
    /// below miss, because both start from an empty database and apply the whole
    /// list in one go.
    ///
    /// This is the regression test for a real startup crash: a migration appended
    /// to the middle of the concatenated list renumbered every migration after it,
    /// so an existing database re-ran them and died on "table workers already
    /// exists". Any future insertion into a frozen list fails here instead of on
    /// a user's machine.
    #[test]
    fn migrate_succeeds_from_every_intermediate_version() {
        let all = all_migrations();
        for version in 0..all.len() {
            let conn = Connection::open_in_memory().unwrap();
            // Reconstruct a database that stopped at `version`.
            for (index, sql) in all.iter().take(version).enumerate() {
                conn.execute_batch(sql).unwrap();
                conn.pragma_update(None, "user_version", (index + 1) as i64).unwrap();
            }
            migrate(&conn).unwrap_or_else(|e| {
                panic!("upgrading a database at version {version} failed: {e}")
            });
            let final_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
            assert_eq!(final_version as usize, all.len());
        }
    }

    #[test]
    fn migrate_twice_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        // Re-running must not re-execute already-applied migrations (several of
        // which are destructive table-rebuild steps that would fail the second
        // time round, e.g. `DROP TABLE` on a table already dropped).
        migrate(&conn).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version as usize, all_migrations().len());
    }
}
