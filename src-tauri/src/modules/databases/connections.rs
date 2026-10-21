//! Saved *custom* database connections — the ones DevsPro does not run itself.
//!
//! The Databases module was previously a closed world: the only thing the
//! browser could open was a MySQL/Postgres service inside DevsPro's own compose
//! project, discovered by reading that compose file. This table is the other
//! half — a server somewhere else, reached one of two ways:
//!
//!   * `docker` — a container on this machine that DevsPro didn't create (an
//!     app's own `docker compose` stack). The client runs *inside* it via
//!     `docker exec`, exactly like a managed service, so no port has to be
//!     published to the host.
//!   * `tcp` — a host and port. Covers both a database installed directly on
//!     this machine and a remote one; the only difference between those two is
//!     what gets typed into the host field, so they are not separate kinds
//!     here (the UI still offers them as separate starting points).
//!
//! Unlike `ssh::hosts`, the password *is* stored, because a query browser that
//! re-prompted on every page of every table would be unusable. It is stored in
//! plain text in the app's SQLite file, which is the same protection the
//! managed databases already get (their passwords sit in DevsPro's compose
//! file) — the UI says so on the form. It is never serialized back to the
//! frontend: `ConnectionView` reports only whether one is set.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Appended to `db::later_migrations()` — see the note there on why new
/// migrations never go in a per-module list.
pub const MIGRATION_DB_CONNECTIONS: &str = r#"
CREATE TABLE db_connections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    transport TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    container TEXT NOT NULL,
    username TEXT NOT NULL,
    password TEXT NOT NULL,
    default_database TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

/// Engines the browser can talk to. Mongo and Redis have no tabular model, so
/// a saved connection to one would have nothing to open.
pub const KINDS: [&str; 2] = ["mysql", "postgres"];
pub const TRANSPORTS: [&str; 2] = ["tcp", "docker"];

/// A stored row. Strings rather than enums for `kind`/`transport` so the
/// column, the Rust value and the TypeScript union all share one spelling.
#[derive(Debug, Clone, PartialEq)]
pub struct DbConnection {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub transport: String,
    pub host: String,
    pub port: u16,
    pub container: String,
    pub username: String,
    pub password: String,
    pub default_database: String,
    pub created_at: String,
}

/// What the frontend sees. Identical to the row minus the password, which
/// never leaves the backend — the form starts blank when editing and only
/// sends a value when it was actually changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConnectionView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub transport: String,
    pub host: String,
    pub port: u16,
    pub container: String,
    pub username: String,
    pub has_password: bool,
    pub default_database: String,
    pub created_at: String,
}

impl From<&DbConnection> for ConnectionView {
    fn from(c: &DbConnection) -> Self {
        ConnectionView {
            id: c.id.clone(),
            name: c.name.clone(),
            kind: c.kind.clone(),
            transport: c.transport.clone(),
            host: c.host.clone(),
            port: c.port,
            container: c.container.clone(),
            username: c.username.clone(),
            has_password: !c.password.is_empty(),
            default_database: c.default_database.clone(),
            created_at: c.created_at.clone(),
        }
    }
}

const COLUMNS: &str =
    "id, name, kind, transport, host, port, container, username, password, default_database, created_at";

fn row_to_connection(row: &rusqlite::Row) -> rusqlite::Result<DbConnection> {
    Ok(DbConnection {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        transport: row.get(3)?,
        host: row.get(4)?,
        port: row.get::<_, i64>(5)? as u16,
        container: row.get(6)?,
        username: row.get(7)?,
        password: row.get(8)?,
        default_database: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<DbConnection>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT {COLUMNS} FROM db_connections ORDER BY name COLLATE NOCASE"))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_connection).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

pub fn find(conn: &Connection, id: &str) -> Result<DbConnection, String> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM db_connections WHERE id = ?1"), [id], row_to_connection)
        .map_err(|_| format!("saved connection '{id}' no longer exists"))
}

/// The form as the UI submits it. `password: None` means "leave whatever is
/// stored alone" (the field starts blank when editing); `Some("")` clears it,
/// which is what a trust-authenticated local Postgres needs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    pub transport: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: i64,
    #[serde(default)]
    pub container: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub default_database: String,
}

/// The validated, trimmed form of a connection. Fields that don't apply to the
/// chosen transport are blanked rather than carried along, so a row always
/// describes exactly what will be dialled.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidConnection {
    pub name: String,
    pub kind: String,
    pub transport: String,
    pub host: String,
    pub port: u16,
    pub container: String,
    pub username: String,
    pub default_database: String,
}

