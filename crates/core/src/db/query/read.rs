//! What the table browser reads: pages of a relation, narrowed and sorted by a [`RowQuery`],
//! and row counts bounded in time.

use tokio_postgres::types::Type;
use tokio_postgres::{Client, SimpleQueryMessage};

use super::failure::sql_error;
use super::run::{ResultSet, run_sql};
use crate::Result;
use crate::db::quote_ident;
use crate::db::rows::RowQuery;

/// Rows in a table or view; `exact` is false when only the planner's
/// estimate was available in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowCount {
    pub rows: u64,
    pub exact: bool,
}

/// Longest an exact `count(*)` may run before falling back to the estimate
/// (a slow count on a remote database would otherwise hold up the connection).
const COUNT_TIMEOUT: &str = "3s";

/// Exact count for relations estimated below ~1M rows (bounded by
/// `COUNT_TIMEOUT`), else the statistics estimate.
pub async fn row_count(client: &Client, schema: &str, table: &str) -> Result<RowCount> {
    row_count_within(client, schema, table, COUNT_TIMEOUT).await
}

/// [`row_count`] with an explicit Postgres interval for the exact count.
pub async fn row_count_within(
    client: &Client,
    schema: &str,
    table: &str,
    timeout: &str,
) -> Result<RowCount> {
    let estimate: i64 = client
        .query_typed(
            "SELECT c.reltuples::bigint FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2",
            &[(&schema, Type::TEXT), (&table, Type::TEXT)],
        )
        .await
        .map_err(sql_error)?
        .first()
        .map(|r| r.get(0))
        .unwrap_or(-1);
    count_with_estimate(client, schema, table, estimate, timeout).await
}

/// [`row_count`] when the planner's estimate is already known (from
/// [`load_schema`](super::load_schema)); `None` (never analyzed) counts exactly.
pub async fn row_count_estimated(
    client: &Client,
    schema: &str,
    table: &str,
    estimate: Option<i64>,
) -> Result<RowCount> {
    count_with_estimate(client, schema, table, estimate.unwrap_or(-1), COUNT_TIMEOUT).await
}

async fn count_with_estimate(
    client: &Client,
    schema: &str,
    table: &str,
    estimate: i64,
    timeout: &str,
) -> Result<RowCount> {
    let fallback = RowCount {
        rows: estimate.max(0) as u64,
        exact: false,
    };
    if estimate >= 1_000_000 {
        return Ok(fallback);
    }
    // One implicit transaction, so SET LOCAL only bounds this count.
    let sql = format!(
        "SET LOCAL statement_timeout = '{}'; SELECT count(*) FROM {}.{}",
        timeout.replace('\'', "''"),
        quote_ident(schema),
        quote_ident(table)
    );
    match client.simple_query(&sql).await {
        Ok(messages) => {
            let rows = messages
                .iter()
                .find_map(|m| match m {
                    SimpleQueryMessage::Row(r) => r.get(0).and_then(|v| v.parse().ok()),
                    _ => None,
                })
                .unwrap_or(0);
            Ok(RowCount { rows, exact: true })
        }
        Err(e) if e.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) => {
            Ok(fallback)
        }
        Err(e) => Err(sql_error(e)),
    }
}

/// One page of a table or view. Ordered by `order_by` (normally the primary
/// key) so pages are stable while rows change; without it, physical order
/// (stable only for an unchanged table).
pub async fn fetch_page(
    client: &Client,
    schema: &str,
    table: &str,
    order_by: &[String],
    offset: u64,
    limit: u64,
) -> Result<ResultSet> {
    fetch_rows(
        client,
        schema,
        table,
        order_by,
        &RowQuery::default(),
        offset,
        limit,
    )
    .await
}

/// The relation's alias in browser queries; a whole-row search reads
/// `alias::text`, which a column of the same name would shadow.
const ROW_ALIAS: &str = "\"sawit row\"";

/// [`fetch_page`] narrowed and sorted by `query`; `order_by` (the key)
/// still breaks ties.
pub async fn fetch_rows(
    client: &Client,
    schema: &str,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    offset: u64,
    limit: u64,
) -> Result<ResultSet> {
    let sql = select_sql(schema, table, order_by, query, Some((offset, limit)));
    let mut out = run_sql(client, &sql, limit as usize).await?;
    Ok(out.results.pop().unwrap_or_default())
}

/// The browser's SELECT for `query`; `page` is (offset, limit), `None` for
/// every row.
pub(in crate::db) fn select_sql(
    schema: &str,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    page: Option<(u64, u64)>,
) -> String {
    // Without any order, pages follow physical order, which synchronized
    // seqscans would shuffle. SET LOCAL lasts for this one implicit
    // transaction, so it is safe behind a pooler too.
    let (prelude, order) = match query.order_sql(ROW_ALIAS, order_by) {
        Some(cols) => ("", format!(" ORDER BY {cols}")),
        None => ("SET LOCAL synchronize_seqscans = off; ", String::new()),
    };
    let filter = query
        .where_sql(ROW_ALIAS)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let range = page
        .map(|(offset, limit)| format!(" OFFSET {offset} LIMIT {limit}"))
        .unwrap_or_default();
    format!(
        "{prelude}SELECT {ROW_ALIAS}.* FROM {}.{} AS {ROW_ALIAS}{filter}{order}{range}",
        quote_ident(schema),
        quote_ident(table)
    )
}

/// Rows matching `query`'s filters, counted exactly within `COUNT_TIMEOUT`;
/// `None` when that ran out (the estimate says nothing about a filter).
pub async fn row_count_matching(
    client: &Client,
    schema: &str,
    table: &str,
    query: &RowQuery,
) -> Result<Option<RowCount>> {
    let filter = query
        .where_sql(ROW_ALIAS)
        .map(|w| format!(" WHERE {w}"))
        .unwrap_or_default();
    let sql = format!(
        "SET LOCAL statement_timeout = '{COUNT_TIMEOUT}'; SELECT count(*) FROM {}.{} AS {ROW_ALIAS}{filter}",
        quote_ident(schema),
        quote_ident(table)
    );
    match client.simple_query(&sql).await {
        Ok(messages) => Ok(messages.iter().find_map(|m| match m {
            SimpleQueryMessage::Row(r) => r
                .get(0)
                .and_then(|v| v.parse().ok())
                .map(|rows| RowCount { rows, exact: true }),
            _ => None,
        })),
        Err(e) if e.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED) => Ok(None),
        Err(e) => Err(sql_error(e)),
    }
}
