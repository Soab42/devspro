//! Database browser: list databases/tables, page through rows, run ad-hoc SQL,
//! and dump/restore to a user-chosen file — for the MySQL and PostgreSQL
//! containers the Databases module manages.
//!
//! Same approach as user management in the parent module: `docker exec` into
//! the official image's bundled client (`mysql` / `psql`) rather than adding a
//! native driver crate per engine. The cost of that choice is that everything
//! comes back as text, so the client is asked for its most machine-readable
//! format and the parsing lives in small pure functions with tests:
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
use std::path::PathBuf;

use super::{admin_context, command_failed, container_name, docker_exec, docker_exec_to_file, DbKind};

/// Marker used for SQL NULL in the row grid. The frontend shows it muted.
pub const NULL_MARKER: &str = "NULL";

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct TableInfo {
    pub name: String,
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

fn valid_identifier(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn check_identifier(s: &str, what: &str) -> Result<(), String> {
    if valid_identifier(s) {
        Ok(())
    } else {
        Err(format!("{what} '{s}' contains characters that cannot be used here"))
    }
}

/// Quotes a possibly schema-qualified table name (`schema.table`) per engine.
fn quote_table(kind: DbKind, table: &str) -> Result<String, String> {
    let parts: Vec<&str> = table.split('.').collect();
    if parts.len() > 2 {
        return Err(format!("table name '{table}' has too many parts"));
    }
    for p in &parts {
        check_identifier(p, "table name")?;
    }
    let quote = |s: &str| match kind {
        DbKind::Mysql => format!("`{s}`"),
        _ => format!("\"{s}\""),
    };
    Ok(parts.iter().map(|p| quote(p)).collect::<Vec<_>>().join("."))
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
    let quoted = match kind {
        DbKind::Mysql => format!("`{column}`"),
        _ => format!("\"{column}\""),
    };
    Ok(format!(" ORDER BY {quoted} {dir}"))
}

/// Builds the paginated SELECT for the table view.
fn rows_sql(kind: DbKind, table: &str, limit: u32, offset: u64, order_by: Option<&str>) -> Result<String, String> {
    let table = quote_table(kind, table)?;
    let order = order_clause(kind, order_by)?;
    Ok(format!("SELECT * FROM {table}{order} LIMIT {limit} OFFSET {offset}"))
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
// Running SQL through the client

struct Conn {
    kind: DbKind,
    container: String,
    user: String,
    password: String,
}

fn connect(app: &tauri::AppHandle, connection: &str) -> Result<Conn, String> {
    let (kind, user, password) = admin_context(app, connection)?;
    if !matches!(kind, DbKind::Mysql | DbKind::Postgres) {
        return Err("browsing is only available for MySQL and PostgreSQL databases".to_string());
    }
    Ok(Conn { kind, container: container_name(connection), user, password })
}

/// Runs one SQL statement and returns the parsed result set (empty when the
/// statement produced no rows, e.g. a write with `allow_write`).
fn run_sql(conn: &Conn, database: &str, sql: &str) -> Result<ResultSet, String> {
    check_identifier(database, "database name")?;
    let out = match conn.kind {
        DbKind::Mysql => docker_exec(
            &conn.container,
            &["mysql", &format!("-u{}", conn.user), &format!("-p{}", conn.password), "--batch", "-e", sql, database],
        )?,
        _ => docker_exec(
            &conn.container,
            &["psql", "-U", &conn.user, "-d", database, "--csv", "-q", "-v", "ON_ERROR_STOP=1", "-c", sql],
        )?,
    };
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
/// the actual error is the first thing the user reads.
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

fn list_tables_sql(kind: DbKind, database: &str) -> String {
    match kind {
        DbKind::Mysql => format!(
            "SELECT TABLE_NAME, IFNULL(TABLE_ROWS,0), IFNULL(DATA_LENGTH,0)+IFNULL(INDEX_LENGTH,0) FROM information_schema.TABLES WHERE TABLE_SCHEMA='{database}' AND TABLE_TYPE='BASE TABLE' ORDER BY TABLE_NAME"
        ),
        // reltuples is -1 on a never-analyzed table; clamped to 0 by parse_i64.
        // Tables outside `public` are shown schema-qualified so the name round-trips.
        _ => "SELECT CASE WHEN n.nspname='public' THEN c.relname ELSE n.nspname||'.'||c.relname END, c.reltuples::bigint, pg_total_relation_size(c.oid) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind IN ('r','p') AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' ORDER BY 1".to_string(),
    }
}

fn list_databases_sql(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Mysql => "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME NOT IN ('information_schema','performance_schema','mysql','sys') ORDER BY SCHEMA_NAME",
        _ => "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
    }
}

// ---------------------------------------------------------------------------
// Commands

#[tauri::command]
pub fn databases_list_databases(app: tauri::AppHandle, connection: String) -> Result<Vec<String>, String> {
    let conn = connect(&app, &connection)?;
    let default_db = match conn.kind {
        DbKind::Mysql => "information_schema",
        _ => "postgres",
    };
    Ok(single_column(run_sql(&conn, default_db, list_databases_sql(conn.kind))?))
}

#[tauri::command]
pub fn databases_list_tables(
    app: tauri::AppHandle,
    connection: String,
    database: String,
) -> Result<Vec<TableInfo>, String> {
    let conn = connect(&app, &connection)?;
    check_identifier(&database, "database name")?;
    let rs = run_sql(&conn, &database, &list_tables_sql(conn.kind, &database))?;
    Ok(rs
        .rows
        .into_iter()
        .filter(|r| r.len() >= 3)
        .map(|r| TableInfo { name: r[0].clone(), rows_estimate: parse_i64(&r[1]), size_bytes: parse_i64(&r[2]) })
        .collect())
}

#[tauri::command]
pub fn databases_table_rows(
    app: tauri::AppHandle,
    connection: String,
    database: String,
    table: String,
    limit: u32,
    offset: u64,
    order_by: Option<String>,
) -> Result<ResultSet, String> {
    let conn = connect(&app, &connection)?;
    let limit = limit.clamp(1, 1000);
    let sql = rows_sql(conn.kind, &table, limit, offset, order_by.as_deref())?;
    let mut rs = run_sql(&conn, &database, &sql)?;

    // Types come from information_schema rather than the client output, so a
    // column of the grid can be labelled without parsing `--column-type-info`.
    let types = run_sql(&conn, &database, &column_types_sql(conn.kind, &database, &table))?;
    for col in &mut rs.columns {
        if let Some(t) = types.rows.iter().find(|r| r.first() == Some(&col.name)) {
            col.kind = t.get(1).cloned().unwrap_or_default();
        }
    }

    let count = run_sql(&conn, &database, &format!("SELECT COUNT(*) FROM {}", quote_table(conn.kind, &table)?))?;
    rs.total = single_column(count).first().map(|s| parse_i64(s));
    Ok(rs)
}

#[tauri::command]
pub fn databases_query(
    app: tauri::AppHandle,
    connection: String,
    database: String,
    sql: String,
    allow_write: Option<bool>,
) -> Result<ResultSet, String> {
    if !allow_write.unwrap_or(false) {
        is_read_only(&sql)?;
    } else if sql.trim().is_empty() {
        return Err("nothing to run".to_string());
    }
    let conn = connect(&app, &connection)?;
    run_sql(&conn, &database, sql.trim())
}

/// Dumps one database with `mysqldump` / `pg_dump` to a host path the user
/// picked. Unlike `databases_backup` (which names the file itself under the app
/// data dir), this is the "Save as…" flavour.
#[tauri::command]
pub fn databases_dump(
    app: tauri::AppHandle,
    connection: String,
    database: String,
    path: String,
) -> Result<String, String> {
    let conn = connect(&app, &connection)?;
    check_identifier(&database, "database name")?;
    let file = PathBuf::from(&path);
    if file.as_os_str().is_empty() {
        return Err("no destination file was chosen".to_string());
    }
    let args = match conn.kind {
        DbKind::Mysql => super::mysql_dump_args(&conn.password, &database),
        _ => super::postgres_dump_args(&conn.user, &database),
    };
    docker_exec_to_file(&conn.container, &super::as_str_args(&args), &file)?;
    Ok(format!("dumped '{database}' to {path}"))
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
            rows_sql(DbKind::Mysql, "users", 50, 100, Some("id desc")).unwrap(),
            "SELECT * FROM `users` ORDER BY `id` DESC LIMIT 50 OFFSET 100"
        );
        assert_eq!(
            rows_sql(DbKind::Postgres, "app.users", 50, 0, Some("name")).unwrap(),
            "SELECT * FROM \"app\".\"users\" ORDER BY \"name\" ASC LIMIT 50 OFFSET 0"
        );
        assert_eq!(rows_sql(DbKind::Postgres, "users", 10, 0, None).unwrap(), "SELECT * FROM \"users\" LIMIT 10 OFFSET 0");
        assert!(rows_sql(DbKind::Mysql, "users; DROP", 50, 0, None).is_err());
        assert!(rows_sql(DbKind::Mysql, "users", 50, 0, Some("id; DROP")).is_err());
        assert!(rows_sql(DbKind::Mysql, "users", 50, 0, Some("id sideways")).is_err());
    }

    #[test]
    fn parse_i64_clamps_negative_estimates_and_tolerates_floats() {
        assert_eq!(parse_i64("-1"), 0);
        assert_eq!(parse_i64("12.0"), 12);
        assert_eq!(parse_i64("junk"), 0);
    }

    #[test]
    fn client_error_drops_the_mysql_password_warning() {
        let err = "mysql: [Warning] Using a password on the command line interface can be insecure.\nERROR 1146 (42S02) at line 1: Table 'x' doesn't exist";
        assert_eq!(tidy_client_error(err), "ERROR 1146 (42S02) at line 1: Table 'x' doesn't exist");
    }
}
