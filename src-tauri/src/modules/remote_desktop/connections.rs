//! Saved remote desktop connections: the address book the VNC viewer and RDP
//! launcher connect from. Only the connection recipe is stored — protocol,
//! address, user, and a few display preferences. Passwords are asked for at
//! connect time and never persisted (see the module note).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Appended to `db::later_migrations()` — see the note there on why new
/// migrations never go in a per-module list.
pub const MIGRATION_REMOTE_DESKTOPS: &str = r#"
CREATE TABLE remote_desktops (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    username TEXT NOT NULL DEFAULT '',
    domain TEXT NOT NULL DEFAULT '',
    view_only INTEGER NOT NULL DEFAULT 0,
    scale INTEGER NOT NULL DEFAULT 1,
    fullscreen INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
"#;

pub const PROTOCOLS: [&str; 2] = ["vnc", "rdp"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteDesktop {
    pub id: String,
    pub name: String,
    /// "vnc" | "rdp". A string rather than an enum so the column and the
    /// TypeScript side share one spelling with no mapping.
    pub protocol: String,
    pub host: String,
    pub port: u16,
    /// RDP: required. VNC: only used by servers that ask for one.
    pub username: String,
    /// RDP only: the Windows domain, empty for a local account.
    pub domain: String,
    /// VNC only: look, don't touch.
    pub view_only: bool,
    /// VNC only: shrink the remote screen to fit the viewer.
    pub scale: bool,
    /// RDP only: ask the client for a fullscreen window.
    pub fullscreen: bool,
    pub created_at: String,
}

const COLUMNS: &str =
    "id, name, protocol, host, port, username, domain, view_only, scale, fullscreen, created_at";

fn row_to_desktop(row: &rusqlite::Row) -> rusqlite::Result<RemoteDesktop> {
    Ok(RemoteDesktop {
        id: row.get(0)?,
        name: row.get(1)?,
        protocol: row.get(2)?,
        host: row.get(3)?,
        port: row.get::<_, i64>(4)? as u16,
        username: row.get(5)?,
        domain: row.get(6)?,
        view_only: row.get::<_, i64>(7)? != 0,
        scale: row.get::<_, i64>(8)? != 0,
        fullscreen: row.get::<_, i64>(9)? != 0,
        created_at: row.get(10)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<RemoteDesktop>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM remote_desktops ORDER BY name COLLATE NOCASE"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_desktop).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

pub fn find(conn: &Connection, id: &str) -> Result<RemoteDesktop, String> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM remote_desktops WHERE id = ?1"),
        [id],
        row_to_desktop,
    )
    .map_err(|_| "connection not found".to_string())
}

/// The validated, trimmed form of a connection as the UI submits it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DesktopInput {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: i64,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub view_only: bool,
    #[serde(default = "default_true")]
    pub scale: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

fn default_true() -> bool {
    true
}

pub fn validate(input: DesktopInput) -> Result<DesktopInput, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if !PROTOCOLS.contains(&input.protocol.as_str()) {
        return Err(format!(
            "unknown protocol \"{}\" (expected vnc or rdp)",
            input.protocol
        ));
    }
    let host = input.host.trim();
    if host.is_empty() {
        return Err("host cannot be empty".to_string());
    }
    if host.chars().any(|c| c.is_whitespace()) {
        return Err("host cannot contain spaces".to_string());
    }
    if !(1..=65535).contains(&input.port) {
        return Err("port must be between 1 and 65535".to_string());
    }
    let username = input.username.trim();
    if input.protocol == "rdp" && username.is_empty() {
        return Err("a username is required for RDP".to_string());
    }
    let is_rdp = input.protocol == "rdp";
    Ok(DesktopInput {
        name: name.to_string(),
        protocol: input.protocol,
        host: host.to_string(),
        port: input.port,
        username: username.to_string(),
        // Each preference only means something for one protocol; dropping
        // the others keeps the row honest about what will be used.
        domain: if is_rdp { input.domain.trim().to_string() } else { String::new() },
        view_only: !is_rdp && input.view_only,
        scale: !is_rdp && input.scale,
        fullscreen: is_rdp && input.fullscreen,
    })
}

#[tauri::command(async)]
pub fn remote_desktop_list(db: State<crate::db::Db>) -> Result<Vec<RemoteDesktop>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    list(&conn)
}

/// Insert when `id` is absent, update when present — one command, because the
/// form is the same either way and the UI shouldn't have to care.
#[tauri::command(async)]
pub fn remote_desktop_save(
    db: State<crate::db::Db>,
    id: Option<String>,
    input: DesktopInput,
) -> Result<RemoteDesktop, String> {
    let input = validate(input)?;
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    match id {
        Some(id) => {
            find(&conn, &id)?;
            conn.execute(
                "UPDATE remote_desktops SET name = ?1, protocol = ?2, host = ?3, port = ?4, username = ?5, domain = ?6, view_only = ?7, scale = ?8, fullscreen = ?9 WHERE id = ?10",
                (
                    &input.name,
                    &input.protocol,
                    &input.host,
                    input.port,
                    &input.username,
                    &input.domain,
                    input.view_only as i64,
                    input.scale as i64,
                    input.fullscreen as i64,
                    &id,
                ),
            )
            .map_err(|e| e.to_string())?;
            find(&conn, &id)
        }
        None => {
            let record = RemoteDesktop {
                id: uuid::Uuid::new_v4().to_string(),
                name: input.name,
                protocol: input.protocol,
                host: input.host,
                port: input.port as u16,
                username: input.username,
                domain: input.domain,
                view_only: input.view_only,
                scale: input.scale,
                fullscreen: input.fullscreen,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            conn.execute(
                &format!("INSERT INTO remote_desktops ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"),
                (
                    &record.id,
                    &record.name,
                    &record.protocol,
                    &record.host,
                    record.port as i64,
                    &record.username,
                    &record.domain,
                    record.view_only as i64,
                    record.scale as i64,
                    record.fullscreen as i64,
                    &record.created_at,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(record)
        }
    }
}

#[tauri::command(async)]
pub fn remote_desktop_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM remote_desktops WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(protocol: &str) -> DesktopInput {
        DesktopInput {
            name: "  Office PC ".into(),
            protocol: protocol.into(),
            host: " 10.0.0.9 ".into(),
            port: if protocol == "rdp" { 3389 } else { 5900 },
            username: " alice ".into(),
            domain: " CORP ".into(),
            view_only: true,
            scale: true,
            fullscreen: true,
        }
    }

    #[test]
    fn validate_trims_and_keeps_only_the_protocols_own_preferences() {
        let vnc = validate(input("vnc")).unwrap();
        assert_eq!(vnc.name, "Office PC");
        assert_eq!(vnc.host, "10.0.0.9");
        assert_eq!(vnc.username, "alice");
        assert_eq!(vnc.domain, "");
        assert!(vnc.view_only && vnc.scale && !vnc.fullscreen);

        let rdp = validate(input("rdp")).unwrap();
        assert_eq!(rdp.domain, "CORP");
        assert!(!rdp.view_only && !rdp.scale && rdp.fullscreen);
    }

    #[test]
    fn validate_rejects_blank_fields_bad_ports_and_unknown_protocols() {
        assert!(validate(DesktopInput { name: " ".into(), ..input("vnc") }).is_err());
        assert!(validate(DesktopInput { host: "".into(), ..input("vnc") }).is_err());
        assert!(validate(DesktopInput { host: "my host".into(), ..input("vnc") }).is_err());
        assert!(validate(DesktopInput { port: 0, ..input("vnc") }).is_err());
        assert!(validate(DesktopInput { port: 70000, ..input("vnc") }).is_err());
        assert!(validate(DesktopInput { protocol: "x11".into(), ..input("vnc") }).is_err());
    }

    #[test]
    fn validate_requires_a_username_for_rdp_only() {
        assert!(validate(DesktopInput { username: " ".into(), ..input("rdp") }).is_err());
        assert!(validate(DesktopInput { username: "".into(), ..input("vnc") }).is_ok());
    }

    #[test]
    fn list_and_find_round_trip_through_sqlite() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn.execute(
            &format!("INSERT INTO remote_desktops ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"),
            ("id-1", "Box", "rdp", "10.0.0.5", 3390_i64, "bob", "", 0_i64, 0_i64, 1_i64, "now"),
        )
        .unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].port, 3390);
        assert!(all[0].fullscreen);
        assert!(!all[0].scale);
        assert_eq!(find(&conn, "id-1").unwrap().name, "Box");
        assert!(find(&conn, "missing").is_err());
    }
}
