//! Generating the SQL behind the grid's filters and its inline editing.
//!
//! Every value here comes from the user — a filter they typed, a cell they
//! changed — and the browser drives a CLI client, so there are no bind
//! parameters to hide behind: each value is embedded in the statement text.
//! That makes literal quoting the whole security surface of this module, and
//! why it is a pure function with more tests than anything else in the file.
//!
//! Identifiers take the other path: they are checked against
//! `browse::valid_identifier` and then quoted per engine, so a column name can
//! never contribute syntax.

use serde::Deserialize;

use super::browse::{quote_ident, valid_identifier};
use super::DbKind;

/// Escapes a value for use inside a single-quoted SQL literal.
///
/// Postgres needs only the doubled quote, because `standard_conforming_strings`
/// has been on by default since 9.1 and a backslash is then an ordinary
/// character. MySQL treats a backslash as an escape character in the default
/// mode, so one has to be doubled too — otherwise a value ending in `\` would
/// escape the closing quote and everything after it would become syntax.
///
/// (Under MySQL's non-default `NO_BACKSLASH_ESCAPES` the doubling would show
/// through as a literal pair. That is a visible, harmless wrong value rather
/// than an injection, which is the right way round for the two to fail.)
pub fn quote_literal(kind: DbKind, value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' if matches!(kind, DbKind::Mysql) => out.push_str("\\\\"),
            // A literal NUL cannot survive the trip through the client's argv
            // either way, so it is dropped rather than silently truncating.
            '\0' => {}
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// A value as the grid sends it: `None` is SQL NULL, which is not the same as
/// the empty string and must not be quoted.
pub type Value = Option<String>;

fn render_value(kind: DbKind, value: &Value) -> String {
    match value {
        None => "NULL".to_string(),
        Some(v) => quote_literal(kind, v),
    }
}

// ---------------------------------------------------------------------------
// Filters

/// The comparisons the filter bar offers. A fixed list, because the operator
/// lands in the statement as syntax — it is never taken from the input text.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Ne,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Gt,
    Gte,
    Lt,
    Lte,
    IsNull,
    NotNull,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: String,
}

/// Escapes the characters LIKE treats as wildcards, so a filter for a literal
/// `100%` doesn't match everything starting with `100`. Both engines default
/// to backslash as the LIKE escape character.
fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn condition(kind: DbKind, filter: &Filter) -> Result<String, String> {
    if !valid_identifier(&filter.column) {
        return Err(format!("column name '{}' cannot be used in a filter", filter.column));
    }
    let col = quote_ident(kind, &filter.column);
    let like = |pattern: String| format!("{col} LIKE {}", quote_literal(kind, &pattern));
    let value = &filter.value;

    Ok(match filter.op {
        FilterOp::IsNull => format!("{col} IS NULL"),
        FilterOp::NotNull => format!("{col} IS NOT NULL"),
        FilterOp::Eq => format!("{col} = {}", quote_literal(kind, value)),
        FilterOp::Ne => format!("{col} <> {}", quote_literal(kind, value)),
        FilterOp::Gt => format!("{col} > {}", quote_literal(kind, value)),
        FilterOp::Gte => format!("{col} >= {}", quote_literal(kind, value)),
        FilterOp::Lt => format!("{col} < {}", quote_literal(kind, value)),
        FilterOp::Lte => format!("{col} <= {}", quote_literal(kind, value)),
        FilterOp::Contains => like(format!("%{}%", escape_like(value))),
        FilterOp::NotContains => format!("{col} NOT LIKE {}", quote_literal(kind, &format!("%{}%", escape_like(value)))),
        FilterOp::StartsWith => like(format!("{}%", escape_like(value))),
        FilterOp::EndsWith => like(format!("%{}", escape_like(value))),
    })
}

/// Builds the ` WHERE ...` clause for a set of filters, ANDed together, or an
/// empty string when there are none.
pub fn where_clause(kind: DbKind, filters: &[Filter]) -> Result<String, String> {
    if filters.is_empty() {
        return Ok(String::new());
    }
    let parts = filters.iter().map(|f| condition(kind, f)).collect::<Result<Vec<_>, _>>()?;
    Ok(format!(" WHERE {}", parts.join(" AND ")))
}

