//! What the table browser reads and changes, in MySQL's dialect: filters, sort, pages and bulk
//! changes. Identifiers are quoted and values become literals, never raw SQL.

use super::{literal, quote, relation};
use crate::db::rows::{BulkChange, FilterOp, RowQuery, RowTarget, SortDir};

/// A LIKE pattern matching `value` literally, for `ESCAPE '!'`. Text compares under the
/// connection's collation (`utf8mb4_0900_ai_ci`, MariaDB's `utf8mb4_general_ci`), which ignores
/// case, as ILIKE does in PostgreSQL.
fn like(prefix: &str, value: &str, suffix: &str) -> String {
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '!' | '%' | '_' => vec!['!', c],
            c => vec![c],
        })
        .collect();
    literal(&format!("{prefix}{escaped}{suffix}"))
}

/// `col` as text, for LIKE: a binary column is compared as its bytes would read as UTF-8.
fn text(col: &str) -> String {
    format!("CAST({col} AS CHAR)")
}

/// The `WHERE` condition for `query` (MySQL's dialect of [`RowQuery::where_sql`]); the whole-row
/// search looks in each of `columns`.
pub fn where_sql(query: &RowQuery, columns: &[String]) -> Option<String> {
    let mut parts: Vec<String> = query
        .filters
        .iter()
        .map(|f| {
            let col = quote(&f.column);
            let v = || literal(&f.value);
            match f.op {
                // The literal converts to the column's type, so `id = '7'` compares numbers.
                FilterOp::Eq => format!("{col} = {}", v()),
                FilterOp::Ne => format!("NOT ({col} <=> {})", v()),
                FilterOp::Gt => format!("{col} > {}", v()),
                FilterOp::Ge => format!("{col} >= {}", v()),
                FilterOp::Lt => format!("{col} < {}", v()),
                FilterOp::Le => format!("{col} <= {}", v()),
                FilterOp::Contains => format!(
                    "{} LIKE {} ESCAPE '!'",
                    text(&col),
                    like("%", &f.value, "%")
                ),
                FilterOp::NotContains => format!(
                    "({col} IS NULL OR {} NOT LIKE {} ESCAPE '!')",
                    text(&col),
                    like("%", &f.value, "%")
                ),
                FilterOp::StartsWith => {
                    format!("{} LIKE {} ESCAPE '!'", text(&col), like("", &f.value, "%"))
                }
                FilterOp::IsNull => format!("{col} IS NULL"),
                FilterOp::IsNotNull => format!("{col} IS NOT NULL"),
            }
        })
        .collect();
    let search = query.search.trim();
    if !search.is_empty() && !columns.is_empty() {
        let pattern = like("%", search, "%");
        let any: Vec<String> = columns
            .iter()
            .map(|c| format!("{} LIKE {pattern} ESCAPE '!'", text(&quote(c))))
            .collect();
        parts.push(format!("({})", any.join(" OR ")));
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

/// The ORDER BY list: the sort column first, NULLs last either way (MySQL has no `NULLS LAST`:
/// `col IS NULL` sorts them after), then `key` so equal values keep a stable order across pages.
pub fn order_sql(query: &RowQuery, key: &[String]) -> Option<String> {
    let mut cols: Vec<String> = Vec::new();
    if let Some((column, dir)) = &query.sort {
        let col = quote(column);
        cols.push(format!("{col} IS NULL"));
        cols.push(format!(
            "{col} {}",
            match dir {
                SortDir::Asc => "ASC",
                SortDir::Desc => "DESC",
            }
        ));
    }
    cols.extend(
        key.iter()
            .filter(|k| query.sort.as_ref().is_none_or(|(c, _)| c != *k))
            .map(|k| quote(k)),
    );
    (!cols.is_empty()).then(|| cols.join(", "))
}

/// The browser's SELECT; `page` is (offset, limit), `None` for every row.
pub fn select_sql(
    schema: &str,
    table: &str,
    columns: &[String],
    order_by: &[String],
    query: &RowQuery,
    page: Option<(u64, u64)>,
) -> String {
    let filter = where_sql(query, columns)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let order = order_sql(query, order_by)
        .map(|o| format!(" ORDER BY {o}"))
        .unwrap_or_default();
    let range = page
        .map(|(offset, limit)| format!(" LIMIT {limit} OFFSET {offset}"))
        .unwrap_or_default();
    format!(
        "SELECT * FROM {}{filter}{order}{range}",
        relation(schema, table)
    )
}

/// The one statement that applies `change` to `target` in `schema.table` (MySQL's dialect of
/// [`crate::db::bulk_sql`]); `columns` are the table's, for the row search.
pub fn bulk_sql(
    schema: &str,
    table: &str,
    columns: &[String],
    target: &RowTarget,
    change: &BulkChange,
) -> String {
    let filter = match target {
        RowTarget::Matching(query) => where_sql(query, columns),
        RowTarget::Keys { rows, .. } if rows.is_empty() => Some("FALSE".into()),
        RowTarget::Keys { key, rows } => {
            let tuple = |values: &Vec<String>| {
                values
                    .iter()
                    .map(|v| literal(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let list = rows
                .iter()
                .map(|r| {
                    if key.len() == 1 {
                        tuple(r)
                    } else {
                        format!("({})", tuple(r))
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            Some(if key.len() == 1 {
                format!("{} IN ({list})", quote(&key[0]))
            } else {
                let cols = key.iter().map(|k| quote(k)).collect::<Vec<_>>().join(", ");
                format!("({cols}) IN ({list})")
            })
        }
    }
    .map(|w| format!(" WHERE {w}"))
    .unwrap_or_default();
    let rel = relation(schema, table);
    match change {
        BulkChange::Delete => format!("DELETE FROM {rel}{filter}"),
        BulkChange::Update { column, value } => format!(
            "UPDATE {rel} SET {} = {}{filter}",
            quote(column),
            value
                .as_deref()
                .map(literal)
                .unwrap_or_else(|| "NULL".into())
        ),
    }
}
