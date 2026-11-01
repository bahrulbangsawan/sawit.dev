//! Ad-hoc SQL: one or more statements over the simple protocol, their result sets as text.

use std::time::{Duration, Instant};

use futures::{StreamExt, pin_mut};
use tokio_postgres::{Client, SimpleQueryMessage};

use super::failure::sql_error;
use crate::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultSet {
    pub columns: Vec<String>,
    /// Text values; `None` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// More rows existed than `max_rows`.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct QueryOutcome {
    pub results: Vec<ResultSet>,
    /// Rows affected by statements that returned no result set.
    pub affected: Vec<u64>,
    pub duration: Duration,
}

/// Runs one or more statements (simple protocol, text values), keeping at
/// most `max_rows` rows per result set without buffering the rest.
pub async fn run_sql(client: &Client, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    run_statements(client, sql, max_rows)
        .await
        .map_err(sql_error)
}

/// [`run_sql`], with the server's error as it came, its SQLSTATE included.
pub(super) async fn run_statements(
    client: &Client,
    sql: &str,
    max_rows: usize,
) -> std::result::Result<QueryOutcome, tokio_postgres::Error> {
    let started = Instant::now();
    let stream = client.simple_query_raw(sql).await?;
    pin_mut!(stream);
    let mut out = QueryOutcome::default();
    let mut current: Option<ResultSet> = None;
    while let Some(msg) = stream.next().await {
        match msg? {
            SimpleQueryMessage::RowDescription(cols) => {
                current = Some(ResultSet {
                    columns: cols.iter().map(|c| c.name().to_string()).collect(),
                    ..ResultSet::default()
                });
            }
            SimpleQueryMessage::Row(row) => {
                let set = current.get_or_insert_with(|| ResultSet {
                    columns: row.columns().iter().map(|c| c.name().to_string()).collect(),
                    ..ResultSet::default()
                });
                if set.rows.len() < max_rows {
                    set.rows.push(
                        (0..row.len())
                            .map(|i| row.get(i).map(str::to_string))
                            .collect(),
                    );
                } else {
                    set.truncated = true;
                }
            }
            SimpleQueryMessage::CommandComplete(n) => match current.take() {
                Some(set) => out.results.push(set),
                None => out.affected.push(n),
            },
            _ => {}
        }
    }
    if let Some(set) = current.take() {
        out.results.push(set);
    }
    out.duration = started.elapsed();
    Ok(out)
}

/// Runs one data-changing statement (see [`bulk_sql`](crate::db::bulk_sql)) on
/// a read-write connection; a single statement commits or fails as a whole.
/// Returns the rows it changed.
pub async fn run_bulk(client: &Client, sql: &str) -> Result<u64> {
    let messages = client.simple_query(sql).await.map_err(sql_error)?;
    Ok(messages
        .iter()
        .find_map(|m| match m {
            SimpleQueryMessage::CommandComplete(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0))
}