pub fn validate(input: &ConnectionInput) -> Result<ValidConnection, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if !KINDS.contains(&input.kind.as_str()) {
        return Err(format!("unknown engine \"{}\" (expected mysql or postgres)", input.kind));
    }
    if !TRANSPORTS.contains(&input.transport.as_str()) {
        return Err(format!("unknown connection type \"{}\" (expected tcp or docker)", input.transport));
    }

    let host = input.host.trim();
    let container = input.container.trim();
    let tcp = input.transport == "tcp";
    if tcp {
        if host.is_empty() {
            return Err("host cannot be empty".to_string());
        }
        if host.chars().any(char::is_whitespace) {
            return Err("host cannot contain spaces".to_string());
        }
        if !(1..=65535).contains(&input.port) {
            return Err("port must be between 1 and 65535".to_string());
        }
    } else {
        if container.is_empty() {
            return Err("pick the container the database runs in".to_string());
        }
        if container.chars().any(char::is_whitespace) {
            return Err("container name cannot contain spaces".to_string());
        }
    }

    let username = input.username.trim();
    if username.is_empty() {
        return Err("username cannot be empty".to_string());
    }
    if username.chars().any(char::is_whitespace) {
        return Err("username cannot contain spaces".to_string());
    }

    let default_database = input.default_database.trim();
    if !default_database.is_empty() && !super::browse::valid_identifier(default_database) {
        return Err(format!("'{default_database}' is not a usable database name"));
    }

    Ok(ValidConnection {
        name: name.to_string(),
        kind: input.kind.clone(),
        transport: input.transport.clone(),
        host: if tcp { host.to_string() } else { String::new() },
        port: if tcp { input.port as u16 } else { 0 },
        container: if tcp { String::new() } else { container.to_string() },
        username: username.to_string(),
        default_database: default_database.to_string(),
    })
}

/// Resolves the password to store: what the form sent, or — when it sent
/// nothing because the field was left untouched — whatever is already saved.
fn resolved_password(conn: &Connection, input: &ConnectionInput) -> Result<String, String> {
    match &input.password {
        Some(p) => Ok(p.clone()),
        None => match &input.id {
            Some(id) => Ok(find(conn, id)?.password),
            None => Ok(String::new()),
        },
    }
}

