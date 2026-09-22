//! Saved SSH hosts: the address book the terminal and SFTP browser connect
//! from. Only the *connection recipe* is stored — name, address, user, and how
//! to authenticate. A password is never written to the database: it's asked
//! for at connect time, handed to the session, and forgotten with it. Storing
//! it would need a real secret store to be responsible, and the key-based
//! flows this module pushes towards don't need one at all.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Appended to `db::later_migrations()` — see the note there on why new
/// migrations never go in a per-module list.
pub const MIGRATION_SSH_HOSTS: &str = r#"
CREATE TABLE ssh_hosts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    username TEXT NOT NULL,
    auth TEXT NOT NULL,
    key_path TEXT,
    created_at TEXT NOT NULL
);
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SshHost {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// "agent" | "key" | "password". A string rather than an enum so the
    /// column and the TypeScript side share one spelling with no mapping.
    pub auth: String,
    pub key_path: Option<String>,
    pub created_at: String,
}

pub const AUTH_KINDS: [&str; 3] = ["agent", "key", "password"];

const COLUMNS: &str = "id, name, host, port, username, auth, key_path, created_at";

fn row_to_host(row: &rusqlite::Row) -> rusqlite::Result<SshHost> {
    Ok(SshHost {
        id: row.get(0)?,
        name: row.get(1)?,
        host: row.get(2)?,
        port: row.get::<_, i64>(3)? as u16,
        username: row.get(4)?,
        auth: row.get(5)?,
        key_path: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<SshHost>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLUMNS} FROM ssh_hosts ORDER BY name COLLATE NOCASE"))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_host).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

pub fn find(conn: &Connection, id: &str) -> Result<SshHost, String> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM ssh_hosts WHERE id = ?1"), [id], row_to_host)
        .map_err(|_| "host not found".to_string())
}

/// The validated, trimmed form of a host as the UI submits it.
#[derive(Debug, Clone, PartialEq)]
pub struct HostInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: String,
    pub key_path: Option<String>,
}

pub fn validate(
    name: &str,
    host: &str,
    port: i64,
    username: &str,
    auth: &str,
    key_path: Option<&str>,
) -> Result<HostInput, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    let host = host.trim();
    if host.is_empty() {
        return Err("host cannot be empty".to_string());
    }
    if host.chars().any(|c| c.is_whitespace()) {
        return Err("host cannot contain spaces".to_string());
    }
    if !(1..=65535).contains(&port) {
        return Err("port must be between 1 and 65535".to_string());
    }
    let username = username.trim();
    if username.is_empty() {
        return Err("username cannot be empty".to_string());
    }
    if !AUTH_KINDS.contains(&auth) {
        return Err(format!("unknown auth method \"{auth}\" (expected agent, key or password)"));
    }
    let key_path = key_path.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if auth == "key" && key_path.is_none() {
        return Err("a private key file is required for key authentication".to_string());
    }
    Ok(HostInput {
        name: name.to_string(),
        host: host.to_string(),
        port: port as u16,
        username: username.to_string(),
        auth: auth.to_string(),
        // Only meaningful for key auth; dropping it otherwise keeps the row
        // honest about what will actually be used to connect.
        key_path: if auth == "key" { key_path } else { None },
    })
}

#[tauri::command]
pub fn ssh_hosts_list(db: State<crate::db::Db>) -> Result<Vec<SshHost>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list(&conn)
}

/// Insert when `id` is absent, update when present — one command, because the
/// form is the same either way and the UI shouldn't have to care.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn ssh_hosts_save(
    db: State<crate::db::Db>,
    id: Option<String>,
    name: String,
    host: String,
    port: i64,
    username: String,
    auth: String,
    key_path: Option<String>,
) -> Result<SshHost, String> {
    let input = validate(&name, &host, port, &username, &auth, key_path.as_deref())?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    match id {
        Some(id) => {
            find(&conn, &id)?;
            conn.execute(
                "UPDATE ssh_hosts SET name = ?1, host = ?2, port = ?3, username = ?4, auth = ?5, key_path = ?6 WHERE id = ?7",
                (
                    &input.name,
                    &input.host,
                    input.port as i64,
                    &input.username,
                    &input.auth,
                    &input.key_path,
                    &id,
                ),
            )
            .map_err(|e| e.to_string())?;
            find(&conn, &id)
        }
        None => {
            let record = SshHost {
                id: uuid::Uuid::new_v4().to_string(),
                name: input.name,
                host: input.host,
                port: input.port,
                username: input.username,
                auth: input.auth,
                key_path: input.key_path,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            conn.execute(
                &format!("INSERT INTO ssh_hosts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
                (
                    &record.id,
                    &record.name,
                    &record.host,
                    record.port as i64,
                    &record.username,
                    &record.auth,
                    &record.key_path,
                    &record.created_at,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(record)
        }
    }
}

#[tauri::command]
pub fn ssh_hosts_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM ssh_hosts WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_trims_and_accepts_a_plain_host() {
        let input = validate("  Prod  ", " example.com ", 22, " deploy ", "agent", None).unwrap();
        assert_eq!(input.name, "Prod");
        assert_eq!(input.host, "example.com");
        assert_eq!(input.username, "deploy");
        assert_eq!(input.port, 22);
        assert_eq!(input.key_path, None);
    }

    #[test]
    fn validate_rejects_blank_fields_and_bad_ports() {
        assert!(validate("", "h", 22, "u", "agent", None).is_err());
        assert!(validate("n", "  ", 22, "u", "agent", None).is_err());
        assert!(validate("n", "h", 22, "", "agent", None).is_err());
        assert!(validate("n", "h", 0, "u", "agent", None).is_err());
        assert!(validate("n", "h", 70000, "u", "agent", None).is_err());
        assert!(validate("n", "my host", 22, "u", "agent", None).is_err());
    }

    #[test]
    fn validate_requires_a_key_path_for_key_auth_and_drops_it_otherwise() {
        assert!(validate("n", "h", 22, "u", "key", None).is_err());
        assert!(validate("n", "h", 22, "u", "key", Some("  ")).is_err());
        let with_key = validate("n", "h", 22, "u", "key", Some("/home/me/.ssh/id_ed25519")).unwrap();
        assert_eq!(with_key.key_path.as_deref(), Some("/home/me/.ssh/id_ed25519"));
        let pw = validate("n", "h", 22, "u", "password", Some("/home/me/.ssh/id_ed25519")).unwrap();
        assert_eq!(pw.key_path, None);
    }

    #[test]
    fn validate_rejects_unknown_auth() {
        assert!(validate("n", "h", 22, "u", "kerberos", None).is_err());
    }

    #[test]
    fn list_and_find_round_trip_through_sqlite() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            &format!("INSERT INTO ssh_hosts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
            ("id-1", "Box", "10.0.0.5", 2222_i64, "root", "password", Option::<String>::None, "now"),
        )
        .unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].port, 2222);
        assert_eq!(find(&conn, "id-1").unwrap().name, "Box");
        assert!(find(&conn, "missing").is_err());
    }
}
