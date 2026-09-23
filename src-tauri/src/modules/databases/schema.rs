//! Reading a table's shape: its columns, indexes and foreign keys.
//!
//! All three come from the engine's own catalogue rather than from parsing a
//! `SHOW CREATE TABLE`, so the two engines produce the *same* rows and the
//! frontend renders one table per section instead of one per engine.
//!
//! The SQL is built here as pure strings and the results are mapped by pure
//! functions, so the queries can be checked without a running database — the
//! part that needs a real server is only `run_sql`, which lives in `browse`.
//!
//! Column order matters: every query below selects its columns in the order
//! the matching `from_row` reads them.

use serde::Serialize;

use super::edit::quote_literal;
use super::DbKind;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColumnMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub nullable: bool,
    pub default: String,
    pub primary_key: bool,
    /// `auto_increment`, an identity generation, or empty.
    pub extra: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IndexMeta {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ForeignKeyMeta {
    pub name: String,
    pub columns: Vec<String>,
    pub references_table: String,
    pub references_columns: Vec<String>,
    pub on_delete: String,
    pub on_update: String,
}

/// A foreign key pointing *at* this table, from somewhere else. The grid shows
/// one column per reverse relation, with the number of child rows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReverseRelation {
    pub name: String,
    /// The table holding the foreign key.
    pub table: String,
    /// Its columns, in the order they match `references_columns`.
    pub columns: Vec<String>,
    /// This table's columns that they point at.
    pub references_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct TableStructure {
    pub columns: Vec<ColumnMeta>,
    pub indexes: Vec<IndexMeta>,
    /// Foreign keys this table declares — where its rows point.
    pub foreign_keys: Vec<ForeignKeyMeta>,
    /// Foreign keys pointing back at it — what its rows own.
    pub referenced_by: Vec<ReverseRelation>,
}

/// Splits `schema.table`, defaulting the schema to the database itself for
/// MySQL (where the two are the same thing) and to `public` for Postgres.
pub fn split_qualified<'a>(kind: DbKind, database: &'a str, table: &'a str) -> (&'a str, &'a str) {
    match table.split_once('.') {
        Some((schema, name)) => (schema, name),
        None => (if matches!(kind, DbKind::Mysql) { database } else { "public" }, table),
    }
}

fn lit(kind: DbKind, value: &str) -> String {
    quote_literal(kind, value)
}

pub fn columns_sql(kind: DbKind, database: &str, table: &str) -> String {
    let (schema, name) = split_qualified(kind, database, table);
    let (s, t) = (lit(kind, schema), lit(kind, name));
    match kind {
        DbKind::Mysql => format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, IFNULL(COLUMN_DEFAULT,''), COLUMN_KEY, EXTRA \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA={s} AND TABLE_NAME={t} ORDER BY ORDINAL_POSITION"
        ),
        // The PK flag is spelled 'PRI' so both engines feed one `from_row`.
        _ => format!(
            "SELECT c.column_name, \
             CASE WHEN c.character_maximum_length IS NOT NULL THEN c.data_type||'('||c.character_maximum_length||')' \
             ELSE c.data_type END, c.is_nullable, COALESCE(c.column_default,''), \
             CASE WHEN pk.attname IS NOT NULL THEN 'PRI' ELSE '' END, COALESCE(c.identity_generation,'') \
             FROM information_schema.columns c \
             LEFT JOIN (SELECT a.attname FROM pg_index i \
               JOIN pg_class rel ON rel.oid=i.indrelid \
               JOIN pg_namespace n ON n.oid=rel.relnamespace \
               JOIN pg_attribute a ON a.attrelid=rel.oid AND a.attnum=ANY(i.indkey) \
               WHERE i.indisprimary AND n.nspname={s} AND rel.relname={t}) pk ON pk.attname=c.column_name \
             WHERE c.table_schema={s} AND c.table_name={t} ORDER BY c.ordinal_position"
        ),
    }
}

pub fn column_from_row(row: &[String]) -> Option<ColumnMeta> {
    if row.len() < 6 {
        return None;
    }
    Some(ColumnMeta {
        name: row[0].clone(),
        kind: row[1].clone(),
        nullable: row[2].eq_ignore_ascii_case("YES"),
        default: row[3].clone(),
        primary_key: row[4].contains("PRI"),
        extra: row[5].clone(),
    })
}

