//! Writing a result set out as CSV, JSON or INSERT statements.
//!
//! This exports *what the grid is showing* — a query's result, filters and
//! all. It is deliberately not a substitute for `databases_dump`: a result set
//! has already been through the client's text output, so a value that reads
//! back as the NULL marker is indistinguishable from the string "NULL". For a
//! faithful copy of a whole database, mysqldump/pg_dump is still the tool, and
//! the Dump… button is still the way to reach it.

use super::browse::{ResultSet, NULL_MARKER};
use super::edit::quote_literal;
use super::DbKind;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Csv,
    Json,
    Sql,
}

pub fn parse_format(name: &str) -> Result<Format, String> {
    match name {
        "csv" => Ok(Format::Csv),
        "json" => Ok(Format::Json),
        "sql" => Ok(Format::Sql),
        other => Err(format!("unknown export format '{other}'")),
    }
}

/// RFC 4180: a field is quoted when it contains a comma, a quote or a newline,
/// and an embedded quote is doubled.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn to_csv(rs: &ResultSet) -> String {
    let mut out = String::new();
    out.push_str(&rs.columns.iter().map(|c| csv_field(&c.name)).collect::<Vec<_>>().join(","));
    out.push('\n');
    for row in &rs.rows {
        // A NULL becomes a bare empty field, which is how psql --csv writes it
        // and how this app's own CSV parser reads it back.
        let cells = row
            .iter()
            .map(|v| if v == NULL_MARKER { String::new() } else { csv_field(v) })
            .collect::<Vec<_>>();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

pub fn to_json(rs: &ResultSet) -> String {
    let rows: Vec<serde_json::Value> = rs
        .rows
        .iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for (index, column) in rs.columns.iter().enumerate() {
                let cell = row.get(index).map(String::as_str).unwrap_or("");
                let value = if cell == NULL_MARKER {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(cell.to_string())
                };
                // A query can repeat a column name (`SELECT a.id, b.id`); the
                // later one wins, as it would in any JSON object.
                object.insert(column.name.clone(), value);
            }
            serde_json::Value::Object(object)
        })
        .collect();
    serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string())
}

/// One INSERT per row, for pasting into another database.
pub fn to_sql(kind: DbKind, quoted_table: &str, rs: &ResultSet) -> String {
    if rs.columns.is_empty() {
        return String::new();
    }
    let columns = rs
        .columns
        .iter()
        .map(|c| super::browse::quote_ident(kind, &c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = String::new();
    for row in &rs.rows {
        let values = row
            .iter()
            .map(|v| if v == NULL_MARKER { "NULL".to_string() } else { quote_literal(kind, v) })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("INSERT INTO {quoted_table} ({columns}) VALUES ({values});\n"));
    }
    out
}

pub fn render(format: Format, kind: DbKind, quoted_table: &str, rs: &ResultSet) -> String {
    match format {
        Format::Csv => to_csv(rs),
        Format::Json => to_json(rs),
        Format::Sql => to_sql(kind, quoted_table, rs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::databases::browse::ColumnInfo;

    fn sample() -> ResultSet {
        ResultSet {
            columns: vec![
                ColumnInfo { name: "id".to_string(), kind: String::new() },
                ColumnInfo { name: "note".to_string(), kind: String::new() },
            ],
            rows: vec![
                vec!["1".to_string(), "hello, \"world\"".to_string()],
                vec!["2".to_string(), NULL_MARKER.to_string()],
            ],
            total: None,
        }
    }

    #[test]
    fn csv_quotes_only_the_fields_that_need_it_and_writes_null_as_empty() {
        assert_eq!(to_csv(&sample()), "id,note\n1,\"hello, \"\"world\"\"\"\n2,\n");
    }

    #[test]
    fn json_distinguishes_null_from_a_string() {
        let json = to_json(&sample());
        assert!(json.contains("\"note\": \"hello, \\\"world\\\"\""), "{json}");
        assert!(json.contains("\"note\": null"), "{json}");
    }

    #[test]
    fn sql_export_quotes_values_and_leaves_null_bare() {
        let sql = to_sql(DbKind::Mysql, "`t`", &sample());
        assert!(sql.contains("INSERT INTO `t` (`id`, `note`) VALUES ('1', 'hello, \"world\"');"), "{sql}");
        assert!(sql.contains("VALUES ('2', NULL);"), "{sql}");
    }

    #[test]
    fn a_result_set_with_no_columns_exports_nothing_rather_than_broken_sql() {
        assert_eq!(to_sql(DbKind::Mysql, "`t`", &ResultSet::default()), "");
    }

    #[test]
    fn only_the_three_known_formats_are_accepted() {
        assert_eq!(parse_format("csv").unwrap(), Format::Csv);
        assert_eq!(parse_format("json").unwrap(), Format::Json);
        assert_eq!(parse_format("sql").unwrap(), Format::Sql);
        assert!(parse_format("xlsx").is_err());
    }
}
