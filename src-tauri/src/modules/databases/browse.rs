//! Database browser: list databases/tables, page through rows, run ad-hoc SQL,
//! and dump/restore to a user-chosen file — for the MySQL and PostgreSQL
//! containers the Databases module manages.
//!
//! Same approach as user management in the parent module: drive the official
//! `mysql` / `psql` client binaries rather than adding a native driver crate
//! per engine. Where that client is *found* depends on the connection (see
//! `Runner`): inside the server's own container for a managed service or a
//! saved Docker connection, on this machine's PATH for a host/remote one, and
//! — when neither is true — inside a throwaway container, so a remote database
//! can still be browsed on a machine with no client installed.
//!
//! The cost of driving a CLI is that everything comes back as text, so the
//! client is asked for its most machine-readable format and the parsing lives
//! in small pure functions with tests:
//!
//!   * `mysql --batch` prints tab-separated rows with a header line and escapes
//!     tabs/newlines/backslashes inside values (`\t`, `\n`, `\\`, `\0`), so a
//!     line-based parse is exact once those are unescaped. `--raw` is deliberately
//!     *not* used: it disables that escaping and makes a tab inside a value
//!     indistinguishable from a field separator.
//!   * `psql --csv` prints RFC 4180 CSV, where a NULL is an unquoted empty
//!     field and an empty string is a quoted `""` — the parser tracks quoting so
//!     the two stay distinguishable.
//!
//! Mongo and Redis have no tabular model and are rejected up front.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::State;

use super::connections::DbConnection;
use super::edit::{
    edit_batch_sql, quote_literal, relation_count_columns, resolve, where_clause, Filter, PendingEdit,
    RelationSpec,
};
use super::{export, schema};
use super::{admin_context, command_failed, container_name, DbKind};

/// Marker used for SQL NULL in the row grid. The frontend shows it muted.
pub const NULL_MARKER: &str = "NULL";

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct TableInfo {
    pub name: String,
    /// "table" or "view" — the sidebar groups them, and a view is never
    /// offered for editing.
    pub kind: String,
    pub rows_estimate: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Default)]
pub struct ResultSet {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<String>>,
    pub total: Option<i64>,
}

// ---------------------------------------------------------------------------
// Pure parsers

/// Reverses `mysql --batch` escaping of a single field.
fn mysql_unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Parses `mysql --batch` output (header line, then one tab-separated line per
/// row) into a result set. Only the first result set is read: a multi-statement
/// run separates result sets with a blank line, and the grid shows one.
pub fn parse_mysql_batch(text: &str) -> ResultSet {
    let mut lines = text.split('\n').take_while(|l| !l.is_empty());
    let Some(header) = lines.next() else {
        return ResultSet::default();
    };
    let columns = header
        .split('\t')
        .map(|h| ColumnInfo { name: mysql_unescape(h), kind: String::new() })
        .collect::<Vec<_>>();
    let rows = lines
        .map(|line| line.split('\t').map(mysql_unescape).collect::<Vec<_>>())
        .collect();
    ResultSet { columns, rows, total: None }
}

/// Parses RFC 4180 CSV as `psql --csv` emits it. Returns rows of
/// `(value, was_quoted)` so the caller can tell NULL (bare empty) from an
/// empty string (`""`). Handles embedded newlines, doubled quotes and a CRLF or
/// missing trailing newline.
pub fn parse_csv(text: &str) -> Vec<Vec<(String, bool)>> {
    let mut records = Vec::new();
    let mut record: Vec<(String, bool)> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !quoted => {
                in_quotes = true;
                quoted = true;
            }
            ',' => {
                record.push((std::mem::take(&mut field), quoted));
                quoted = false;
            }
            '\r' => {}
            '\n' => {
                record.push((std::mem::take(&mut field), quoted));
                quoted = false;
                records.push(std::mem::take(&mut record));
            }
            other => field.push(other),
        }
    }
    if !field.is_empty() || quoted || !record.is_empty() {
        record.push((field, quoted));
        records.push(record);
    }
    records
}

/// Turns `psql --csv` output into a result set, mapping bare-empty fields to
/// the NULL marker.
pub fn parse_psql_csv(text: &str) -> ResultSet {
    let mut records = parse_csv(text).into_iter();
    let Some(header) = records.next() else {
        return ResultSet::default();
    };
    let columns = header
        .into_iter()
        .map(|(name, _)| ColumnInfo { name, kind: String::new() })
        .collect();
    let rows = records
        .map(|r| {
            r.into_iter()
                .map(|(v, quoted)| if v.is_empty() && !quoted { NULL_MARKER.to_string() } else { v })
                .collect()
        })
        .collect();
    ResultSet { columns, rows, total: None }
}

// ---------------------------------------------------------------------------
// Read-only guard

