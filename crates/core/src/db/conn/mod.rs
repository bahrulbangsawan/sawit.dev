//! One handle for every kind of database, so the sidebar, table grid, SQL
//! editor, diagrams, exports and `sawit-mcp` call the same operations
//! whether the profile is PostgreSQL ([`Conn::Pg`], the code in `query.rs`
//! as it was) or SQLite underneath ([`Conn::Lite`]: a `.sqlite` file, a D1
//! binding's local file, or a remote D1 database through Wrangler).
//!
//! Reads of a local file run on a short-lived read-only connection. Writes
//! take the route [`Lite::write_route`] finds: a running dev server's Local
//! Explorer (workerd owns the file then), the file itself (nothing has it
//! open), or Wrangler for a remote database. Every outcome says which.

mod dialect;
mod lite;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use tokio_postgres::{CancelToken, Client};

use super::export::{ExportFormat, ExportSummary, RowWriter, kinds_for, write_file};
use super::profile::{ConnProfile, DbKind};
use super::query::{ColumnInfo, QueryOutcome, ReadOnlyTry, ResultSet, RowCount, SchemaInfo};
use super::rows::RowQuery;
use super::{explorer, remote, sqlite};
use crate::{CoreError, Result};

pub use dialect::{bulk_sql_for, drop_sql_for, empty_sql_for, relation_sql_for};
pub use lite::{Lite, WriteRoute, holders};

/// A live handle on a database.
#[derive(Clone)]
pub enum Conn {
    Pg(Arc<Client>),
    Lite(Arc<Lite>),
}

impl Conn {
    /// A SQLite-backed handle for `profile`.
    pub fn lite(profile: ConnProfile, read_only: bool) -> Conn {
        Conn::Lite(Arc::new(Lite::new(profile, read_only)))
    }

    pub fn kind(&self) -> DbKind {
        match self {
            Conn::Pg(_) => DbKind::Postgres,
            Conn::Lite(l) => l.profile.kind,
        }
    }

    /// The link dropped (PostgreSQL); a file never "drops".
    pub fn is_closed(&self) -> bool {
        match self {
            Conn::Pg(c) => c.is_closed(),
            Conn::Lite(_) => false,
        }
    }

    /// A request holds a clone of this handle right now.
    pub fn in_use(&self) -> bool {
        match self {
            Conn::Pg(c) => Arc::strong_count(c) > 1,
            Conn::Lite(l) => Arc::strong_count(l) > 1,
        }
    }

    /// Cancels the statement in flight (PostgreSQL only; SQLite reads are short).
    pub fn cancel_token(&self) -> Option<CancelToken> {
        match self {
            Conn::Pg(c) => Some(c.cancel_token()),
            Conn::Lite(_) => None,
        }
    }

    /// The PostgreSQL client, for what only PostgreSQL has (backups, imports, roles).
    pub fn pg(&self) -> Option<&Arc<Client>> {
        match self {
            Conn::Pg(c) => Some(c),
            Conn::Lite(_) => None,
        }
    }

