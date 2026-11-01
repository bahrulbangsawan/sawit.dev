//! Running SQL on a SQLite file: tried read-only first, writes in one transaction, and every
//! matching row streamed to an export file.

use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use rusqlite::fallible_iterator::FallibleIterator as _;
use rusqlite::{Batch, Connection};

use super::rows::select_sql;
use super::schema::column_names;
use super::{lite_error, value_text};
use crate::db::export::{ExportFormat, ExportSummary, RowWriter, kinds_for, write_file};
use crate::db::query::{ColumnInfo, QueryOutcome, ReadOnlyTry, ResultSet};
use crate::db::rows::RowQuery;
use crate::{CoreError, Result};

/// Runs every statement of `sql` on `conn`, keeping at most `max_rows` rows per result.
fn run_all(conn: &Connection, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    let started = Instant::now();
    let mut out = QueryOutcome::default();
    let mut batch = Batch::new(conn, sql);
    while let Some(mut stmt) = batch.next().map_err(lite_error)? {
        let n = stmt.column_count();
        let columns: Vec<String> = stmt.column_names().map(String::from).collect();
        let mut set = ResultSet {
            columns,
            ..ResultSet::default()
        };
        let mut rows = stmt.raw_query();
        while let Some(row) = rows.next().map_err(lite_error)? {
            if set.rows.len() < max_rows {
                set.rows.push(
                    (0..n)
                        .map(|i| row.get_ref(i).ok().and_then(value_text))
                        .collect(),
                );
            } else {
                set.truncated = true;
            }
        }
        drop(rows);
        if n > 0 {
            out.results.push(set);
        } else {
            out.affected.push(conn.changes());
        }
    }
    out.duration = started.elapsed();
    Ok(out)
}

/// Runs `sql` read-only: what it returned when every statement only reads; else, the first
/// statement that writes stops it ([`ReadOnlyTry::Writes`]), the reads before it having run
/// (they change nothing). The connection is read-only too, so a write can't get through.
pub fn run_if_read_only(conn: &Connection, sql: &str, max_rows: usize) -> Result<ReadOnlyTry> {
    let started = Instant::now();
    let mut out = QueryOutcome::default();
    let mut batch = Batch::new(conn, sql);
    while let Some(stmt) = batch.next().map_err(lite_error)? {
        if !stmt.readonly() {
            let text = stmt
                .expanded_sql()
                .unwrap_or_default()
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            return Ok(ReadOnlyTry::Writes(format!(
                "“{text}…” changes the database, and this runs read-only."
            )));
        }
        drop(stmt);
    }
    // Every statement reads: run them for real.
    let mut ran = run_all(conn, sql, max_rows)?;
    ran.duration = started.elapsed();
    out.results = ran.results;
    out.affected = ran.affected;
    out.duration = ran.duration;
    Ok(ReadOnlyTry::Read(out))
}

/// [`run_if_read_only`] for one statement, refusing writes and several statements, as
/// PostgreSQL's `run_read_only` does.
pub fn run_read_only(conn: &Connection, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    let mut count = 0;
    let mut batch = Batch::new(conn, sql);
    while batch.next().map_err(lite_error)?.is_some() {
        count += 1;
    }
    if count > 1 {
        return Err(CoreError::Sql(
            "Only one statement can run read-only at a time.".into(),
        ));
    }
    match run_if_read_only(conn, sql, max_rows)? {
        ReadOnlyTry::Read(outcome) => Ok(outcome),
        ReadOnlyTry::Writes(why) | ReadOnlyTry::Several(why) => Err(CoreError::Sql(why)),
    }
}

/// Runs `sql` (one or more statements) on a read-write connection in one transaction: all of
/// it lands, or none of it. As D1 itself, it takes no BEGIN or COMMIT of its own.
pub fn run_write(conn: &Connection, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
    conn.execute_batch("BEGIN IMMEDIATE").map_err(lite_error)?;
    match run_all(conn, sql, max_rows) {
        Ok(out) => {
            conn.execute_batch("COMMIT").map_err(lite_error)?;
            Ok(out)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Every row of `table` matching `query`, in the browser's order, streamed to `path`.
#[allow(clippy::too_many_arguments)]
pub fn export_rows(
    conn: &Connection,
    table: &str,
    order_by: &[String],
    query: &RowQuery,
    format: ExportFormat,
    types: &[ColumnInfo],
    path: &Path,
    progress: &AtomicU64,
    stop: &AtomicBool,
) -> Result<ExportSummary> {
    let columns = column_names(conn, table)?;
    let sql = select_sql(table, &columns, order_by, query, None);
    let mut stmt = conn.prepare(&sql).map_err(lite_error)?;
    let names: Vec<String> = stmt.column_names().map(String::from).collect();
    let n = names.len();
    let kinds = kinds_for(&names, types);
    write_file(path, |out: BufWriter<std::fs::File>| {
        let mut w = RowWriter::new(out, format, table, &names, kinds)?;
        let mut rows = stmt.raw_query();
        while let Some(row) = rows.next().map_err(lite_error)? {
            if stop.load(Ordering::Relaxed) {
                return Err(CoreError::Sql("Export canceled.".into()));
            }
            let full = w.is_full();
            let values: Vec<Option<String>> = (0..n)
                .map(|i| row.get_ref(i).ok().and_then(value_text))
                .collect();
            let refs: Vec<Option<&str>> = values.iter().map(|v| v.as_deref()).collect();
            w.row(&refs)?;
            if full {
                break;
            }
            progress.fetch_add(1, Ordering::Relaxed);
        }
        Ok(w.finish()?)
    })
}