/// Strips `-- line` and `/* block */` comments and string literals so keyword
/// scanning cannot be fooled by a `DELETE` inside a quoted value or a comment.
fn strip_comments_and_strings(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '-' && next == Some('-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            out.push(' ');
            continue;
        }
        if c == '\'' || c == '"' || c == '`' {
            let quote = c;
            i += 1;
            while i < chars.len() {
                if chars[i] == quote {
                    if chars.get(i + 1) == Some(&quote) {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i += 1;
            out.push_str("''");
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

// Deliberately not including words that legitimately appear inside a SELECT
// (`REPLACE(...)`, `SET` in `SHOW CHARACTER SET`, `ANALYZE` in `EXPLAIN ANALYZE`).
const WRITE_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "CREATE", "DROP", "ALTER", "TRUNCATE", "GRANT", "REVOKE", "CALL", "RENAME",
    "LOAD", "OUTFILE", "DUMPFILE",
];

/// Whether `sql` is a single read-only statement. Accepts one of
/// SELECT / SHOW / DESCRIBE / DESC / EXPLAIN / WITH / TABLE / VALUES, with an
/// optional trailing semicolon; rejects anything else, a second statement, and
/// a CTE or EXPLAIN that wraps a write (`WITH x AS (...) DELETE ...`,
/// `EXPLAIN ANALYZE UPDATE ...` — the latter actually runs the update).
pub fn is_read_only(sql: &str) -> Result<(), String> {
    let cleaned = strip_comments_and_strings(sql);
    let trimmed = cleaned.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return Err("nothing to run".to_string());
    }
    if trimmed.contains(';') {
        return Err("only a single statement can be run at a time".to_string());
    }
    let words: Vec<String> = trimmed
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_uppercase())
        .collect();
    let first = words.first().map(String::as_str).unwrap_or("");
    if !matches!(first, "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "WITH" | "TABLE" | "VALUES") {
        return Err(format!(
            "'{first}' is not a read-only statement — turn on \"Allow writes\" to run it"
        ));
    }
    if let Some(w) = words.iter().find(|w| WRITE_KEYWORDS.contains(&w.as_str())) {
        return Err(format!("statement contains '{w}' — turn on \"Allow writes\" to run it"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Identifier handling. Table/column names come from the UI (which got them
// from the engine), but they still go into generated SQL, so they are checked
// against a strict charset and then quoted per engine.

/// Dashes are allowed because real databases and tables are named `my-app`
/// far too often to reject, and every use here either quotes the identifier
/// per engine or embeds it in a SQL string literal. A *leading* dash is not:
/// the database name is the one identifier that reaches a client binary's
/// argv, where `-h` would be read as a flag rather than a name.
pub fn valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && s.len() <= 128
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '-')
}

fn check_identifier(s: &str, what: &str) -> Result<(), String> {
    if valid_identifier(s) {
        Ok(())
    } else {
        Err(format!("{what} '{s}' contains characters that cannot be used here"))
    }
}

/// Quotes one identifier per engine. The single place that knows MySQL uses
/// backticks and everyone else uses double quotes — `edit` and `export` build
/// their statements through this rather than repeating the rule.
pub fn quote_ident(kind: DbKind, name: &str) -> String {
    match kind {
        DbKind::Mysql => format!("`{name}`"),
        _ => format!("\"{name}\""),
    }
}

/// Quotes a possibly schema-qualified table name (`schema.table`) per engine.
pub fn quote_table(kind: DbKind, table: &str) -> Result<String, String> {
    let parts: Vec<&str> = table.split('.').collect();
    if parts.len() > 2 {
        return Err(format!("table name '{table}' has too many parts"));
    }
    for p in &parts {
        check_identifier(p, "table name")?;
    }
    Ok(parts.iter().map(|p| quote_ident(kind, p)).collect::<Vec<_>>().join("."))
}

/// Parses `order_by` as `<column>` or `<column> asc|desc`, returning a quoted
/// ORDER BY clause (or an empty string when unset).
fn order_clause(kind: DbKind, order_by: Option<&str>) -> Result<String, String> {
    let Some(raw) = order_by.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(String::new());
    };
    let mut parts = raw.split_whitespace();
    let column = parts.next().unwrap_or("");
    check_identifier(column, "column name")?;
    let dir = match parts.next().map(|d| d.to_ascii_lowercase()).as_deref() {
        None | Some("asc") => "ASC",
        Some("desc") => "DESC",
        Some(other) => return Err(format!("unknown sort direction '{other}'")),
    };
    if parts.next().is_some() {
        return Err("order_by must be '<column> [asc|desc]'".to_string());
    }
    Ok(format!(" ORDER BY {} {dir}", quote_ident(kind, column)))
}

/// The alias the page's own row is given, so relation subqueries can correlate
/// against it. Short and underscore-prefixed to stay clear of real names.
const ROW_ALIAS: &str = "_t";

/// Builds the paginated SELECT for the table view: filters, sort, and one
/// `COUNT(*)` subquery per reverse relation the grid is showing.
fn rows_sql(
    kind: DbKind,
    table: &str,
    filters: &[Filter],
    relations: &[RelationSpec],
    limit: u32,
    offset: u64,
    order_by: Option<&str>,
) -> Result<String, String> {
    let quoted = quote_table(kind, table)?;
    let where_sql = where_clause(kind, filters)?;
    let order = order_clause(kind, order_by)?;
    if relations.is_empty() {
        return Ok(format!("SELECT * FROM {quoted}{where_sql}{order} LIMIT {limit} OFFSET {offset}"));
    }
    // Aliased only when there are relations to correlate, so the common case
    // stays the plain statement it has always been.
    let counts = relation_count_columns(kind, relations, ROW_ALIAS)?;
    let alias = quote_ident(kind, ROW_ALIAS);
    Ok(format!(
        "SELECT {alias}.*{counts} FROM {quoted} {alias}{where_sql}{order} LIMIT {limit} OFFSET {offset}"
    ))
}

/// The matching-row count for the same filters, so the pager reports what the
/// filter actually selected rather than the whole table.
fn count_sql(kind: DbKind, table: &str, filters: &[Filter]) -> Result<String, String> {
    Ok(format!(
        "SELECT COUNT(*) FROM {}{}",
        quote_table(kind, table)?,
        where_clause(kind, filters)?
    ))
}

/// Splits `schema.table` into its two halves for information_schema lookups,
/// defaulting to `public` for Postgres.
fn split_table(table: &str) -> (Option<&str>, &str) {
    match table.split_once('.') {
        Some((s, t)) => (Some(s), t),
        None => (None, table),
    }
}

// ---------------------------------------------------------------------------
// Reaching the server

/// How a client binary gets to the database.
#[derive(Debug, Clone, PartialEq)]
pub enum Transport {
    /// `docker exec` into the server's own container: the client runs beside
    /// the server and dials its loopback, so nothing needs publishing to the
    /// host. Used for DevsPro-managed services and for saved Docker connections.
    Container(String),
    /// A TCP endpoint — a database installed directly on this machine, or a
    /// remote one. The two differ only in what is typed into the host field.
    Tcp { host: String, port: u16 },
}

pub struct Conn {
    kind: DbKind,
    transport: Transport,
    user: String,
    password: String,
}

/// Docker's alias for the machine the daemon runs on. Needed when the client
/// runs in a throwaway container but the server listens on the *host's*
/// loopback — inside a container, `127.0.0.1` is the container itself.
const HOST_GATEWAY: &str = "host.docker.internal";

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0")
}

/// The image a throwaway client container is started from. Pinned rather than
/// `latest` so the same DevsPro build always behaves the same way.
fn client_image(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql => "mysql:8",
        _ => "postgres:16",
    }
}

/// Where a client binary will be found and run. Resolved per call, because
/// `mysql` being installed says nothing about `mysqldump` being installed.
#[derive(Debug, Clone, PartialEq)]
enum Runner {
    /// Inside an existing container, via `docker exec`.
    Exec { container: String },
    /// A binary on this machine's PATH.
    Host { host: String, port: u16 },
    /// A throwaway `docker run --rm` container, for a TCP connection on a
    /// machine that has no client installed — which is the normal state of a
    /// machine whose databases all live in Docker.
    Image { image: String, host: String, port: u16 },
}

/// Whether `program` is on PATH. Uses `which`, as the rest of the app does for
/// nginx/docker-compose/php, rather than a second PATH-walking implementation.
fn on_path(program: &str) -> bool {
    Command::new("which")
        .arg(program)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn runner_for(conn: &Conn, program: &str) -> Runner {
    match &conn.transport {
        Transport::Container(container) => Runner::Exec { container: container.clone() },
        Transport::Tcp { host, port } if on_path(program) => Runner::Host { host: host.clone(), port: *port },
        Transport::Tcp { host, port } => Runner::Image {
            image: client_image(conn.kind).to_string(),
            host: if is_loopback(host) { HOST_GATEWAY.to_string() } else { host.clone() },
            port: *port,
        },
    }
}

/// The password is passed by environment variable, never argv: both clients
/// support it, it keeps the secret out of the host's process list, and it
/// avoids mysql's "using a password on the command line" warning entirely.
fn password_env(kind: DbKind, password: &str) -> (String, String) {
    let var = match kind {
        DbKind::Mysql => "MYSQL_PWD",
        _ => "PGPASSWORD",
    };
    (var.to_string(), password.to_string())
}

/// The client's own connection flags. `dial` is `None` when the client runs
/// inside the server's container and reaches it over loopback.
fn connect_args(kind: DbKind, user: &str, dial: Option<(&str, u16)>) -> Vec<String> {
    match kind {
        DbKind::Mysql => {
            let mut args = vec![format!("-u{user}")];
            if let Some((host, port)) = dial {
                args.push(format!("-h{host}"));
                args.push(format!("-P{port}"));
                // Without this the mysql client silently prefers a unix socket
                // whenever the host is "localhost", ignoring -P entirely.
                args.push("--protocol=TCP".to_string());
            }
            args
        }
        _ => {
            let mut args = vec!["-U".to_string(), user.to_string()];
            if let Some((host, port)) = dial {
                args.push("-h".to_string());
                args.push(host.to_string());
                args.push("-p".to_string());
                args.push(port.to_string());
            }
            args
        }
    }
}

/// A fully resolved client call: what to execute, with which arguments and
/// environment. Pure, so the argv of every transport is asserted in tests
/// rather than discovered by running Docker.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// `stdin` marks a call that is fed from a file, which `docker` needs told
/// about with `-i` — without it the container gets a closed stdin and a
/// restore silently applies nothing.
fn invocation(conn: &Conn, runner: &Runner, client: &str, tail: &[String], stdin: bool) -> Invocation {
    let (var, value) = password_env(conn.kind, &conn.password);
    match runner {
        Runner::Exec { container } => {
            let mut args = vec!["exec".to_string()];
            if stdin {
                args.push("-i".to_string());
            }
            args.push("-e".to_string());
            args.push(format!("{var}={value}"));
            args.push(container.clone());
            args.push(client.to_string());
            args.extend(connect_args(conn.kind, &conn.user, None));
            args.extend(tail.iter().cloned());
            Invocation { program: "docker".to_string(), args, env: Vec::new() }
        }
        Runner::Host { host, port } => {
            let mut args = connect_args(conn.kind, &conn.user, Some((host, *port)));
            args.extend(tail.iter().cloned());
            Invocation { program: client.to_string(), args, env: vec![(var, value)] }
        }
        Runner::Image { image, host, port } => {
            let mut args = vec!["run".to_string(), "--rm".to_string()];
            if stdin {
                args.push("-i".to_string());
            }
            // Makes HOST_GATEWAY resolve on Linux too, where Docker Desktop's
            // automatic entry doesn't exist.
            args.push(format!("--add-host={HOST_GATEWAY}:host-gateway"));
            args.push("-e".to_string());
            args.push(format!("{var}={value}"));
            args.push(image.clone());
            args.push(client.to_string());
            args.extend(connect_args(conn.kind, &conn.user, Some((host, *port))));
            args.extend(tail.iter().cloned());
            Invocation { program: "docker".to_string(), args, env: Vec::new() }
        }
    }
}

fn command_for(inv: &Invocation) -> Command {
    let mut cmd = Command::new(&inv.program);
    cmd.args(&inv.args);
    for (key, value) in &inv.env {
        cmd.env(key, value);
    }
    cmd
}

fn spawn_failed(inv: &Invocation, e: std::io::Error) -> String {
    format!("failed to run {}: {e}", inv.program)
}

/// Streams the client's stdout straight into a file rather than buffering it,
/// since a dump can be far too large to hold in memory.
fn run_client_to_file(conn: &Conn, client: &str, tail: &[String], file_path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(file_path)
        .map_err(|e| format!("could not create {}: {e}", file_path.display()))?;
    let inv = invocation(conn, &runner_for(conn, client), client, tail, false);
    let child = command_for(&inv)
        .stdout(Stdio::from(file))
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spawn_failed(&inv, e))?;
    let output = child.wait_with_output().map_err(|e| format!("failed to wait for {}: {e}", inv.program))?;
    if let Some(err) = command_failed(&output) {
        let _ = std::fs::remove_file(file_path);
        return Err(tidy_client_error(&err));
    }
    Ok(())
}

fn run_client_from_file(conn: &Conn, client: &str, tail: &[String], file_path: &Path) -> Result<(), String> {
    let file =
        std::fs::File::open(file_path).map_err(|e| format!("could not open {}: {e}", file_path.display()))?;
    let inv = invocation(conn, &runner_for(conn, client), client, tail, true);
    let child = command_for(&inv)
        .stdin(Stdio::from(file))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spawn_failed(&inv, e))?;
    let output = child.wait_with_output().map_err(|e| format!("failed to wait for {}: {e}", inv.program))?;
    match command_failed(&output) {
        Some(err) => Err(tidy_client_error(&err)),
        None => Ok(()),
    }
}

/// Turns a saved connection row into something dialable. The engine check is
/// here as well as in `connections::validate` because a row could predate a
/// change to the allowed list.
fn conn_from_saved(saved: &DbConnection) -> Result<Conn, String> {
    let kind = match saved.kind.as_str() {
        "mysql" => DbKind::Mysql,
        "postgres" => DbKind::Postgres,
        other => return Err(format!("'{other}' connections cannot be browsed")),
    };
    let transport = match saved.transport.as_str() {
        "docker" => Transport::Container(saved.container.clone()),
        "tcp" => Transport::Tcp { host: saved.host.clone(), port: saved.port },
        other => return Err(format!("unknown connection type '{other}'")),
    };
    Ok(Conn { kind, transport, user: saved.username.clone(), password: saved.password.clone() })
}

/// Prefix marking a `connection` argument as a saved custom connection rather
/// than a service in DevsPro's own compose project. Kept as one opaque string
/// so every browse command keeps its existing signature.
pub const CUSTOM_PREFIX: &str = "custom:";

fn connect(app: &tauri::AppHandle, db: &crate::db::Db, connection: &str) -> Result<Conn, String> {
    let conn = match connection.strip_prefix(CUSTOM_PREFIX) {
        Some(id) => {
            let saved = {
                let guard = db.0.lock().map_err(|e| e.to_string())?;
                super::connections::find(&guard, id)?
            };
            conn_from_saved(&saved)?
        }
        None => {
            let (kind, user, password) = admin_context(app, connection)?;
            Conn { kind, transport: Transport::Container(container_name(connection)), user, password }
        }
    };
    if !matches!(conn.kind, DbKind::Mysql | DbKind::Postgres) {
        return Err("browsing is only available for MySQL and PostgreSQL databases".to_string());
    }
    Ok(conn)
}

// ---------------------------------------------------------------------------
// Running SQL through the client

/// The client program for a role, per engine.
fn client_for(kind: DbKind, role: &str) -> &'static str {
    match (kind, role) {
        (DbKind::Mysql, "dump") => "mysqldump",
        (DbKind::Mysql, _) => "mysql",
        (_, "dump") => "pg_dump",
        (_, _) => "psql",
    }
}