    pub async fn load_schema(&self) -> Result<Vec<SchemaInfo>> {
        match self {
            Conn::Pg(c) => super::load_schema(c).await,
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let schema = l.remote_schema(true).await?;
                sqlite::load_schema(&remote::shadow(&schema)?)
            }
            Conn::Lite(l) => l.read(sqlite::load_schema).await,
        }
    }

    pub async fn primary_key(&self, schema: &str, table: &str) -> Result<Vec<String>> {
        match self {
            Conn::Pg(c) => super::primary_key(c, schema, table).await,
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let copy = remote::shadow(&l.remote_schema(false).await?)?;
                sqlite::primary_key(&copy, table)
            }
            Conn::Lite(l) => {
                let table = table.to_string();
                l.read(move |c| sqlite::primary_key(c, &table)).await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn fetch_rows(
        &self,
        schema: &str,
        table: &str,
        order_by: &[String],
        query: &RowQuery,
        offset: u64,
        limit: u64,
    ) -> Result<ResultSet> {
        match self {
            Conn::Pg(c) => {
                super::fetch_rows(c, schema, table, order_by, query, offset, limit).await
            }
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let columns = l.remote_columns(table).await?;
                let sql =
                    sqlite::select_sql(table, &columns, order_by, query, Some((offset, limit)));
                let mut out = l.remote_run(&sql, limit as usize).await?;
                Ok(out.results.pop().unwrap_or(ResultSet {
                    columns,
                    ..ResultSet::default()
                }))
            }
            Conn::Lite(l) => {
                let (table, order_by, query) =
                    (table.to_string(), order_by.to_vec(), query.clone());
                l.read(move |c| sqlite::fetch_rows(c, &table, &order_by, &query, offset, limit))
                    .await
            }
        }
    }

    /// Rows in the relation: PostgreSQL's estimate-aware count, SQLite's exact one.
    pub async fn row_count_estimated(
        &self,
        schema: &str,
        table: &str,
        estimate: Option<i64>,
    ) -> Result<RowCount> {
        match self {
            Conn::Pg(c) => super::row_count_estimated(c, schema, table, estimate).await,
            Conn::Lite(_) => Ok(self
                .row_count_matching(schema, table, &RowQuery::default())
                .await?
                .unwrap_or(RowCount {
                    rows: 0,
                    exact: false,
                })),
        }
    }

    pub async fn row_count_matching(
        &self,
        schema: &str,
        table: &str,
        query: &RowQuery,
    ) -> Result<Option<RowCount>> {
        match self {
            Conn::Pg(c) => super::row_count_matching(c, schema, table, query).await,
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let columns = l.remote_columns(table).await?;
                let filter = sqlite::where_sql(query, &columns)
                    .map(|w| format!(" WHERE {w}"))
                    .unwrap_or_default();
                let sql = format!("SELECT count(*) AS n FROM {}{filter}", sqlite::quote(table));
                let out = l.remote_run(&sql, 1).await?;
                Ok(out
                    .results
                    .first()
                    .and_then(|s| s.rows.first()?.first()?.as_deref()?.parse().ok())
                    .map(|rows| RowCount { rows, exact: true }))
            }
            Conn::Lite(l) => {
                let (table, query) = (table.to_string(), query.clone());
                l.read(move |c| sqlite::row_count(c, &table, &query))
                    .await
                    .map(Some)
            }
        }
    }

    /// Runs one or more statements. On SQLite, SQL that only reads runs read-only; anything
    /// else takes the write route (callers ask first, see [`Self::run_if_read_only`]).
    pub async fn run_sql(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
        match self {
            Conn::Pg(c) => super::run_sql(c, sql, max_rows).await,
            Conn::Lite(l) => match l.run_if_read_only(sql, max_rows).await? {
                ReadOnlyTry::Read(outcome) => Ok(outcome),
                ReadOnlyTry::Writes(_) | ReadOnlyTry::Several(_) => {
                    Ok(l.write(sql, max_rows).await?.0)
                }
            },
        }
    }

    /// One statement, read-only; writes are refused.
    pub async fn run_read_only(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
        match self {
            Conn::Pg(c) => super::run_read_only(c, sql, max_rows).await,
            Conn::Lite(l) => {
                if explorer::split_statements(sql).len() > 1 {
                    return Err(CoreError::Sql(
                        "Only one statement can run read-only at a time.".into(),
                    ));
                }
                match l.run_if_read_only(sql, max_rows).await? {
                    ReadOnlyTry::Read(outcome) => Ok(outcome),
                    ReadOnlyTry::Writes(why) | ReadOnlyTry::Several(why) => {
                        Err(CoreError::Sql(why))
                    }
                }
            }
        }
    }

    pub async fn run_if_read_only(&self, sql: &str, max_rows: usize) -> Result<ReadOnlyTry> {
        match self {
            Conn::Pg(c) => super::run_if_read_only(c, sql, max_rows).await,
            Conn::Lite(l) => l.run_if_read_only(sql, max_rows).await,
        }
    }

    /// Runs a data-changing statement; answers the rows it changed.
    pub async fn run_bulk(&self, sql: &str) -> Result<u64> {
        match self {
            Conn::Pg(c) => super::run_bulk(c, sql).await,
            Conn::Lite(l) => Ok(l.write(sql, 0).await?.0.affected.iter().sum()),
        }
    }

    /// Every row of `schema.table` matching `query`, streamed to `path`.
    #[allow(clippy::too_many_arguments)]
    pub async fn export_rows(
        &self,
        schema: &str,
        table: &str,
        order_by: &[String],
        query: &RowQuery,
        format: ExportFormat,
        types: &[ColumnInfo],
        path: &Path,
        progress: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
    ) -> Result<ExportSummary> {
        match self {
            Conn::Pg(c) => {
                super::export_rows(
                    c, schema, table, order_by, query, format, types, path, &progress, &stop,
                )
                .await
            }
            Conn::Lite(l) if l.profile.kind == DbKind::D1Remote => {
                let columns = l.remote_columns(table).await?;
                let sql = sqlite::select_sql(table, &columns, order_by, query, None);
                let out = l.remote_run(&sql, usize::MAX).await?;
                let set = out.results.into_iter().next().unwrap_or(ResultSet {
                    columns,
                    ..ResultSet::default()
                });
                let (types, table, path) = (types.to_vec(), table.to_string(), path.to_path_buf());
                tokio::task::spawn_blocking(move || {
                    write_file(&path, |out| {
                        let mut w = RowWriter::new(
                            out,
                            format,
                            &table,
                            &set.columns,
                            kinds_for(&set.columns, &types),
                        )?;
                        for row in &set.rows {
                            let values: Vec<Option<&str>> =
                                row.iter().map(|v| v.as_deref()).collect();
                            w.row(&values)?;
                        }
                        Ok(w.finish()?)
                    })
                })
                .await
                .map_err(|_| CoreError::Sql("The export stopped unexpectedly.".into()))?
            }
            Conn::Lite(l) => {
                let (table, order_by, query, types, path) = (
                    table.to_string(),
                    order_by.to_vec(),
                    query.clone(),
                    types.to_vec(),
                    path.to_path_buf(),
                );
                l.read(move |c| {
                    sqlite::export_rows(
                        c, &table, &order_by, &query, format, &types, &path, &progress, &stop,
                    )
                })
                .await
            }
        }
    }
}

#[cfg(test)]
mod tests;