pub fn save(conn: &Connection, input: &ConnectionInput) -> Result<DbConnection, String> {
    let valid = validate(input)?;
    let password = resolved_password(conn, input)?;
    match &input.id {
        Some(id) => {
            let existing = find(conn, id)?;
            conn.execute(
                "UPDATE db_connections SET name = ?1, kind = ?2, transport = ?3, host = ?4, port = ?5,
                 container = ?6, username = ?7, password = ?8, default_database = ?9 WHERE id = ?10",
                (
                    &valid.name,
                    &valid.kind,
                    &valid.transport,
                    &valid.host,
                    valid.port as i64,
                    &valid.container,
                    &valid.username,
                    &password,
                    &valid.default_database,
                    id,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(DbConnection { password, created_at: existing.created_at, id: id.clone(), ..from_valid(valid) })
        }
        None => {
            let record = DbConnection {
                id: uuid::Uuid::new_v4().to_string(),
                password,
                created_at: chrono::Utc::now().to_rfc3339(),
                ..from_valid(valid)
            };
            conn.execute(
                &format!("INSERT INTO db_connections ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"),
                (
                    &record.id,
                    &record.name,
                    &record.kind,
                    &record.transport,
                    &record.host,
                    record.port as i64,
                    &record.container,
                    &record.username,
                    &record.password,
                    &record.default_database,
                    &record.created_at,
                ),
            )
            .map_err(|e| e.to_string())?;
            Ok(record)
        }
    }
}

/// The validated form with the identity fields left blank, for the two callers
/// above to fill in.
fn from_valid(valid: ValidConnection) -> DbConnection {
    DbConnection {
        id: String::new(),
        name: valid.name,
        kind: valid.kind,
        transport: valid.transport,
        host: valid.host,
        port: valid.port,
        container: valid.container,
        username: valid.username,
        password: String::new(),
        default_database: valid.default_database,
        created_at: String::new(),
    }
}

/// The row a test should run against: a draft the user has not saved yet, but
/// falling back to the stored password when the form left that field alone.
pub fn draft(conn: &Connection, input: &ConnectionInput) -> Result<DbConnection, String> {
    let valid = validate(input)?;
    let password = resolved_password(conn, input)?;
    Ok(DbConnection {
        id: input.id.clone().unwrap_or_default(),
        password,
        created_at: String::new(),
        ..from_valid(valid)
    })
}

// ---------------------------------------------------------------------------
// Read-only / read & write mode
//
// Two modes, one per connection, so the usual accident — editing production
// while meaning to edit a local copy — takes a deliberate switch rather than a
// steady hand. The mode is enforced in the backend, not merely drawn in the
// UI: every write path asks `require_writable` first.
//
// It lives in the generic `settings` table rather than a column on
// `db_connections`, because a DevsPro-managed service has no row there and
// deserves the same guard. One key shape covers both kinds of connection.

pub const MODE_READ_ONLY: &str = "ro";
pub const MODE_READ_WRITE: &str = "rw";

fn mode_key(connection: &str) -> String {
    format!("databases.mode.{connection}")
}

/// Reads a connection's mode. Anything unset — or unrecognised, if a future
/// version writes a third value and this one reads it back — is read-only:
/// the safe answer is the one to fall back to.
pub fn mode(conn: &Connection, connection: &str) -> String {
    match crate::modules::settings::get(conn, &mode_key(connection)).as_deref() {
        Some(MODE_READ_WRITE) => MODE_READ_WRITE.to_string(),
        _ => MODE_READ_ONLY.to_string(),
    }
}

pub fn is_writable(conn: &Connection, connection: &str) -> bool {
    mode(conn, connection) == MODE_READ_WRITE
}

#[tauri::command(async)]
pub fn databases_mode_get(db: State<crate::db::Db>, connection: String) -> Result<String, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(mode(&conn, &connection))
}

#[tauri::command(async)]
pub fn databases_mode_set(
    db: State<crate::db::Db>,
    connection: String,
    mode: String,
) -> Result<String, String> {
    if mode != MODE_READ_ONLY && mode != MODE_READ_WRITE {
        return Err(format!("unknown mode '{mode}'"));
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (&mode_key(&connection), &mode),
    )
    .map_err(|e| e.to_string())?;
    Ok(mode)
}

// ---------------------------------------------------------------------------
// Reading a container's own credentials
//
// A database container already carries everything needed to connect to it in
// its environment — the official images take the admin user, password and
// initial database from exactly those variables at first start. Reading them
// back turns picking a container into filling the whole form, instead of the
// user going to look up a password they set months ago in someone else's
// compose file.

/// What a container's environment says about the database inside it. Every
/// field is optional: a container built from a custom image may set none of
/// them, and the form then keeps whatever is already typed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ContainerCredentials {
    /// "mysql" or "postgres", when the variables make it unambiguous.
    pub kind: Option<String>,
    pub username: String,
    pub password: String,
    pub database: String,
}

fn env_value(env: &[String], key: &str) -> Option<String> {
    env.iter()
        .find_map(|entry| entry.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v.to_string()))
        .filter(|v| !v.is_empty())
}

/// Takes the first of `keys` that is set — the MariaDB images accept both
/// `MARIADB_*` and the older `MYSQL_*` spellings, and set both when started
/// from the former.
fn first_env(env: &[String], keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| env_value(env, k))
}

/// Derives connection details from a container's `Config.Env`.
///
/// The *root* credentials win over `MYSQL_USER`/`MYSQL_PASSWORD` where both
/// are set: the browser's first act is to list every database on the server,
/// which the scoped user created by those variables is usually not granted.
pub fn credentials_from_env(env: &[String]) -> ContainerCredentials {
    if let Some(password) = first_env(env, &["MARIADB_ROOT_PASSWORD", "MYSQL_ROOT_PASSWORD"]) {
        return ContainerCredentials {
            kind: Some("mysql".to_string()),
            username: "root".to_string(),
            password,
            database: first_env(env, &["MARIADB_DATABASE", "MYSQL_DATABASE"]).unwrap_or_default(),
        };
    }
    if let Some(password) = first_env(env, &["MARIADB_PASSWORD", "MYSQL_PASSWORD"]) {
        return ContainerCredentials {
            kind: Some("mysql".to_string()),
            username: first_env(env, &["MARIADB_USER", "MYSQL_USER"]).unwrap_or_default(),
            password,
            database: first_env(env, &["MARIADB_DATABASE", "MYSQL_DATABASE"]).unwrap_or_default(),
        };
    }
    if let Some(password) = env_value(env, "POSTGRES_PASSWORD") {
        let username = env_value(env, "POSTGRES_USER").unwrap_or_else(|| "postgres".to_string());
        return ContainerCredentials {
            kind: Some("postgres".to_string()),
            // POSTGRES_DB defaults to the user's own name when unset, which is
            // the database psql would connect to anyway.
            database: env_value(env, "POSTGRES_DB").unwrap_or_else(|| username.clone()),
            username,
            password,
        };
    }
    ContainerCredentials::default()
}