/// Runs one SQL statement and returns the parsed result set (empty when the
/// statement produced no rows, e.g. a write with `allow_write`).
fn run_sql(conn: &Conn, database: &str, sql: &str) -> Result<ResultSet, String> {
    run_sql_in(conn, database, sql, None)
}

/// The `PGOPTIONS` that make one psql process resolve bare names in `schema`
/// first. Built only from a validated identifier, so nothing in it can break
/// out of the quotes. `None` for MySQL, where the database is the schema.
fn search_path_options(kind: DbKind, schema: Option<&str>) -> Result<Option<String>, String> {
    match (kind, schema) {
        (DbKind::Mysql, _) | (_, None) => Ok(None),
        (_, Some(schema)) => {
            check_identifier(schema, "schema name")?;
            Ok(Some(format!("-c search_path=\"{schema}\",public")))
        }
    }
}

/// Adds an environment variable to an already-built invocation: on the host it
/// goes in `env`, through docker it is an `-e` before the container/image.
fn with_env(mut inv: Invocation, key: &str, value: &str) -> Invocation {
    if inv.program == "docker" {
        inv.args.splice(1..1, ["-e".to_string(), format!("{key}={value}")]);
    } else {
        inv.env.push((key.to_string(), value.to_string()));
    }
    inv
}