// ---------------------------------------------------------------------------
// Related-row counts
//
// The grid shows, per row, how many child rows each reverse relation has —
// "Task 20" beside a project. Done as a correlated subquery per relation in
// the page's own SELECT rather than a second round trip per row, so a page
// costs one query however many relations a table has.

/// The alias prefix for those counts. The frontend splits the result set on
/// it, so both sides must agree; a column genuinely named this way would be
/// misread, which is why it is a shape no migration tool generates.
pub const RELATION_PREFIX: &str = "__rel_";

/// One reverse relation to count, as the frontend sends it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RelationSpec {
    /// The child table holding the foreign key.
    pub table: String,
    /// Its columns.
    pub columns: Vec<String>,
    /// The columns of the table being browsed that they point at.
    pub references_columns: Vec<String>,
}

/// `, (SELECT COUNT(*) FROM child c WHERE c.fk = t.pk) AS __rel_0` for each
/// relation, or an empty string when there are none.
pub fn relation_count_columns(
    kind: DbKind,
    relations: &[RelationSpec],
    row_alias: &str,
) -> Result<String, String> {
    let mut out = String::new();
    for (index, relation) in relations.iter().enumerate() {
        if relation.columns.is_empty() || relation.columns.len() != relation.references_columns.len() {
            return Err(format!("relation to '{}' has mismatched columns", relation.table));
        }
        let child = quote_table_path(kind, &relation.table)?;
        let mut predicates = Vec::with_capacity(relation.columns.len());
        for (child_column, parent_column) in relation.columns.iter().zip(&relation.references_columns) {
            if !valid_identifier(child_column) || !valid_identifier(parent_column) {
                return Err(format!("relation to '{}' names a column that cannot be used", relation.table));
            }
            predicates.push(format!(
                "c.{} = {row_alias}.{}",
                quote_ident(kind, child_column),
                quote_ident(kind, parent_column)
            ));
        }
        out.push_str(&format!(
            ", (SELECT COUNT(*) FROM {child} c WHERE {}) AS {}",
            predicates.join(" AND "),
            quote_ident(kind, &format!("{RELATION_PREFIX}{index}"))
        ));
    }
    Ok(out)
}

/// Quotes a possibly schema-qualified name. A thin wrapper so this module does
/// not need to reach back into `browse` for the two-part case.
fn quote_table_path(kind: DbKind, table: &str) -> Result<String, String> {
    let parts: Vec<&str> = table.split('.').collect();
    if parts.len() > 2 || !parts.iter().all(|p| valid_identifier(p)) {
        return Err(format!("table name '{table}' cannot be used here"));
    }
    Ok(parts.iter().map(|p| quote_ident(kind, p)).collect::<Vec<_>>().join("."))
}

// ---------------------------------------------------------------------------
// Row edits

/// One staged change from the grid.
///
/// `key` identifies the row by its primary key *as it was read*, so an edit
/// still lands on the right row after the key column itself is changed. An
/// empty key is refused for updates and deletes — a WHERE-less UPDATE is the
/// single worst thing this module could emit.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowEdit {
    Update {
        key: Vec<(String, Value)>,
        set: Vec<(String, Value)>,
    },
    Insert {
        values: Vec<(String, Value)>,
    },
    Delete {
        key: Vec<(String, Value)>,
    },
}

