//! The statements each kind of database takes for the same change: PostgreSQL's, MySQL's, or
//! SQLite's (which D1 speaks too).

use crate::db::profile::DbKind;
use crate::db::rows::{BulkChange, RowTarget};
use crate::db::{mysql, sqlite};

/// The statement that applies `change` to `target`, in `kind`'s dialect; `columns` are the
/// table's (SQLite's row search names each).
pub fn bulk_sql_for(
    kind: DbKind,
    schema: &str,
    table: &str,
    columns: &[String],
    target: &RowTarget,
    change: &BulkChange,
) -> String {
    match kind {
        DbKind::Postgres => crate::db::bulk_sql(schema, table, target, change),
        DbKind::Mysql => mysql::bulk_sql(schema, table, columns, target, change),
        _ => sqlite::bulk_sql(table, columns, target, change),
    }
}

/// The relation as `kind`'s SQL names it: `"schema"."table"`, or SQLite's `"table"`.
pub fn relation_sql_for(kind: DbKind, schema: &str, table: &str) -> String {
    match kind {
        DbKind::Postgres => format!(
            "{}.{}",
            crate::db::quote_ident(schema),
            crate::db::quote_ident(table)
        ),
        DbKind::Mysql => mysql::relation(schema, table),
        _ => sqlite::quote(table),
    }
}

/// The statement that empties `table`: PostgreSQL's TRUNCATE, SQLite's DELETE (it has no
/// TRUNCATE).
pub fn empty_sql_for(kind: DbKind, schema: &str, table: &str) -> String {
    match kind {
        DbKind::Postgres => crate::db::truncate_sql(schema, table),
        DbKind::Mysql => format!("TRUNCATE TABLE {}", mysql::relation(schema, table)),
        _ => format!("DELETE FROM {}", sqlite::quote(table)),
    }
}

/// The statement that deletes a table or view; SQLite has no CASCADE, so `cascade` is
/// PostgreSQL's only.
pub fn drop_sql_for(
    kind: DbKind,
    schema: &str,
    name: &str,
    rel: crate::db::RelKind,
    cascade: bool,
) -> String {
    match kind {
        DbKind::Postgres => crate::db::drop_sql(schema, name, rel, cascade),
        // MySQL takes CASCADE and ignores it; foreign keys still refuse a referenced table.
        DbKind::Mysql => format!(
            "DROP {} {}",
            if rel == crate::db::RelKind::View {
                "VIEW"
            } else {
                "TABLE"
            },
            mysql::relation(schema, name)
        ),
        _ => format!(
            "DROP {} {}",
            if rel == crate::db::RelKind::View {
                "VIEW"
            } else {
                "TABLE"
            },
            sqlite::quote(name)
        ),
    }
}