fn run_sql_in(conn: &Conn, database: &str, sql: &str, schema: Option<&str>) -> Result<ResultSet, String> {
    check_identifier(database, "database name")?;
    let options = search_path_options(conn.kind, schema)?;
    let tail: Vec<String> = match conn.kind {
        // `--database=` rather than a trailing positional: unambiguous, and it
        // keeps a database whose name starts oddly from being read as a flag.
        DbKind::Mysql => vec!["--batch".to_string(), "-e".to_string(), sql.to_string(), format!("--database={database}")],
        _ => vec![
            "-d".to_string(),
            database.to_string(),
            "--csv".to_string(),
            "-q".to_string(),
            "-v".to_string(),
            "ON_ERROR_STOP=1".to_string(),
            "-c".to_string(),
            sql.to_string(),
        ],
    };
    let client = client_for(conn.kind, "sql");
    let mut inv = invocation(conn, &runner_for(conn, client), client, &tail, false);
    if let Some(options) = options {
        inv = with_env(inv, "PGOPTIONS", &options);
    }
    let out = command_for(&inv).output().map_err(|e| spawn_failed(&inv, e))?;
    if let Some(err) = command_failed(&out) {
        return Err(tidy_client_error(&err));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(match conn.kind {
        DbKind::Mysql => parse_mysql_batch(&text),
        _ => parse_psql_csv(&text),
    })
}

/// Drops the password-on-command-line warning mysql prints on every call so
/// the actual error is the first thing the user reads. Still needed even now
/// that the password goes via the environment: an older client warns about
/// `MYSQL_PWD` too, and a `docker run` line can also prepend image-pull noise.
fn tidy_client_error(err: &str) -> String {
    err.lines()
        .filter(|l| !l.contains("Using a password on the command line"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn single_column(rs: ResultSet) -> Vec<String> {
    rs.rows.into_iter().filter_map(|r| r.into_iter().next()).collect()
}

fn parse_i64(s: &str) -> i64 {
    s.trim().parse::<f64>().map(|v| v.max(0.0) as i64).unwrap_or(0)
}

fn column_types_sql(kind: DbKind, database: &str, table: &str) -> String {
    let (schema, name) = split_table(table);
    match kind {
        DbKind::Mysql => format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{database}' AND TABLE_NAME='{name}' ORDER BY ORDINAL_POSITION"
        ),
        _ => format!(
            "SELECT column_name, data_type FROM information_schema.columns WHERE table_schema='{}' AND table_name='{name}' ORDER BY ordinal_position",
            schema.unwrap_or("public")
        ),
    }
}

fn list_databases_sql(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql => "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME NOT IN ('information_schema','performance_schema','mysql','sys') ORDER BY SCHEMA_NAME",
        _ => "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
    }
}

/// The database a connection is made to before the user has picked one — every
/// engine has one that is always present.
fn bootstrap_database(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql => "information_schema",
        _ => "postgres",
    }
}

// ---------------------------------------------------------------------------
// Commands

#[tauri::command(async)]
pub fn databases_list_databases(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
) -> Result<Vec<String>, String> {
    let conn = connect(&app, &db, &connection)?;
    Ok(single_column(run_sql(&conn, bootstrap_database(conn.kind), list_databases_sql(conn.kind))?))
}

/// Dials the server and reports what answered, so a connection can be checked
/// from the form before anything else is attempted against it.
#[tauri::command(async)]
pub fn databases_test_connection(
    db: State<crate::db::Db>,
    input: super::connections::ConnectionInput,
) -> Result<String, String> {
    let saved = {
        let guard = db.0.lock().map_err(|e| e.to_string())?;
        super::connections::draft(&guard, &input)?
    };
    let conn = conn_from_saved(&saved)?;
    let sql = match conn.kind {
        DbKind::Mysql => "SELECT VERSION()",
        _ => "SHOW server_version",
    };
    let rs = run_sql(&conn, bootstrap_database(conn.kind), sql)?;
    let version = single_column(rs).first().cloned().unwrap_or_default();
    Ok(format!("connected — server version {}", if version.is_empty() { "unknown" } else { &version }))
}

/// The sidebar's contents: every table and view in one database.
#[tauri::command(async)]
pub fn databases_list_tables(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    schema: Option<String>,
) -> Result<Vec<TableInfo>, String> {
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    // MySQL's database is its schema; only PostgreSQL narrows further.
    let only = match (conn.kind, schema.as_deref()) {
        (DbKind::Mysql, _) | (_, None) => None,
        (_, Some(s)) => {
            check_identifier(s, "schema name")?;
            Some(s)
        }
    };
    let rs = run_sql(&conn, &database, &schema::objects_sql(conn.kind, &database, only))?;
    Ok(rs
        .rows
        .into_iter()
        .filter(|r| r.len() >= 4)
        .map(|r| TableInfo {
            name: r[0].clone(),
            kind: r[1].clone(),
            rows_estimate: parse_i64(&r[2]),
            size_bytes: parse_i64(&r[3]),
        })
        .collect())
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct SchemaInfo {
    pub name: String,
    pub tables: i64,
    pub size_bytes: i64,
}

/// The schema picker's contents. Empty for MySQL, whose databases are schemas.
#[tauri::command(async)]
pub fn databases_list_schemas(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
) -> Result<Vec<SchemaInfo>, String> {
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    let Some(sql) = schema::schemas_sql(conn.kind) else {
        return Ok(Vec::new());
    };
    let rs = run_sql(&conn, &database, sql)?;
    Ok(rs
        .rows
        .into_iter()
        .filter(|r| r.len() >= 3)
        .map(|r| SchemaInfo { name: r[0].clone(), tables: parse_i64(&r[1]), size_bytes: parse_i64(&r[2]) })
        .collect())
}

/// Columns, indexes and foreign keys for one table — the Structure tab, and
/// the source of the primary key the grid edits through.
#[tauri::command(async)]
pub fn databases_table_structure(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    table: String,
) -> Result<schema::TableStructure, String> {
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    quote_table(conn.kind, &table)?;
    let columns = run_sql(&conn, &database, &schema::columns_sql(conn.kind, &database, &table))?;
    let indexes = run_sql(&conn, &database, &schema::indexes_sql(conn.kind, &database, &table))?;
    let foreign_keys = run_sql(&conn, &database, &schema::foreign_keys_sql(conn.kind, &database, &table))?;
    let referenced_by = run_sql(&conn, &database, &schema::referenced_by_sql(conn.kind, &database, &table))?;
    Ok(schema::TableStructure {
        columns: columns.rows.iter().filter_map(|r| schema::column_from_row(r)).collect(),
        indexes: indexes.rows.iter().filter_map(|r| schema::index_from_row(r)).collect(),
        foreign_keys: foreign_keys.rows.iter().filter_map(|r| schema::foreign_key_from_row(r)).collect(),
        referenced_by: referenced_by
            .rows
            .iter()
            .filter_map(|r| schema::reverse_relation_from_row(r))
            .collect(),
    })
}

#[tauri::command(async)]
pub fn databases_table_rows(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    table: String,
    limit: u32,
    offset: u64,
    order_by: Option<String>,
    filters: Option<Vec<Filter>>,
    relations: Option<Vec<RelationSpec>>,
) -> Result<ResultSet, String> {
    let conn = connect(&app, &db, &connection)?;
    let filters = filters.unwrap_or_default();
    let relations = relations.unwrap_or_default();
    let limit = limit.clamp(1, 1000);
    let sql = rows_sql(conn.kind, &table, &filters, &relations, limit, offset, order_by.as_deref())?;
    let mut rs = run_sql(&conn, &database, &sql)?;

    // Types come from information_schema rather than the client output, so a
    // column of the grid can be labelled without parsing `--column-type-info`.
    let types = run_sql(&conn, &database, &column_types_sql(conn.kind, &database, &table))?;
    for col in &mut rs.columns {
        if let Some(t) = types.rows.iter().find(|r| r.first() == Some(&col.name)) {
            col.kind = t.get(1).cloned().unwrap_or_default();
        }
    }

    // Counted with the same filters, so the pager describes the filtered set.
    let count = run_sql(&conn, &database, &count_sql(conn.kind, &table, &filters)?)?;
    rs.total = single_column(count).first().map(|s| parse_i64(s));
    Ok(rs)
}

/// Refuses a write when the connection is in read-only mode.
///
/// Enforced here rather than by hiding buttons: the mode is a safety catch
/// against editing the wrong database, and a safety catch that only exists in
/// the UI is not one.
fn require_writable(db: &crate::db::Db, connection: &str) -> Result<(), String> {
    let writable = {
        let guard = db.0.lock().map_err(|e| e.to_string())?;
        super::connections::is_writable(&guard, connection)
    };
    if writable {
        Ok(())
    } else {
        Err("this connection is in read-only mode — switch it to \"Read & write\" to make changes".to_string())
    }
}

/// Applies the grid's staged edits in one transaction.
///
/// The key columns are read from the catalogue here, not taken from the
/// request: the frontend says which row it read and what changed, and this
/// decides what identifies that row.
#[tauri::command(async)]
pub fn databases_apply_edits(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    table: String,
    edits: Vec<PendingEdit>,
) -> Result<String, String> {
    require_writable(&db, &connection)?;
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    let quoted = quote_table(conn.kind, &table)?;

    let columns = run_sql(&conn, &database, &schema::columns_sql(conn.kind, &database, &table))?;
    let key_columns: Vec<String> = columns
        .rows
        .iter()
        .filter_map(|r| schema::column_from_row(r))
        .filter(|c| c.primary_key)
        .map(|c| c.name)
        .collect();

    let resolved = edits.iter().map(|e| resolve(e, &key_columns)).collect::<Result<Vec<_>, _>>()?;
    let sql = edit_batch_sql(conn.kind, &quoted, &resolved)?;
    run_sql(&conn, &database, &sql)?;
    Ok(match edits.len() {
        1 => "saved 1 change".to_string(),
        n => format!("saved {n} changes"),
    })
}

/// A column type as the Add column form submits it.
///
/// A type cannot be parameterised — it is syntax, not a value — so it is
/// checked against a strict charset instead. That admits everything a type
/// actually needs (`VARCHAR(255)`, `NUMERIC(10, 2)`, `TIMESTAMP WITH TIME
/// ZONE`, `int[]`) and nothing that could close the statement.
fn check_data_type(data_type: &str) -> Result<String, String> {
    let trimmed = data_type.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err("a column needs a type".to_string());
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '(' | ')' | ',' | '[' | ']');
    if !trimmed.chars().all(allowed) {
        return Err(format!("'{trimmed}' is not a usable column type"));
    }
    Ok(trimmed.to_string())
}

/// Adds a column to an existing table.
///
/// The one piece of DDL the browser offers, because "I need somewhere to put
/// this" is the schema change people actually make while looking at data.
/// Anything further belongs in a migration, not in a grid.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn databases_add_column(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    table: String,
    name: String,
    data_type: String,
    nullable: Option<bool>,
    default_value: Option<String>,
) -> Result<String, String> {
    require_writable(&db, &connection)?;
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    check_identifier(&name, "column name")?;
    let quoted_table = quote_table(conn.kind, &table)?;
    let data_type = check_data_type(&data_type)?;

    let mut sql = format!(
        "ALTER TABLE {quoted_table} ADD COLUMN {} {data_type}",
        quote_ident(conn.kind, &name)
    );
    if let Some(default) = default_value.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        sql.push_str(&format!(" DEFAULT {}", quote_literal(conn.kind, default)));
    }
    // A NOT NULL column with no default cannot be added to a table that
    // already has rows; the engine says so, and that error is the right one to
    // pass through rather than second-guessing here.
    if !nullable.unwrap_or(true) {
        sql.push_str(" NOT NULL");
    }
    run_sql(&conn, &database, &sql)?;
    Ok(format!("added column '{name}'"))
}