pub fn indexes_sql(kind: DbKind, database: &str, table: &str) -> String {
    let (schema, name) = split_qualified(kind, database, table);
    let (s, t) = (lit(kind, schema), lit(kind, name));
    match kind {
        // MIN(NON_UNIQUE) is 0 for a unique index; kept in MySQL's spelling so
        // one `index_from_row` reads both engines.
        DbKind::Mysql => format!(
            "SELECT INDEX_NAME, GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX SEPARATOR ','), MIN(NON_UNIQUE) \
             FROM information_schema.STATISTICS WHERE TABLE_SCHEMA={s} AND TABLE_NAME={t} \
             GROUP BY INDEX_NAME ORDER BY INDEX_NAME"
        ),
        _ => format!(
            "SELECT i.relname, string_agg(a.attname, ',' ORDER BY k.ord), \
             CASE WHEN ix.indisunique THEN 0 ELSE 1 END \
             FROM pg_class rel \
             JOIN pg_namespace n ON n.oid=rel.relnamespace \
             JOIN pg_index ix ON ix.indrelid=rel.oid \
             JOIN pg_class i ON i.oid=ix.indexrelid \
             JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) ON true \
             JOIN pg_attribute a ON a.attrelid=rel.oid AND a.attnum=k.attnum \
             WHERE n.nspname={s} AND rel.relname={t} \
             GROUP BY i.relname, ix.indisunique ORDER BY i.relname"
        ),
    }
}

