//! Running SQL on a MySQL server: one or more statements over the text protocol, their result
//! sets as text; reads tried read-only; the table browser's pages and counts; every matching
//! row streamed to an export file.

use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use mysql_async::prelude::Queryable;

use super::rows::select_sql;
use super::{My, my_error, relation, value_text};
use crate::db::export::{ExportFormat, ExportSummary, RowWriter, kinds_for};
use crate::db::query::{ColumnInfo, QueryOutcome, ReadOnlyTry, ResultSet, RowCount};
use crate::db::rows::RowQuery;
use crate::{CoreError, Result};

/// The server refused a write in a read-only transaction (`ER_CANT_EXECUTE_IN_READ_ONLY_TRANSACTION`).
const READ_ONLY_REFUSAL: u16 = 1792;

/// Above this many rows (the server's estimate), the browser shows the estimate rather than
/// counting every row.
const COUNT_LIMIT: i64 = 1_000_000;

/// Splits `sql` into its statements at the `;` outside strings, quoted names and comments
/// (`--`, `#`, `/* */`). Empty statements are left out.
pub fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '`' => {
                current.push(c);
                while let Some(d) = chars.next() {
                    current.push(d);
                    if d == '\\' && c != '`' {
                        current.extend(chars.next());
                    } else if d == c {
                        if chars.peek() == Some(&c) {
                            current.extend(chars.next());
                        } else {
                            break;
                        }
                    }
                }
            }
            '#' => {
                current.push(c);
                for d in chars.by_ref() {
                    current.push(d);
                    if d == '\n' {
                        break;
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                current.push(c);
                for d in chars.by_ref() {
                    current.push(d);
                    if d == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                current.push(c);
                current.extend(chars.next());
                let mut prev = ' ';
                for d in chars.by_ref() {
                    current.push(d);
                    if prev == '*' && d == '/' {
                        break;
                    }
                    prev = d;
                }
            }
            ';' => {
                if !strip_comments(&current).trim().is_empty() {
                    out.push(current.trim().to_string());
                }
                current.clear();
            }
            c => current.push(c),
        }
    }
    if !strip_comments(&current).trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// `sql` without its leading comments and whitespace (and opening parentheses), lower case: what
/// its first keyword is read from.
fn strip_comments(sql: &str) -> String {
    let mut rest = sql;
    loop {
        let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '(');
        if let Some(after) = trimmed.strip_prefix("--") {
            rest = after.split_once('\n').map(|(_, r)| r).unwrap_or("");
        } else if let Some(after) = trimmed.strip_prefix('#') {
            rest = after.split_once('\n').map(|(_, r)| r).unwrap_or("");
        } else if trimmed.starts_with("/*!") || trimmed.starts_with("/*+") {
            // An executable comment (`/*!80000 … */`) or an optimizer hint runs: it is the SQL.
            return trimmed.to_lowercase();
        } else if let Some(after) = trimmed.strip_prefix("/*") {
            rest = after.split_once("*/").map(|(_, r)| r).unwrap_or("");
        } else {
            return trimmed.to_lowercase();
        }
    }
}

/// Why `statement` can't run read-only, if it can't: anything but a read, a `SELECT` that
/// writes a server file, and switches of the read-only mode.
pub(crate) fn write_reason(statement: &str) -> Option<String> {
    let lowered = strip_comments(statement);
    let keyword: String = lowered
        .chars()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '_')
        .collect();
    let preview = || {
        statement
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let reads = matches!(
        keyword.as_str(),
        "select" | "with" | "show" | "describe" | "desc" | "explain" | "table" | "values" | "help"
    );
    if !reads {
        return Some(format!(
            "“{}…” changes the database, and this runs read-only.",
            preview()
        ));
    }
    let squeezed = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
    if squeezed.contains("into outfile") || squeezed.contains("into dumpfile") {
        return Some(
            "SELECT … INTO OUTFILE writes a file on the server, and this runs read-only.".into(),
        );
    }
    // `EXPLAIN ANALYZE` runs what it explains, which may be a write.
    if keyword == "explain" || keyword == "describe" || keyword == "desc" {
        let rest = squeezed
            .split_whitespace()
            .skip(1)
            .find(|w| {
                !w.starts_with("analyze") && !w.starts_with("format") && !w.starts_with("extended")
            })
            .unwrap_or("select");
        if matches!(rest, "insert" | "update" | "delete" | "replace")
            && squeezed.contains("analyze")
        {
            return Some(format!(
                "“{}…” runs a change, and this runs read-only.",
                preview()
            ));
        }
    }
    None
}