fn key_predicate(kind: DbKind, key: &[(String, Value)]) -> Result<String, String> {
    if key.is_empty() {
        return Err("this table has no primary key, so rows cannot be changed from the grid".to_string());
    }
    let parts = key
        .iter()
        .map(|(column, value)| {
            if !valid_identifier(column) {
                return Err(format!("column name '{column}' cannot be used here"));
            }
            let col = quote_ident(kind, column);
            Ok(match value {
                // A NULL key column cannot be matched with `=`; it also means
                // the row was never uniquely identified, but IS NULL is at
                // least correct for the composite case.
                None => format!("{col} IS NULL"),
                Some(v) => format!("{col} = {}", quote_literal(kind, v)),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(parts.join(" AND "))
}

fn assignments(kind: DbKind, set: &[(String, Value)]) -> Result<Vec<String>, String> {
    set.iter()
        .map(|(column, value)| {
            if !valid_identifier(column) {
                return Err(format!("column name '{column}' cannot be used here"));
            }
            Ok(format!("{} = {}", quote_ident(kind, column), render_value(kind, value)))
        })
        .collect()
}

/// Turns one staged edit into a statement against `table` (already quoted by
/// the caller, which is the only place that knows how to split `schema.table`).
pub fn edit_sql(kind: DbKind, quoted_table: &str, edit: &RowEdit) -> Result<String, String> {
    match edit {
        RowEdit::Update { key, set } => {
            if set.is_empty() {
                return Err("nothing was changed in this row".to_string());
            }
            Ok(format!(
                "UPDATE {quoted_table} SET {} WHERE {}",
                assignments(kind, set)?.join(", "),
                key_predicate(kind, key)?
            ))
        }
        RowEdit::Delete { key } => {
            Ok(format!("DELETE FROM {quoted_table} WHERE {}", key_predicate(kind, key)?))
        }
        RowEdit::Insert { values } => {
            if values.is_empty() {
                return Err("a new row needs at least one value".to_string());
            }
            let mut columns = Vec::with_capacity(values.len());
            let mut literals = Vec::with_capacity(values.len());
            for (column, value) in values {
                if !valid_identifier(column) {
                    return Err(format!("column name '{column}' cannot be used here"));
                }
                columns.push(quote_ident(kind, column));
                literals.push(render_value(kind, value));
            }
            Ok(format!(
                "INSERT INTO {quoted_table} ({}) VALUES ({})",
                columns.join(", "),
                literals.join(", ")
            ))
        }
    }
}

/// A staged change as the grid sends it: the row as it was *read*, plus what
/// changed. The grid does not say which columns identify the row — the backend
/// works that out from the catalogue, so a wrong or missing key can never come
/// from the frontend and turn an edit into a table-wide UPDATE.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingEdit {
    Update {
        row: Vec<(String, Value)>,
        set: Vec<(String, Value)>,
    },
    Insert {
        values: Vec<(String, Value)>,
    },
    Delete {
        row: Vec<(String, Value)>,
    },
}

/// Narrows a read row down to its key columns, in the order the catalogue
/// reports them.
fn key_from_row(row: &[(String, Value)], key_columns: &[String]) -> Result<Vec<(String, Value)>, String> {
    if key_columns.is_empty() {
        return Err("this table has no primary key, so rows cannot be changed from the grid".to_string());
    }
    key_columns
        .iter()
        .map(|column| {
            row.iter()
                .find(|(name, _)| name == column)
                .cloned()
                .ok_or_else(|| format!("the key column '{column}' is not in the grid, so this row cannot be changed"))
        })
        .collect()
}

/// Resolves a staged change against the table's real primary key.
pub fn resolve(edit: &PendingEdit, key_columns: &[String]) -> Result<RowEdit, String> {
    Ok(match edit {
        PendingEdit::Update { row, set } => {
            RowEdit::Update { key: key_from_row(row, key_columns)?, set: set.clone() }
        }
        PendingEdit::Delete { row } => RowEdit::Delete { key: key_from_row(row, key_columns)? },
        PendingEdit::Insert { values } => RowEdit::Insert { values: values.clone() },
    })
}

/// Wraps the staged edits in a transaction so a batch either lands whole or
/// not at all — the grid shows one "Save", and a half-applied save would leave
/// the user with no idea which rows made it.
pub fn edit_batch_sql(kind: DbKind, quoted_table: &str, edits: &[RowEdit]) -> Result<String, String> {
    if edits.is_empty() {
        return Err("there are no changes to save".to_string());
    }
    let mut statements = vec![match kind {
        DbKind::Mysql => "START TRANSACTION".to_string(),
        _ => "BEGIN".to_string(),
    }];
    for edit in edits {
        statements.push(edit_sql(kind, quoted_table, edit)?);
    }
    statements.push("COMMIT".to_string());
    Ok(format!("{};", statements.join(";\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(column: &str, op: FilterOp, value: &str) -> Filter {
        Filter { column: column.to_string(), op, value: value.to_string() }
    }

    #[test]
    fn a_quote_inside_a_value_is_doubled_not_left_to_end_the_literal() {
        assert_eq!(quote_literal(DbKind::Mysql, "O'Brien"), "'O''Brien'");
        assert_eq!(quote_literal(DbKind::Postgres, "O'Brien"), "'O''Brien'");
    }

    /// The injection that quote-doubling alone does not stop: under MySQL's
    /// default mode a trailing backslash escapes the closing quote, so
    /// `x\' OR 1=1 -- ` would break out of the literal.
    #[test]
    fn mysql_doubles_backslashes_so_a_trailing_one_cannot_escape_the_quote() {
        assert_eq!(quote_literal(DbKind::Mysql, "x\\"), "'x\\\\'");
        let hostile = quote_literal(DbKind::Mysql, "x\\' OR 1=1 -- ");
        assert_eq!(hostile, "'x\\\\'' OR 1=1 -- '");
        // Postgres reads a backslash literally, so it is left alone there.
        assert_eq!(quote_literal(DbKind::Postgres, "x\\"), "'x\\'");
    }

    #[test]
    fn a_null_is_the_keyword_and_an_empty_string_is_a_literal() {
        assert_eq!(render_value(DbKind::Mysql, &None), "NULL");
        assert_eq!(render_value(DbKind::Mysql, &Some(String::new())), "''");
    }

    #[test]
    fn filters_quote_the_column_per_engine_and_and_them_together() {
        let filters = [f("name", FilterOp::Eq, "alice"), f("age", FilterOp::Gte, "30")];
        assert_eq!(
            where_clause(DbKind::Mysql, &filters).unwrap(),
            " WHERE `name` = 'alice' AND `age` >= '30'"
        );
        assert_eq!(
            where_clause(DbKind::Postgres, &filters).unwrap(),
            " WHERE \"name\" = 'alice' AND \"age\" >= '30'"
        );
        assert_eq!(where_clause(DbKind::Mysql, &[]).unwrap(), "");
    }

    #[test]
    fn null_filters_take_no_value() {
        assert_eq!(
            where_clause(DbKind::Mysql, &[f("deleted_at", FilterOp::IsNull, "ignored")]).unwrap(),
            " WHERE `deleted_at` IS NULL"
        );
        assert_eq!(
            where_clause(DbKind::Mysql, &[f("deleted_at", FilterOp::NotNull, "")]).unwrap(),
            " WHERE `deleted_at` IS NOT NULL"
        );
    }

    #[test]
    fn a_contains_filter_escapes_wildcards_so_a_literal_percent_is_searchable() {
        // Without the escape, "100%" would match anything starting with 100.
        assert_eq!(
            where_clause(DbKind::Postgres, &[f("label", FilterOp::Contains, "100%")]).unwrap(),
            " WHERE \"label\" LIKE '%100\\%%'"
        );
        assert_eq!(
            where_clause(DbKind::Postgres, &[f("label", FilterOp::StartsWith, "a_b")]).unwrap(),
            " WHERE \"label\" LIKE 'a\\_b%'"
        );
    }

    #[test]
    fn a_filter_on_an_unusable_column_name_is_refused_rather_than_quoted() {
        let err = where_clause(DbKind::Mysql, &[f("id`; DROP TABLE users; --", FilterOp::Eq, "1")]).unwrap_err();
        assert!(err.contains("cannot be used in a filter"), "{err}");
    }

    fn relation(table: &str, child: &str, parent: &str) -> RelationSpec {
        RelationSpec {
            table: table.to_string(),
            columns: vec![child.to_string()],
            references_columns: vec![parent.to_string()],
        }
    }

    #[test]
    fn relation_counts_are_aliased_in_order_so_the_grid_can_match_them_up() {
        let relations = [relation("tasks", "project_id", "id"), relation("sprints", "project_id", "id")];
        assert_eq!(
            relation_count_columns(DbKind::Mysql, &relations, "t").unwrap(),
            concat!(
                ", (SELECT COUNT(*) FROM `tasks` c WHERE c.`project_id` = t.`id`) AS `__rel_0`",
                ", (SELECT COUNT(*) FROM `sprints` c WHERE c.`project_id` = t.`id`) AS `__rel_1`"
            )
        );
        assert_eq!(relation_count_columns(DbKind::Mysql, &[], "t").unwrap(), "");
    }

    #[test]
    fn a_composite_relation_matches_on_every_column_pair() {
        let composite = RelationSpec {
            table: "line_items".to_string(),
            columns: vec!["order_id".to_string(), "line".to_string()],
            references_columns: vec!["id".to_string(), "line_no".to_string()],
        };
        let sql = relation_count_columns(DbKind::Postgres, &[composite], "t").unwrap();
        assert!(sql.contains("c.\"order_id\" = t.\"id\" AND c.\"line\" = t.\"line_no\""), "{sql}");
    }

    #[test]
    fn a_relation_naming_something_unusable_is_refused_rather_than_quoted() {
        let hostile = RelationSpec {
            table: "tasks".to_string(),
            columns: vec!["id`; DROP TABLE users; --".to_string()],
            references_columns: vec!["id".to_string()],
        };
        assert!(relation_count_columns(DbKind::Mysql, &[hostile], "t").is_err());
        // A mismatched pair would silently produce a cross join count.
        let lopsided = RelationSpec {
            table: "tasks".to_string(),
            columns: vec!["a".to_string(), "b".to_string()],
            references_columns: vec!["id".to_string()],
        };
        assert!(relation_count_columns(DbKind::Mysql, &[lopsided], "t").is_err());
    }

    #[test]
    fn an_update_targets_the_row_by_the_key_it_was_read_with() {
        let edit = RowEdit::Update {
            key: vec![("id".to_string(), Some("7".to_string()))],
            set: vec![("name".to_string(), Some("bob".to_string())), ("note".to_string(), None)],
        };
        assert_eq!(
            edit_sql(DbKind::Mysql, "`users`", &edit).unwrap(),
            "UPDATE `users` SET `name` = 'bob', `note` = NULL WHERE `id` = '7'"
        );
    }

    /// The one statement this module must never be able to emit.
    #[test]
    fn an_update_or_delete_without_a_key_is_refused() {
        let update = RowEdit::Update { key: vec![], set: vec![("a".to_string(), None)] };
        let delete = RowEdit::Delete { key: vec![] };
        for edit in [update, delete] {
            let err = edit_sql(DbKind::Mysql, "`t`", &edit).unwrap_err();
            assert!(err.contains("no primary key"), "{err}");
        }
    }

    #[test]
    fn a_composite_key_matches_on_every_part() {
        let edit = RowEdit::Delete {
            key: vec![
                ("order_id".to_string(), Some("1".to_string())),
                ("line".to_string(), Some("2".to_string())),
            ],
        };
        assert_eq!(
            edit_sql(DbKind::Postgres, "\"line_items\"", &edit).unwrap(),
            "DELETE FROM \"line_items\" WHERE \"order_id\" = '1' AND \"line\" = '2'"
        );
    }

    #[test]
    fn an_insert_lists_only_the_columns_that_were_filled_in() {
        let edit = RowEdit::Insert {
            values: vec![("name".to_string(), Some("carol".to_string())), ("note".to_string(), None)],
        };
        assert_eq!(
            edit_sql(DbKind::Mysql, "`users`", &edit).unwrap(),
            "INSERT INTO `users` (`name`, `note`) VALUES ('carol', NULL)"
        );
        assert!(edit_sql(DbKind::Mysql, "`users`", &RowEdit::Insert { values: vec![] }).is_err());
    }

    #[test]
    fn an_update_that_changes_nothing_is_refused_rather_than_emitting_empty_set() {
        let edit = RowEdit::Update { key: vec![("id".to_string(), Some("1".to_string()))], set: vec![] };
        assert!(edit_sql(DbKind::Mysql, "`t`", &edit).is_err());
    }

    #[test]
    fn the_key_comes_from_the_catalogue_not_from_the_grid() {
        let row = vec![
            ("id".to_string(), Some("7".to_string())),
            ("name".to_string(), Some("bob".to_string())),
        ];
        let edit = PendingEdit::Update { row, set: vec![("name".to_string(), Some("bobby".to_string()))] };
        let resolved = resolve(&edit, &["id".to_string()]).unwrap();
        let RowEdit::Update { key, .. } = &resolved else { panic!("expected an update") };
        // Only the primary key, even though the grid sent every column.
        assert_eq!(key, &vec![("id".to_string(), Some("7".to_string()))]);
    }

    #[test]
    fn a_table_with_no_primary_key_cannot_be_edited_from_the_grid() {
        let edit = PendingEdit::Delete { row: vec![("a".to_string(), Some("1".to_string()))] };
        let err = resolve(&edit, &[]).unwrap_err();
        assert!(err.contains("no primary key"), "{err}");
        // An insert needs no key, so it is still allowed.
        assert!(resolve(&PendingEdit::Insert { values: vec![("a".to_string(), None)] }, &[]).is_ok());
    }

    #[test]
    fn an_edit_is_refused_when_the_key_column_was_not_among_the_columns_read() {
        // e.g. the grid is showing the result of `SELECT name FROM users`.
        let edit = PendingEdit::Delete { row: vec![("name".to_string(), Some("bob".to_string()))] };
        let err = resolve(&edit, &["id".to_string()]).unwrap_err();
        assert!(err.contains("not in the grid"), "{err}");
    }

    #[test]
    fn a_composite_key_is_taken_in_catalogue_order_not_grid_order() {
        let row = vec![
            ("line".to_string(), Some("2".to_string())),
            ("order_id".to_string(), Some("1".to_string())),
        ];
        let resolved =
            resolve(&PendingEdit::Delete { row }, &["order_id".to_string(), "line".to_string()]).unwrap();
        assert_eq!(
            edit_sql(DbKind::Mysql, "`t`", &resolved).unwrap(),
            "DELETE FROM `t` WHERE `order_id` = '1' AND `line` = '2'"
        );
    }

    #[test]
    fn a_batch_is_wrapped_in_a_transaction_per_engine() {
        let edits = vec![RowEdit::Delete { key: vec![("id".to_string(), Some("1".to_string()))] }];
        let mysql = edit_batch_sql(DbKind::Mysql, "`t`", &edits).unwrap();
        assert!(mysql.starts_with("START TRANSACTION;"), "{mysql}");
        assert!(mysql.ends_with("COMMIT;"), "{mysql}");

        let pg = edit_batch_sql(DbKind::Postgres, "\"t\"", &edits).unwrap();
        assert!(pg.starts_with("BEGIN;"), "{pg}");

        assert!(edit_batch_sql(DbKind::Mysql, "`t`", &[]).is_err());
    }

    #[test]
    fn a_hostile_value_stays_inside_its_literal_all_the_way_through_a_batch() {
        let edits = vec![RowEdit::Update {
            key: vec![("id".to_string(), Some("1' OR '1'='1".to_string()))],
            set: vec![("name".to_string(), Some("'; DROP TABLE users; --".to_string()))],
        }];
        let sql = edit_batch_sql(DbKind::Postgres, "\"users\"", &edits).unwrap();
        assert!(sql.contains("\"name\" = '''; DROP TABLE users; --'"), "{sql}");
        assert!(sql.contains("\"id\" = '1'' OR ''1''=''1'"), "{sql}");
        // The value's own semicolons stay inside their literal, so the batch is
        // still the three statements it is meant to be. Counting every semicolon
        // would not show that — two of them come from the value itself.
        assert_eq!(sql.split(";\n").count(), 3, "{sql}");
    }
}