/// Reads one container's environment. The name reaches `docker`'s argv, so it
/// is checked rather than passed through.
#[tauri::command(async)]
pub fn databases_inspect_container(name: String) -> Result<ContainerCredentials, String> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_whitespace) {
        return Err(format!("'{name}' is not a usable container name"));
    }
    let output = crate::modules::tools::command("docker")
        .args(["inspect", "--format", "{{json .Config.Env}}", &name])
        .output()
        .map_err(|e| format!("failed to run docker inspect: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let env: Vec<String> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not read the container's environment: {e}"))?;
    Ok(credentials_from_env(&env))
}

// ---------------------------------------------------------------------------
// Commands

#[tauri::command(async)]
pub fn databases_connections_list(db: State<crate::db::Db>) -> Result<Vec<ConnectionView>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(list(&conn)?.iter().map(ConnectionView::from).collect())
}

/// Insert when `input.id` is absent, update when present — one command,
/// because the form is the same either way.
#[tauri::command(async)]
pub fn databases_connections_save(
    db: State<crate::db::Db>,
    input: ConnectionInput,
) -> Result<ConnectionView, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(ConnectionView::from(&save(&conn, &input)?))
}

#[tauri::command(async)]
pub fn databases_connections_delete(db: State<crate::db::Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM db_connections WHERE id = ?1", [&id]).map_err(|e| e.to_string())?;
    // The mode lives under the connection's browse ref, not its row, so it has
    // to be cleared here or a forgotten connection would leave a stray setting.
    let _ = conn.execute("DELETE FROM settings WHERE key = ?1", [&mode_key(&format!("custom:{id}"))]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ConnectionInput {
        ConnectionInput {
            id: None,
            name: "  Prod  ".to_string(),
            kind: "mysql".to_string(),
            transport: "tcp".to_string(),
            host: " db.example.com ".to_string(),
            port: 3306,
            container: "ignored".to_string(),
            username: " root ".to_string(),
            password: Some("s3cret".to_string()),
            default_database: " shop ".to_string(),
        }
    }

    #[test]
    fn validate_trims_and_blanks_the_fields_the_transport_does_not_use() {
        let v = validate(&input()).unwrap();
        assert_eq!(v.name, "Prod");
        assert_eq!(v.host, "db.example.com");
        assert_eq!(v.username, "root");
        assert_eq!(v.default_database, "shop");
        // tcp was chosen, so the container field is not carried along.
        assert_eq!(v.container, "");

        let docker = ConnectionInput {
            transport: "docker".to_string(),
            container: " app_mysql_1 ".to_string(),
            ..input()
        };
        let v = validate(&docker).unwrap();
        assert_eq!(v.container, "app_mysql_1");
        assert_eq!(v.host, "");
        assert_eq!(v.port, 0);
    }

    #[test]
    fn validate_rejects_blank_fields_bad_ports_and_unknown_kinds() {
        let cases = [
            ConnectionInput { name: "  ".to_string(), ..input() },
            ConnectionInput { kind: "mongo".to_string(), ..input() },
            ConnectionInput { transport: "carrier pigeon".to_string(), ..input() },
            ConnectionInput { host: "  ".to_string(), ..input() },
            ConnectionInput { host: "a host".to_string(), ..input() },
            ConnectionInput { port: 0, ..input() },
            ConnectionInput { port: 70000, ..input() },
            ConnectionInput { username: "".to_string(), ..input() },
            ConnectionInput { transport: "docker".to_string(), container: " ".to_string(), ..input() },
        ];
        for case in cases {
            assert!(validate(&case).is_err(), "{case:?} should not validate");
        }
    }

    #[test]
    fn a_default_database_is_checked_against_the_browser_identifier_rule() {
        assert!(validate(&ConnectionInput { default_database: "my-app_db".to_string(), ..input() }).is_ok());
        // A leading dash would reach the client binary's argv as a flag.
        assert!(validate(&ConnectionInput { default_database: "-h".to_string(), ..input() }).is_err());
        assert!(validate(&ConnectionInput { default_database: "drop; table".to_string(), ..input() }).is_err());
    }

    #[test]
    fn an_unset_or_unrecognised_mode_reads_as_read_only() {
        let conn = memory_db();
        assert_eq!(mode(&conn, "shop"), MODE_READ_ONLY);
        assert!(!is_writable(&conn, "shop"));
        conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2)", (mode_key("shop"), "something-else"))
            .unwrap();
        assert_eq!(mode(&conn, "shop"), MODE_READ_ONLY);
    }

    #[test]
    fn a_mode_is_per_connection() {
        let conn = memory_db();
        conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2)", (mode_key("shop"), MODE_READ_WRITE))
            .unwrap();
        assert!(is_writable(&conn, "shop"));
        assert!(!is_writable(&conn, "custom:prod"));
    }

    #[test]
    fn root_credentials_win_over_the_scoped_user_when_both_are_set() {
        // The scoped MYSQL_USER usually cannot list the other databases on the
        // server, which is the first thing the browser does.
        let env = [
            "MYSQL_ROOT_PASSWORD=rootpw",
            "MYSQL_USER=app",
            "MYSQL_PASSWORD=apppw",
            "MYSQL_DATABASE=shop",
        ]
        .map(String::from);
        assert_eq!(
            credentials_from_env(&env),
            ContainerCredentials {
                kind: Some("mysql".to_string()),
                username: "root".to_string(),
                password: "rootpw".to_string(),
                database: "shop".to_string(),
            }
        );
    }

    #[test]
    fn a_scoped_mysql_user_is_used_when_there_is_no_root_password() {
        let env = ["MYSQL_USER=app", "MYSQL_PASSWORD=apppw"].map(String::from);
        let creds = credentials_from_env(&env);
        assert_eq!(creds.username, "app");
        assert_eq!(creds.password, "apppw");
    }

    #[test]
    fn mariadb_spellings_are_read_as_mysql() {
        let env = ["MARIADB_ROOT_PASSWORD=pw", "MARIADB_DATABASE=shop"].map(String::from);
        let creds = credentials_from_env(&env);
        assert_eq!(creds.kind.as_deref(), Some("mysql"));
        assert_eq!(creds.password, "pw");
        assert_eq!(creds.database, "shop");
    }

    #[test]
    fn postgres_falls_back_to_its_documented_defaults() {
        // POSTGRES_USER defaults to postgres, and POSTGRES_DB to the user.
        let creds = credentials_from_env(&["POSTGRES_PASSWORD=pw".to_string()]);
        assert_eq!(
            creds,
            ContainerCredentials {
                kind: Some("postgres".to_string()),
                username: "postgres".to_string(),
                password: "pw".to_string(),
                database: "postgres".to_string(),
            }
        );
        let named = ["POSTGRES_PASSWORD=pw", "POSTGRES_USER=app"].map(String::from);
        assert_eq!(credentials_from_env(&named).database, "app");
    }

    #[test]
    fn an_unrecognised_container_yields_nothing_rather_than_a_guess() {
        let env = ["PATH=/usr/bin".to_string(), "LANG=C.UTF-8".to_string(), "EMPTY=".to_string()];
        assert_eq!(credentials_from_env(&env), ContainerCredentials::default());
    }

    #[test]
    fn a_value_containing_an_equals_sign_survives_the_split() {
        let env = ["POSTGRES_PASSWORD=a=b=c".to_string()];
        assert_eq!(credentials_from_env(&env).password, "a=b=c");
    }

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn save_round_trips_and_updates_in_place() {
        let conn = memory_db();
        let saved = save(&conn, &input()).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 1);

        let renamed =
            ConnectionInput { id: Some(saved.id.clone()), name: "Staging".to_string(), password: None, ..input() };
        let updated = save(&conn, &renamed).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 1, "an edit must not insert a second row");
        assert_eq!(updated.name, "Staging");
        assert_eq!(updated.created_at, saved.created_at);
    }

    #[test]
    fn an_untouched_password_field_keeps_the_stored_one_and_an_empty_one_clears_it() {
        let conn = memory_db();
        let saved = save(&conn, &input()).unwrap();

        let untouched = ConnectionInput { id: Some(saved.id.clone()), password: None, ..input() };
        assert_eq!(save(&conn, &untouched).unwrap().password, "s3cret");

        let cleared = ConnectionInput { id: Some(saved.id.clone()), password: Some(String::new()), ..input() };
        assert_eq!(save(&conn, &cleared).unwrap().password, "");
    }

    #[test]
    fn the_view_reports_whether_a_password_is_set_without_revealing_it() {
        let conn = memory_db();
        let saved = save(&conn, &input()).unwrap();
        let view = ConnectionView::from(&saved);
        assert!(view.has_password);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("s3cret"), "the password must never be serialized: {json}");
    }

    #[test]
    fn find_on_a_removed_connection_explains_itself() {
        let conn = memory_db();
        let err = find(&conn, "gone").unwrap_err();
        assert!(err.contains("no longer exists"), "{err}");
    }
}