/// Runs `sql` (one or more statements) on `conn`, keeping at most `max_rows` rows per result
/// set; the rest are read and dropped.
pub(super) async fn run_on(
    conn: &mut mysql_async::Conn,
    sql: &str,
    max_rows: usize,
) -> std::result::Result<QueryOutcome, mysql_async::Error> {
    let started = Instant::now();
    let mut out = QueryOutcome::default();
    let mut result = conn.query_iter(sql).await?;
    while let Some(columns) = result.columns() {
        if columns.is_empty() {
            out.affected.push(result.affected_rows());
            // Moves on to the next result.
            result.next().await?;
            continue;
        }
        let mut set = ResultSet {
            columns: columns.iter().map(|c| c.name_str().into_owned()).collect(),
            ..ResultSet::default()
        };
        while let Some(row) = result.next().await? {
            if set.rows.len() < max_rows {
                set.rows.push(
                    (0..columns.len())
                        .map(|i| row.as_ref(i).and_then(|v| value_text(v, &columns[i])))
                        .collect(),
                );
            } else {
                set.truncated = true;
            }
        }
        out.results.push(set);
    }
    out.duration = started.elapsed();
    Ok(out)
}

impl My {
    /// Runs one or more statements (on the read-only handle, only reads).
    pub async fn run_sql(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
        if self.read_only {
            return match self.run_if_read_only(sql, max_rows).await? {
                ReadOnlyTry::Read(outcome) => Ok(outcome),
                ReadOnlyTry::Writes(why) | ReadOnlyTry::Several(why) => Err(CoreError::Sql(why)),
            };
        }
        let mut conn = self.conn().await?;
        run_on(&mut conn, sql, max_rows).await.map_err(my_error)
    }

    /// Runs `sql` read-only when every statement only reads (inside a read-only transaction,
    /// rolled back after); else nothing of it runs and the answer says why
    /// ([`ReadOnlyTry::Writes`]).
    pub async fn run_if_read_only(&self, sql: &str, max_rows: usize) -> Result<ReadOnlyTry> {
        let statements = split_statements(sql);
        if statements.is_empty() {
            return Ok(ReadOnlyTry::Read(QueryOutcome::default()));
        }
        if let Some(why) = statements.iter().find_map(|s| write_reason(s)) {
            return Ok(ReadOnlyTry::Writes(why));
        }
        let lowered = sql.to_lowercase();
        if lowered.contains("transaction_read_only") || lowered.contains("tx_read_only") {
            return Ok(ReadOnlyTry::Writes(
                "Read-only mode can't be changed.".into(),
            ));
        }
        let mut conn = self.conn().await?;
        conn.query_drop("START TRANSACTION READ ONLY")
            .await
            .map_err(my_error)?;
        let result = run_on(&mut conn, sql, max_rows).await;
        let _ = conn.query_drop("ROLLBACK").await;
        match result {
            Ok(outcome) => Ok(ReadOnlyTry::Read(outcome)),
            Err(mysql_async::Error::Server(e)) if e.code == READ_ONLY_REFUSAL => Ok(
                ReadOnlyTry::Writes(my_error(mysql_async::Error::Server(e)).to_string()),
            ),
            Err(e) => Err(my_error(e)),
        }
    }

    /// One statement, read-only; writes and several statements are refused.
    pub async fn run_read_only(&self, sql: &str, max_rows: usize) -> Result<QueryOutcome> {
        if split_statements(sql).len() > 1 {
            return Err(CoreError::Sql(
                "Only one statement can run read-only at a time.".into(),
            ));
        }
        match self.run_if_read_only(sql, max_rows).await? {
            ReadOnlyTry::Read(outcome) => Ok(outcome),
            ReadOnlyTry::Writes(why) | ReadOnlyTry::Several(why) => Err(CoreError::Sql(why)),
        }
    }

    /// Runs a data-changing statement; answers the rows it changed.
    pub async fn run_bulk(&self, sql: &str) -> Result<u64> {
        if self.read_only {
            return Err(CoreError::Sql("This connection is read-only.".into()));
        }
        let mut conn = self.conn().await?;
        let out = run_on(&mut conn, sql, 0).await.map_err(my_error)?;
        Ok(out.affected.iter().sum())
    }

    /// Rows of one query, every one (the schema's own reads).
    pub(super) async fn rows(&self, sql: &str) -> Result<Vec<Vec<Option<String>>>> {
        let mut conn = self.conn().await?;
        let out = run_on(&mut conn, sql, usize::MAX).await.map_err(my_error)?;
        Ok(out
            .results
            .into_iter()
            .next()
            .map(|s| s.rows)
            .unwrap_or_default())
    }

