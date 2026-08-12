//! One SQLite file, one migrations pipeline. Each module contributes its own ordered
//! migration SQL via a `migrations()` fn (see `crate::auth::migrations`); this module
//! just concatenates them in a fixed order and applies whatever hasn't run yet,
//! tracked via SQLite's built-in `PRAGMA user_version`.

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Db(pub Mutex<Connection>);

fn all_migrations() -> Vec<&'static str> {
    let mut migrations = Vec::new();
    migrations.extend(crate::auth::migrations());
    migrations.extend(crate::modules::nginx_ssl::migrations());
    // Future modules append here.
    migrations
}

pub fn open(app_data_dir: &Path) -> rusqlite::Result<Connection> {
    std::fs::create_dir_all(app_data_dir).expect("failed to create app data directory");
    let conn = Connection::open(app_data_dir.join("app.db"))?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
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