/// Writes rows to a file as CSV, JSON or INSERT statements.
///
/// Two callers, two shapes. The console exports the statement it just ran, so
/// it passes `sql`. The grid exports *what it is showing* — the same filters
/// and sort, without the page limit — so it passes the table instead and the
/// SELECT is rebuilt here, which keeps identifier quoting in the one place
/// that knows the engine.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn databases_export(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    sql: Option<String>,
    table: Option<String>,
    filters: Option<Vec<Filter>>,
    order_by: Option<String>,
    format: String,
    path: String,
) -> Result<String, String> {
    let format = export::parse_format(&format)?;
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    let table = table.filter(|t| !t.is_empty());

    let (statement, quoted) = match sql.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(sql) => {
            // An export only ever reads, so it is allowed in read-only mode —
            // but it goes through the console's own guard so a write cannot be
            // smuggled in as one.
            is_read_only(&sql)?;
            let quoted = match &table {
                Some(t) => quote_table(conn.kind, t)?,
                None => quote_ident(conn.kind, "exported_rows"),
            };
            (sql, quoted)
        }
        None => {
            let table = table.ok_or_else(|| "nothing to export".to_string())?;
            let quoted = quote_table(conn.kind, &table)?;
            let where_sql = where_clause(conn.kind, &filters.unwrap_or_default())?;
            let order = order_clause(conn.kind, order_by.as_deref())?;
            (format!("SELECT * FROM {quoted}{where_sql}{order}"), quoted)
        }
    };

    let rs = run_sql(&conn, &database, &statement)?;
    let body = export::render(format, conn.kind, &quoted, &rs);
    std::fs::write(&path, body).map_err(|e| format!("could not write {path}: {e}"))?;
    Ok(format!("exported {} rows to {path}", rs.rows.len()))
}

