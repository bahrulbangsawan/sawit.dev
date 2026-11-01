//! The SQL `sawit <db> [table] ["sql"]` runs: the table list, a preview, SQL as given, or a
//! filter on the table.

use crate::db;

/// Rows shown by `sawit <db> <table>`.
const PREVIEW_ROWS: u32 = 100;

/// Tables and views of the user's schemas, with estimated row counts.
pub(super) const LIST_TABLES: &str = "\
SELECT n.nspname AS schema, c.relname AS name,
       CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'matview'
                      WHEN 'f' THEN 'foreign' ELSE 'table' END AS kind,
       CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END AS est_rows
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
  AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%'
ORDER BY 1, 2";

/// First words of complete statements; anything else after a table name is
/// a filter.
const SQL_STARTS: &[&str] = &[
    "select",
    "with",
    "insert",
    "update",
    "delete",
    "merge",
    "explain",
    "show",
    "table",
    "values",
    "create",
    "alter",
    "drop",
    "truncate",
    "copy",
    "set",
    "reset",
    "begin",
    "start",
    "commit",
    "rollback",
    "grant",
    "revoke",
    "analyze",
    "vacuum",
    "refresh",
    "call",
    "do",
    "comment",
    "lock",
    "listen",
    "notify",
    "prepare",
    "execute",
    "deallocate",
    "discard",
    "reindex",
    "cluster",
    "declare",
    "fetch",
    "close",
];

/// Clauses that may follow `SELECT * FROM table` as they are.
const CLAUSE_STARTS: &[&str] = &["where", "order", "limit", "offset", "group", "fetch"];

/// The statement for `sawit <db> [table] ["sql"]`: the table list, a preview,
/// SQL as given, or a filter on the table (`"phone is null"` becomes
/// `SELECT * FROM leads WHERE phone is null`).
pub fn plan_sql(table: Option<&str>, sql: Option<&str>) -> String {
    let first_word = |s: &str| {
        s.trim_start()
            .trim_start_matches('(')
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    match (table, sql) {
        (None, None) => LIST_TABLES.into(),
        (None, Some(sql)) => sql.into(),
        (Some(table), None) => {
            format!("SELECT * FROM {} LIMIT {PREVIEW_ROWS}", table_ref(table))
        }
        (Some(table), Some(sql)) => {
            let word = first_word(sql);
            if SQL_STARTS.contains(&word.as_str()) {
                sql.into()
            } else if CLAUSE_STARTS.contains(&word.as_str()) {
                format!("SELECT * FROM {} {}", table_ref(table), sql.trim())
            } else {
                format!("SELECT * FROM {} WHERE {}", table_ref(table), sql.trim())
            }
        }
    }
}

/// `leads` → `"leads"`, `crm.leads` → `"crm"."leads"`; already quoted
/// names are left alone.
fn table_ref(table: &str) -> String {
    if table.contains('"') {
        return table.into();
    }
    table
        .split('.')
        .map(db::quote_ident)
        .collect::<Vec<_>>()
        .join(".")
}
