//! What the table browser reads and changes, in SQLite's dialect: filters, sort, pages, counts
//! and bulk changes.

use rusqlite::Connection;

use super::schema::column_names;
use super::{lite_error, literal, quote, value_text};
use crate::Result;
use crate::db::query::{ResultSet, RowCount};
use crate::db::rows::{BulkChange, FilterOp, RowQuery, RowTarget, SortDir};

/// A LIKE pattern matching `value` literally, for `ESCAPE '!'`. LIKE ignores ASCII case in
/// SQLite, as ILIKE does in PostgreSQL.
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

/// The `WHERE` condition for `query` (SQLite's dialect of [`RowQuery::where_sql`]); the
/// whole-row search looks in each of `columns`.
pub fn where_sql(query: &RowQuery, columns: &[String]) -> Option<String> {
    let mut parts: Vec<String> = query
        .filters
        .iter()
        .map(|f| {
            let col = quote(&f.column);
            let v = || literal(&f.value);
            let text = format!("CAST({col} AS TEXT)");
            match f.op {
                // The column's affinity applies to the literal, so `id = '7'` compares numbers.
                FilterOp::Eq => format!("{col} = {}", v()),
                FilterOp::Ne => format!("{col} IS NOT {}", v()),
                FilterOp::Gt => format!("{col} > {}", v()),
                FilterOp::Ge => format!("{col} >= {}", v()),
                FilterOp::Lt => format!("{col} < {}", v()),
                FilterOp::Le => format!("{col} <= {}", v()),
                FilterOp::Contains => {
                    format!("{text} LIKE {} ESCAPE '!'", like("%", &f.value, "%"))
                }
                FilterOp::NotContains => format!(
                    "({col} IS NULL OR {text} NOT LIKE {} ESCAPE '!')",
                    like("%", &f.value, "%")
                ),
                FilterOp::StartsWith => {
                    format!("{text} LIKE {} ESCAPE '!'", like("", &f.value, "%"))
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
            .map(|c| format!("CAST({} AS TEXT) LIKE {pattern} ESCAPE '!'", quote(c)))
            .collect();
        parts.push(format!("({})", any.join(" OR ")));
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

/// The ORDER BY list: the sort column first (NULLs last either way), then `key`.
pub fn order_sql(query: &RowQuery, key: &[String]) -> Option<String> {
    let mut cols: Vec<String> = Vec::new();
    if let Some((column, dir)) = &query.sort {
        cols.push(format!(
            "{} {} NULLS LAST",
            quote(column),
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
    format!("SELECT * FROM {}{filter}{order}{range}", quote(table))
}

/// Runs one query, keeping at most `max_rows` rows.
fn query_rows(conn: &Connection, sql: &str, max_rows: usize) -> Result<ResultSet> {
    let mut stmt = conn.prepare(sql).map_err(lite_error)?;
    let columns: Vec<String> = stmt.column_names().map(String::from).collect();
    let n = columns.len();
    let mut set = ResultSet {
        columns,
        ..ResultSet::default()
    };
    let mut rows = stmt.raw_query();
    while let Some(row) = rows.next().map_err(lite_error)? {
        if set.rows.len() >= max_rows {
            set.truncated = true;
            break;
        }
        set.rows.push(
            (0..n)
                .map(|i| row.get_ref(i).ok().and_then(value_text))
                .collect(),
        );
    }
    Ok(set)
}

/// One page of `table`, narrowed and sorted by `query`, `order_by` (the key) breaking ties.
pub fn fetch_rows(
    conn: &Connection,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    offset: u64,
    limit: u64,
) -> Result<ResultSet> {
    let columns = column_names(conn, table)?;
    let sql = select_sql(table, &columns, order_by, query, Some((offset, limit)));
    query_rows(conn, &sql, limit as usize)
}

/// Rows of `table` matching `query`, counted exactly (local files are small enough).
pub fn row_count(conn: &Connection, table: &str, query: &RowQuery) -> Result<RowCount> {
    let columns = column_names(conn, table)?;
    let filter = where_sql(query, &columns)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let rows: i64 = conn
        .query_row(
            &format!("SELECT count(*) FROM {}{filter}", quote(table)),
            [],
            |r| r.get(0),
        )
        .map_err(lite_error)?;
    Ok(RowCount {
        rows: rows.max(0) as u64,
        exact: true,
    })
}

/// The one statement that applies `change` to `target` in `table` (SQLite's dialect of
/// [`crate::db::bulk_sql`]); `columns` are the table's, for the row search.
pub fn bulk_sql(
    table: &str,
    columns: &[String],
    target: &RowTarget,
    change: &BulkChange,
) -> String {
    let filter = match target {
        RowTarget::Matching(query) => where_sql(query, columns),
        RowTarget::Keys { rows, .. } if rows.is_empty() => Some("0".into()),
        RowTarget::Keys { key, rows } => Some(
            rows.iter()
                .map(|values| {
                    let terms: Vec<String> = key
                        .iter()
                        .zip(values)
                        .map(|(k, v)| format!("{} = {}", quote(k), literal(v)))
                        .collect();
                    format!("({})", terms.join(" AND "))
                })
                .collect::<Vec<_>>()
                .join(" OR "),
        ),
    }
    .map(|w| format!(" WHERE {w}"))
    .unwrap_or_default();
    match change {
        BulkChange::Delete => format!("DELETE FROM {}{filter}", quote(table)),
        BulkChange::Update { column, value } => format!(
            "UPDATE {} SET {} = {}{filter}",
            quote(table),
            quote(column),
            value
                .as_deref()
                .map(literal)
                .unwrap_or_else(|| "NULL".into())
        ),
    }
}