#[tauri::command(async)]
pub fn databases_query(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    sql: String,
    allow_write: Option<bool>,
    schema: Option<String>,
) -> Result<ResultSet, String> {
    if allow_write.unwrap_or(false) {
        if sql.trim().is_empty() {
            return Err("nothing to run".to_string());
        }
        require_writable(&db, &connection)?;
    } else {
        is_read_only(&sql)?;
    }
    let conn = connect(&app, &db, &connection)?;

    let started = std::time::Instant::now();
    let result = run_sql_in(&conn, &database, sql.trim(), schema.as_deref());
    let elapsed = started.elapsed().as_millis() as i64;

    // Recorded whether it worked or not — "what did I run just before it
    // broke" is most of what a query log is for. A failure to write the log
    // never fails the query itself.
    if let Ok(guard) = db.0.lock() {
        let rows = result.as_ref().map(|rs| rs.rows.len() as i64).unwrap_or(0);
        let _ = super::history::record(
            &guard,
            &connection,
            &database,
            sql.trim(),
            result.is_ok(),
            rows,
            elapsed,
        );
    }
    result
}

/// Dumps one database with `mysqldump` / `pg_dump` to a host path the user
/// picked. Unlike `databases_backup` (which names the file itself under the app
/// data dir), this is the "Save as…" flavour — and unlike it, this one works
/// for saved custom connections too.
#[tauri::command(async)]
pub fn databases_dump(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    path: String,
) -> Result<String, String> {
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    let file = PathBuf::from(&path);
    if file.as_os_str().is_empty() {
        return Err("no destination file was chosen".to_string());
    }
    // A bare positional, not `--databases`: that flag would wrap the dump in
    // `CREATE DATABASE`/`USE`, pinning it to this name and making a restore
    // into a differently-named database silently go elsewhere.
    let tail = match conn.kind {
        DbKind::Mysql => vec![database.clone()],
        _ => vec!["-d".to_string(), database.clone()],
    };
    run_client_to_file(&conn, client_for(conn.kind, "dump"), &tail, &file)?;
    Ok(format!("dumped '{database}' to {path}"))
}