    /// The table's column names, in order, for the whole-row search (only asked when there is
    /// one).
    async fn search_columns(
        &self,
        schema: &str,
        table: &str,
        query: &RowQuery,
    ) -> Result<Vec<String>> {
        if query.search.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.column_names(schema, table).await
    }

    /// One page of `schema.table`, narrowed and sorted by `query`, `order_by` (the key)
    /// breaking ties.
    pub async fn fetch_rows(
        &self,
        schema: &str,
        table: &str,
        order_by: &[String],
        query: &RowQuery,
        offset: u64,
        limit: u64,
    ) -> Result<ResultSet> {
        let columns = self.search_columns(schema, table, query).await?;
        let sql = select_sql(
            schema,
            table,
            &columns,
            order_by,
            query,
            Some((offset, limit)),
        );
        let mut conn = self.conn().await?;
        let mut out = run_on(&mut conn, &sql, limit as usize)
            .await
            .map_err(my_error)?;
        Ok(out.results.pop().unwrap_or_default())
    }

    /// Rows of `schema.table` matching `query`, counted.
    pub async fn row_count_matching(
        &self,
        schema: &str,
        table: &str,
        query: &RowQuery,
    ) -> Result<RowCount> {
        let columns = self.search_columns(schema, table, query).await?;
        let filter = super::where_sql(query, &columns)
            .map(|w| format!(" WHERE {w}"))
            .unwrap_or_default();
        let rows = self
            .rows(&format!(
                "SELECT COUNT(*) FROM {}{filter}",
                relation(schema, table)
            ))
            .await?;
        let n = rows
            .first()
            .and_then(|r| r.first()?.as_deref()?.parse().ok())
            .unwrap_or(0);
        Ok(RowCount {
            rows: n,
            exact: true,
        })
    }

    /// Every row counted, or the server's estimate when it is a million or more.
    pub async fn row_count_estimated(
        &self,
        schema: &str,
        table: &str,
        estimate: Option<i64>,
    ) -> Result<RowCount> {
        match estimate {
            Some(n) if n >= COUNT_LIMIT => Ok(RowCount {
                rows: n as u64,
                exact: false,
            }),
            _ => {
                self.row_count_matching(schema, table, &RowQuery::default())
                    .await
            }
        }
    }

    /// Every row of `schema.table` matching `query`, in the browser's order, streamed to `path`.
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
        progress: &AtomicU64,
        stop: &AtomicBool,
    ) -> Result<ExportSummary> {
        let columns = self.search_columns(schema, table, query).await?;
        let sql = select_sql(schema, table, &columns, order_by, query, None);
        let mut conn = self.conn().await?;
        let file = std::fs::File::create(path)?;
        let result = async {
            let mut result = conn.query_iter(sql).await.map_err(my_error)?;
            let Some(cols) = result.columns().filter(|c| !c.is_empty()) else {
                return Err(CoreError::Sql(
                    "The query returned no rows to export.".into(),
                ));
            };
            let names: Vec<String> = cols.iter().map(|c| c.name_str().into_owned()).collect();
            let mut w = RowWriter::new(
                BufWriter::with_capacity(1 << 16, file),
                format,
                table,
                &names,
                kinds_for(&names, types),
            )?;
            while let Some(row) = result.next().await.map_err(my_error)? {
                if stop.load(Ordering::Relaxed) {
                    return Err(CoreError::Sql("Export canceled.".into()));
                }
                let full = w.is_full();
                let values: Vec<Option<String>> = (0..cols.len())
                    .map(|i| row.as_ref(i).and_then(|v| value_text(v, &cols[i])))
                    .collect();
                let refs: Vec<Option<&str>> = values.iter().map(|v| v.as_deref()).collect();
                w.row(&refs)?;
                if full {
                    // Excel holds no more: marked truncated, the rest dropped.
                    break;
                }
                progress.fetch_add(1, Ordering::Relaxed);
            }
            Ok(w.finish()?)
        }
        .await;
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        result
    }

    /// The table's column names, in order.
    pub async fn column_names(&self, schema: &str, table: &str) -> Result<Vec<String>> {
        let rows = self
            .rows(&format!("SHOW COLUMNS FROM {}", relation(schema, table)))
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect())
    }

    /// The server's databases (the system ones too, as `SHOW DATABASES` lists them).
    pub async fn list_databases(&self) -> Result<Vec<String>> {
        let rows = self.rows("SHOW DATABASES").await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect())
    }
}