/// Splits a comma-joined column list, dropping the empties a NULL aggregate
/// would leave behind.
fn split_columns(joined: &str) -> Vec<String> {
    joined.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

pub fn index_from_row(row: &[String]) -> Option<IndexMeta> {
    if row.len() < 3 {
        return None;
    }
    let name = row[0].clone();
    Some(IndexMeta {
        unique: row[2].trim() == "0",
        // Both engines name the primary key's index distinctly; MySQL calls it
        // PRIMARY, Postgres <table>_pkey.
        primary: name == "PRIMARY" || name.ends_with("_pkey"),
        columns: split_columns(&row[1]),
        name,
    })
}

pub fn foreign_keys_sql(kind: DbKind, database: &str, table: &str) -> String {
    let (schema, name) = split_qualified(kind, database, table);
    let (s, t) = (lit(kind, schema), lit(kind, name));
    match kind {
        DbKind::Mysql => format!(
            "SELECT k.CONSTRAINT_NAME, GROUP_CONCAT(k.COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ','), \
             k.REFERENCED_TABLE_NAME, \
             GROUP_CONCAT(k.REFERENCED_COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ','), \
             r.DELETE_RULE, r.UPDATE_RULE \
             FROM information_schema.KEY_COLUMN_USAGE k \
             JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
               ON r.CONSTRAINT_SCHEMA=k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME=k.CONSTRAINT_NAME \
             WHERE k.TABLE_SCHEMA={s} AND k.TABLE_NAME={t} AND k.REFERENCED_TABLE_NAME IS NOT NULL \
             GROUP BY k.CONSTRAINT_NAME, k.REFERENCED_TABLE_NAME, r.DELETE_RULE, r.UPDATE_RULE \
             ORDER BY k.CONSTRAINT_NAME"
        ),
        // confdeltype/confupdtype are single chars; mapped to the same words
        // MySQL's REFERENTIAL_CONSTRAINTS already uses.
        _ => format!(
            "SELECT con.conname, \
             (SELECT string_agg(a.attname, ',' ORDER BY k.ord) FROM unnest(con.conkey) WITH ORDINALITY k(attnum,ord) \
               JOIN pg_attribute a ON a.attrelid=con.conrelid AND a.attnum=k.attnum), \
             ft.relname, \
             (SELECT string_agg(a.attname, ',' ORDER BY k.ord) FROM unnest(con.confkey) WITH ORDINALITY k(attnum,ord) \
               JOIN pg_attribute a ON a.attrelid=con.confrelid AND a.attnum=k.attnum), \
             CASE con.confdeltype WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' \
               WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' END, \
             CASE con.confupdtype WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' \
               WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' END \
             FROM pg_constraint con \
             JOIN pg_class rel ON rel.oid=con.conrelid \
             JOIN pg_namespace n ON n.oid=rel.relnamespace \
             JOIN pg_class ft ON ft.oid=con.confrelid \
             WHERE con.contype='f' AND n.nspname={s} AND rel.relname={t} ORDER BY con.conname"
        ),
    }
}

pub fn foreign_key_from_row(row: &[String]) -> Option<ForeignKeyMeta> {
    if row.len() < 6 {
        return None;
    }
    Some(ForeignKeyMeta {
        name: row[0].clone(),
        columns: split_columns(&row[1]),
        references_table: row[2].clone(),
        references_columns: split_columns(&row[3]),
        on_delete: row[4].clone(),
        on_update: row[5].clone(),
    })
}

/// The mirror image of `foreign_keys_sql`: constraints whose *target* is this
/// table. Same column order, so `reverse_relation_from_row` reads it.
pub fn referenced_by_sql(kind: DbKind, database: &str, table: &str) -> String {
    let (schema, name) = split_qualified(kind, database, table);
    let (s, t) = (lit(kind, schema), lit(kind, name));
    match kind {
        DbKind::Mysql => format!(
            "SELECT k.CONSTRAINT_NAME, k.TABLE_NAME, \
             GROUP_CONCAT(k.COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ','), \
             GROUP_CONCAT(k.REFERENCED_COLUMN_NAME ORDER BY k.ORDINAL_POSITION SEPARATOR ',') \
             FROM information_schema.KEY_COLUMN_USAGE k \
             WHERE k.REFERENCED_TABLE_SCHEMA={s} AND k.REFERENCED_TABLE_NAME={t} \
             GROUP BY k.CONSTRAINT_NAME, k.TABLE_NAME ORDER BY k.TABLE_NAME, k.CONSTRAINT_NAME"
        ),
        _ => format!(
            "SELECT con.conname, child.relname, \
             (SELECT string_agg(a.attname, ',' ORDER BY k.ord) FROM unnest(con.conkey) WITH ORDINALITY k(attnum,ord) \
               JOIN pg_attribute a ON a.attrelid=con.conrelid AND a.attnum=k.attnum), \
             (SELECT string_agg(a.attname, ',' ORDER BY k.ord) FROM unnest(con.confkey) WITH ORDINALITY k(attnum,ord) \
               JOIN pg_attribute a ON a.attrelid=con.confrelid AND a.attnum=k.attnum) \
             FROM pg_constraint con \
             JOIN pg_class parent ON parent.oid=con.confrelid \
             JOIN pg_namespace n ON n.oid=parent.relnamespace \
             JOIN pg_class child ON child.oid=con.conrelid \
             WHERE con.contype='f' AND n.nspname={s} AND parent.relname={t} \
             ORDER BY child.relname, con.conname"
        ),
    }
}

pub fn reverse_relation_from_row(row: &[String]) -> Option<ReverseRelation> {
    if row.len() < 4 {
        return None;
    }
    Some(ReverseRelation {
        name: row[0].clone(),
        table: row[1].clone(),
        columns: split_columns(&row[2]),
        references_columns: split_columns(&row[3]),
    })
}

/// Lists a database's tables *and* views, with the row/size estimates the
/// sidebar shows. Views have neither, and report zero.
pub fn objects_sql(kind: DbKind, database: &str) -> String {
    match kind {
        DbKind::Mysql => format!(
            "SELECT TABLE_NAME, IF(TABLE_TYPE='VIEW','view','table'), IFNULL(TABLE_ROWS,0), \
             IFNULL(DATA_LENGTH,0)+IFNULL(INDEX_LENGTH,0) \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA={} AND TABLE_TYPE IN ('BASE TABLE','VIEW') \
             ORDER BY TABLE_NAME",
            quote_literal(kind, database)
        ),
        // relkind: r/p are tables and partitioned tables, v/m views and
        // materialized views. reltuples is -1 before the first ANALYZE.
        _ => "SELECT CASE WHEN n.nspname='public' THEN c.relname ELSE n.nspname||'.'||c.relname END, \
              CASE WHEN c.relkind IN ('v','m') THEN 'view' ELSE 'table' END, \
              c.reltuples::bigint, pg_total_relation_size(c.oid) \
              FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
              WHERE c.relkind IN ('r','p','v','m') AND n.nspname NOT IN ('pg_catalog','information_schema') \
              AND n.nspname NOT LIKE 'pg_toast%' ORDER BY 1"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unqualified_table_takes_the_database_on_mysql_and_public_on_postgres() {
        assert_eq!(split_qualified(DbKind::Mysql, "shop", "users"), ("shop", "users"));
        assert_eq!(split_qualified(DbKind::Postgres, "shop", "users"), ("public", "users"));
        assert_eq!(split_qualified(DbKind::Postgres, "shop", "audit.users"), ("audit", "users"));
    }

    /// The schema and table reach the catalogue queries as string literals, so
    /// they go through the same quoting as any other value.
    #[test]
    fn catalogue_queries_quote_the_names_they_look_up() {
        let sql = columns_sql(DbKind::Mysql, "shop", "o'brien");
        assert!(sql.contains("TABLE_NAME='o''brien'"), "{sql}");
        let sql = foreign_keys_sql(DbKind::Postgres, "shop", "audit.o'brien");
        assert!(sql.contains("n.nspname='audit'"), "{sql}");
        assert!(sql.contains("rel.relname='o''brien'"), "{sql}");
    }

    #[test]
    fn a_column_row_reads_nullability_and_the_primary_key_marker() {
        let row = ["id".into(), "bigint(20)".into(), "NO".into(), "".into(), "PRI".into(), "auto_increment".into()];
        assert_eq!(
            column_from_row(&row).unwrap(),
            ColumnMeta {
                name: "id".to_string(),
                kind: "bigint(20)".to_string(),
                nullable: false,
                default: String::new(),
                primary_key: true,
                extra: "auto_increment".to_string(),
            }
        );
        let nullable = ["note".into(), "text".into(), "YES".into(), "".into(), "".into(), "".into()];
        let meta = column_from_row(&nullable).unwrap();
        assert!(meta.nullable && !meta.primary_key);
        assert!(column_from_row(&["short".into()]).is_none());
    }

    #[test]
    fn an_index_row_splits_its_columns_and_reads_uniqueness_back_to_front() {
        // Both engines report MySQL's NON_UNIQUE sense: 0 means unique.
        let unique = ["PRIMARY".into(), "id".into(), "0".into()];
        let meta = index_from_row(&unique).unwrap();
        assert!(meta.unique && meta.primary);
        assert_eq!(meta.columns, vec!["id"]);

        let plain = ["idx_name_email".into(), "name,email".into(), "1".into()];
        let meta = index_from_row(&plain).unwrap();
        assert!(!meta.unique && !meta.primary);
        assert_eq!(meta.columns, vec!["name", "email"]);

        assert!(index_from_row(&["users_pkey".into(), "id".into(), "0".into()]).unwrap().primary);
    }

    #[test]
    fn a_foreign_key_row_keeps_both_sides_of_a_composite_reference_aligned() {
        let row = [
            "fk_line_order".into(),
            "order_id,line".into(),
            "orders".into(),
            "id,line_no".into(),
            "CASCADE".into(),
            "NO ACTION".into(),
        ];
        let fk = foreign_key_from_row(&row).unwrap();
        assert_eq!(fk.columns, vec!["order_id", "line"]);
        assert_eq!(fk.references_columns, vec!["id", "line_no"]);
        assert_eq!(fk.references_table, "orders");
        assert_eq!(fk.on_delete, "CASCADE");
    }

    #[test]
    fn the_reverse_query_looks_up_the_target_side_of_the_constraint() {
        // The mirror of foreign_keys_sql: same table, opposite end.
        let mysql = referenced_by_sql(DbKind::Mysql, "shop", "users");
        assert!(mysql.contains("REFERENCED_TABLE_NAME='users'"), "{mysql}");
        assert!(!mysql.contains("k.TABLE_NAME='users'"), "{mysql}");

        let pg = referenced_by_sql(DbKind::Postgres, "shop", "users");
        assert!(pg.contains("parent.relname='users'"), "{pg}");
        assert!(pg.contains("child.relname"), "{pg}");
    }

    #[test]
    fn a_reverse_relation_row_keeps_the_child_and_parent_columns_aligned() {
        let row = [
            "fk_task_project".into(),
            "tasks".into(),
            "project_id".into(),
            "id".into(),
        ];
        assert_eq!(
            reverse_relation_from_row(&row).unwrap(),
            ReverseRelation {
                name: "fk_task_project".to_string(),
                table: "tasks".to_string(),
                columns: vec!["project_id".to_string()],
                references_columns: vec!["id".to_string()],
            }
        );
        assert!(reverse_relation_from_row(&["short".into()]).is_none());
    }

    #[test]
    fn the_object_list_includes_views_and_says_which_is_which() {
        let mysql = objects_sql(DbKind::Mysql, "shop");
        assert!(mysql.contains("'BASE TABLE','VIEW'"), "{mysql}");
        assert!(mysql.contains("'view'"), "{mysql}");
        let pg = objects_sql(DbKind::Postgres, "shop");
        assert!(pg.contains("'r','p','v','m'"), "{pg}");
    }
}