/// Restores a SQL dump into the selected database. The Manage tab's
/// `databases_restore` only knows about compose services; this is its
/// equivalent for anything the browser can open.
#[tauri::command(async)]
pub fn databases_restore_dump(
    app: tauri::AppHandle,
    db: State<crate::db::Db>,
    connection: String,
    database: String,
    path: String,
) -> Result<String, String> {
    require_writable(&db, &connection)?;
    let conn = connect(&app, &db, &connection)?;
    check_identifier(&database, "database name")?;
    let file = PathBuf::from(&path);
    if !file.is_file() {
        return Err(format!("dump file not found: {path}"));
    }
    let tail = match conn.kind {
        DbKind::Mysql => vec![format!("--database={database}")],
        _ => vec!["-d".to_string(), database.clone(), "-v".to_string(), "ON_ERROR_STOP=1".to_string()],
    };
    run_client_from_file(&conn, client_for(conn.kind, "sql"), &tail, &file)?;
    Ok(format!("restored '{database}' from {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_batch_parses_header_and_rows() {
        let rs = parse_mysql_batch("id\tname\n1\talice\n2\tbob\n");
        assert_eq!(rs.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["id", "name"]);
        assert_eq!(rs.rows, vec![vec!["1", "alice"], vec!["2", "bob"]]);
    }

    #[test]
    fn mysql_batch_unescapes_tabs_newlines_and_backslashes_inside_values() {
        let rs = parse_mysql_batch("v\na\\tb\\nc\\\\d\\0e\n");
        assert_eq!(rs.rows, vec![vec!["a\tb\nc\\d\0e"]]);
    }

    #[test]
    fn mysql_batch_keeps_null_marker_and_stops_at_first_result_set() {
        let rs = parse_mysql_batch("a\tb\nNULL\tx\n\nc\n1\n");
        assert_eq!(rs.rows, vec![vec!["NULL", "x"]]);
    }

    #[test]
    fn mysql_batch_empty_output_means_no_columns() {
        assert_eq!(parse_mysql_batch(""), ResultSet::default());
    }

    #[test]
    fn csv_handles_quotes_embedded_newlines_and_crlf() {
        let rows = parse_csv("a,b\r\n\"x,\"\"y\"\"\nz\",2\r\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][0], ("x,\"y\"\nz".to_string(), true));
        assert_eq!(rows[1][1], ("2".to_string(), false));
    }

    #[test]
    fn csv_without_trailing_newline_keeps_last_record() {
        let rows = parse_csv("a\n1");
        assert_eq!(rows, vec![vec![("a".to_string(), false)], vec![("1".to_string(), false)]]);
    }

    #[test]
    fn psql_csv_distinguishes_null_from_empty_string() {
        let rs = parse_psql_csv("id,note\n1,\n2,\"\"\n");
        assert_eq!(rs.rows, vec![vec!["1", "NULL"], vec!["2", ""]]);
        assert_eq!(rs.columns[1].name, "note");
    }

    #[test]
    fn read_only_accepts_the_usual_read_statements() {
        for sql in [
            "SELECT 1",
            "  select * from t;  ",
            "SHOW TABLES",
            "DESCRIBE users",
            "EXPLAIN SELECT * FROM t",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "-- comment\nSELECT 'DELETE FROM t'",
            "SELECT * FROM t WHERE name = 'a;b'",
        ] {
            assert!(is_read_only(sql).is_ok(), "{sql}");
        }
    }

    #[test]
    fn read_only_rejects_writes_multiple_statements_and_disguised_writes() {
        for sql in [
            "DELETE FROM t",
            "UPDATE t SET a=1",
            "SELECT 1; DROP TABLE t",
            "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x",
            "EXPLAIN ANALYZE UPDATE t SET a=1",
            "SELECT * INTO OUTFILE '/tmp/x' FROM t",
            "",
            "/* SELECT */ TRUNCATE t",
        ] {
            assert!(is_read_only(sql).is_err(), "{sql}");
        }
    }

    #[test]
    fn rows_sql_quotes_per_engine_and_validates_order_by() {
        assert_eq!(
            rows_sql(DbKind::Mysql, "users", &[], &[], 50, 100, Some("id desc")).unwrap(),
            "SELECT * FROM `users` ORDER BY `id` DESC LIMIT 50 OFFSET 100"
        );
        assert_eq!(
            rows_sql(DbKind::Postgres, "app.users", &[], &[], 50, 0, Some("name")).unwrap(),
            "SELECT * FROM \"app\".\"users\" ORDER BY \"name\" ASC LIMIT 50 OFFSET 0"
        );
        assert_eq!(
            rows_sql(DbKind::Postgres, "users", &[], &[], 10, 0, None).unwrap(),
            "SELECT * FROM \"users\" LIMIT 10 OFFSET 0"
        );
        assert!(rows_sql(DbKind::Mysql, "users; DROP", &[], &[], 50, 0, None).is_err());
        assert!(rows_sql(DbKind::Mysql, "users", &[], &[], 50, 0, Some("id; DROP")).is_err());
        assert!(rows_sql(DbKind::Mysql, "users", &[], &[], 50, 0, Some("id sideways")).is_err());
    }

    #[test]
    fn relations_alias_the_row_so_the_count_subqueries_can_correlate() {
        let relations = [RelationSpec {
            table: "tasks".to_string(),
            columns: vec!["project_id".to_string()],
            references_columns: vec!["id".to_string()],
        }];
        let sql = rows_sql(DbKind::Mysql, "projects", &[], &relations, 50, 0, None).unwrap();
        assert!(sql.starts_with("SELECT `_t`.*, (SELECT COUNT(*) FROM `tasks` c"), "{sql}");
        assert!(sql.contains("FROM `projects` `_t` LIMIT 50 OFFSET 0"), "{sql}");
    }

    #[test]
    fn no_relations_means_the_plain_statement_it_has_always_been() {
        assert_eq!(
            rows_sql(DbKind::Postgres, "users", &[], &[], 10, 0, None).unwrap(),
            "SELECT * FROM \"users\" LIMIT 10 OFFSET 0"
        );
    }

    #[test]
    fn a_column_type_admits_real_types_and_refuses_anything_that_could_close_the_statement() {
        for good in ["varchar(255)", "NUMERIC(10, 2)", "TIMESTAMP WITH TIME ZONE", "int[]"] {
            assert!(check_data_type(good).is_ok(), "{good}");
        }
        for bad in ["", "text; DROP TABLE users", "text'", "text`", "text--"] {
            assert!(check_data_type(bad).is_err(), "{bad}");
        }
        assert_eq!(check_data_type("  text  ").unwrap(), "text");
    }

    #[test]
    fn filters_land_between_the_table_and_the_order_by() {
        let filters = [Filter {
            column: "status".to_string(),
            op: super::super::edit::FilterOp::Eq,
            value: "active".to_string(),
        }];
        assert_eq!(
            rows_sql(DbKind::Mysql, "users", &filters, &[], 50, 0, Some("id desc")).unwrap(),
            "SELECT * FROM `users` WHERE `status` = 'active' ORDER BY `id` DESC LIMIT 50 OFFSET 0"
        );
        // The count has to carry the same filters or the pager lies.
        assert_eq!(
            count_sql(DbKind::Mysql, "users", &filters).unwrap(),
            "SELECT COUNT(*) FROM `users` WHERE `status` = 'active'"
        );
    }

    #[test]
    fn parse_i64_clamps_negative_estimates_and_tolerates_floats() {
        assert_eq!(parse_i64("-1"), 0);
        assert_eq!(parse_i64("12.0"), 12);
        assert_eq!(parse_i64("junk"), 0);
    }

    fn tcp_conn(kind: DbKind) -> Conn {
        Conn {
            kind,
            transport: Transport::Tcp { host: "db.example.com".to_string(), port: 3307 },
            user: "app".to_string(),
            password: "pw".to_string(),
        }
    }

    #[test]
    fn exec_runs_the_client_inside_the_container_with_no_host_flags() {
        let conn = Conn {
            kind: DbKind::Mysql,
            transport: Transport::Container("devspro_shop".to_string()),
            user: "root".to_string(),
            password: "pw".to_string(),
        };
        let runner = Runner::Exec { container: "devspro_shop".to_string() };
        let inv = invocation(&conn, &runner, "mysql", &["--batch".to_string()], false);
        assert_eq!(inv.program, "docker");
        assert_eq!(
            inv.args,
            vec!["exec", "-e", "MYSQL_PWD=pw", "devspro_shop", "mysql", "-uroot", "--batch"]
        );
        assert!(inv.args.iter().all(|a| a != "-h"), "a container client dials its own loopback");
    }

    #[test]
    fn a_host_client_gets_the_password_from_the_environment_not_argv() {
        let conn = tcp_conn(DbKind::Postgres);
        let runner = Runner::Host { host: "db.example.com".to_string(), port: 3307 };
        let inv = invocation(&conn, &runner, "psql", &["--csv".to_string()], false);
        assert_eq!(inv.program, "psql");
        assert_eq!(inv.args, vec!["-U", "app", "-h", "db.example.com", "-p", "3307", "--csv"]);
        assert_eq!(inv.env, vec![("PGPASSWORD".to_string(), "pw".to_string())]);
        assert!(!inv.args.iter().any(|a| a.contains("pw")));
    }

    #[test]
    fn mysql_over_tcp_forces_the_tcp_protocol() {
        // Otherwise the client quietly uses a unix socket for "localhost" and
        // ignores the port entirely.
        let args = connect_args(DbKind::Mysql, "root", Some(("localhost", 3306)));
        assert!(args.contains(&"--protocol=TCP".to_string()), "{args:?}");
    }

    #[test]
    fn a_throwaway_client_container_rewrites_loopback_to_the_host_gateway() {
        let conn = Conn {
            kind: DbKind::Mysql,
            transport: Transport::Tcp { host: "127.0.0.1".to_string(), port: 3306 },
            user: "root".to_string(),
            password: "pw".to_string(),
        };
        let runner = runner_for_tcp_without_a_host_client(&conn);
        let inv = invocation(&conn, &runner, "mysql", &[], false);
        assert_eq!(inv.program, "docker");
        assert!(inv.args.contains(&format!("--add-host={HOST_GATEWAY}:host-gateway")));
        assert!(inv.args.contains(&format!("-h{HOST_GATEWAY}")), "{:?}", inv.args);
        assert!(!inv.args.iter().any(|a| a.contains("127.0.0.1")));
    }

    /// `runner_for` consults PATH, which a test cannot control; this is the
    /// branch it takes when nothing is installed.
    fn runner_for_tcp_without_a_host_client(conn: &Conn) -> Runner {
        let Transport::Tcp { host, port } = &conn.transport else { unreachable!() };
        Runner::Image {
            image: client_image(conn.kind).to_string(),
            host: if is_loopback(host) { HOST_GATEWAY.to_string() } else { host.clone() },
            port: *port,
        }
    }

    #[test]
    fn a_remote_host_is_dialled_as_given_from_a_throwaway_container() {
        let conn = tcp_conn(DbKind::Mysql);
        let runner = runner_for_tcp_without_a_host_client(&conn);
        let inv = invocation(&conn, &runner, "mysql", &[], false);
        assert!(inv.args.contains(&"-hdb.example.com".to_string()), "{:?}", inv.args);
    }

    #[test]
    fn a_restore_asks_docker_for_stdin() {
        let conn = tcp_conn(DbKind::Mysql);
        let exec = Runner::Exec { container: "c".to_string() };
        assert!(invocation(&conn, &exec, "mysql", &[], true).args.contains(&"-i".to_string()));
        // Without -i the container's stdin is closed and the dump applies nothing.
        assert!(!invocation(&conn, &exec, "mysql", &[], false).args.contains(&"-i".to_string()));
    }

    #[test]
    fn saved_rows_map_onto_a_transport_and_unknown_engines_are_refused() {
        let base = DbConnection {
            id: "1".to_string(),
            name: "Prod".to_string(),
            kind: "postgres".to_string(),
            transport: "tcp".to_string(),
            host: "10.0.0.5".to_string(),
            port: 5432,
            container: String::new(),
            username: "postgres".to_string(),
            password: "pw".to_string(),
            default_database: String::new(),
            created_at: String::new(),
        };
        let conn = conn_from_saved(&base).unwrap();
        assert_eq!(conn.transport, Transport::Tcp { host: "10.0.0.5".to_string(), port: 5432 });

        let in_docker =
            DbConnection { transport: "docker".to_string(), container: "app_db_1".to_string(), ..base.clone() };
        assert_eq!(
            conn_from_saved(&in_docker).unwrap().transport,
            Transport::Container("app_db_1".to_string())
        );

        assert!(conn_from_saved(&DbConnection { kind: "mongo".to_string(), ..base }).is_err());
    }

    #[test]
    fn identifiers_allow_a_dash_but_never_lead_with_one() {
        assert!(valid_identifier("my-app_db"));
        assert!(!valid_identifier("-h"));
        assert!(!valid_identifier("a b"));
        assert!(rows_sql(DbKind::Mysql, "line-items", &[], &[], 10, 0, None).is_ok());
    }

    #[test]
    fn client_error_drops_the_mysql_password_warning() {
        let err = "mysql: [Warning] Using a password on the command line interface can be insecure.\nERROR 1146 (42S02) at line 1: Table 'x' doesn't exist";
        assert_eq!(tidy_client_error(err), "ERROR 1146 (42S02) at line 1: Table 'x' doesn't exist");
    }

    #[test]
    fn the_search_path_option_is_only_built_from_a_valid_identifier() {
        assert_eq!(
            search_path_options(DbKind::Postgres, Some("audit")).unwrap().as_deref(),
            Some("-c search_path=\"audit\",public")
        );
        assert!(search_path_options(DbKind::Postgres, Some("a\",public;--")).is_err());
        assert!(search_path_options(DbKind::Postgres, Some("")).is_err());
        assert_eq!(search_path_options(DbKind::Postgres, None).unwrap(), None);
        assert_eq!(search_path_options(DbKind::Mysql, Some("bad name")).unwrap(), None);
    }

    #[test]
    fn an_extra_environment_variable_reaches_the_client_on_every_transport() {
        let host = Invocation { program: "psql".into(), args: vec!["-U".into()], env: Vec::new() };
        assert_eq!(with_env(host, "K", "v").env, vec![("K".to_string(), "v".to_string())]);
        let docker = Invocation { program: "docker".into(), args: vec!["exec".into(), "c".into()], env: Vec::new() };
        assert_eq!(with_env(docker, "K", "v").args, vec!["exec", "-e", "K=v", "c"]);
    }
}
