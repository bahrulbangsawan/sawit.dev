//! Rows to a CSV or Excel (.xlsx) file, written as they arrive so a whole
//! table never has to fit in memory. The .xlsx is a minimal workbook (one
//! sheet of inline strings) zipped here with a streamed sheet entry.

mod xlsx;
mod zip;

use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use futures::{StreamExt, pin_mut};
use tokio_postgres::{Client, SimpleQueryMessage};

use super::admin::find_relation;
use super::query::{ColumnInfo, ResultSet, load_schema, run_read_only, select_sql, sql_error};
use super::rows::RowQuery;
use crate::{CoreError, Result};
use xlsx::Xlsx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Xlsx,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Xlsx => "xlsx",
        }
    }

    /// The format a file name asks for: `.csv` or `.xlsx`.
    pub fn for_path(path: &Path) -> Result<ExportFormat> {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("csv") => Ok(ExportFormat::Csv),
            Some(e) if e.eq_ignore_ascii_case("xlsx") => Ok(ExportFormat::Xlsx),
            _ => Err(CoreError::Parse(format!(
                "{}: the file name must end in .csv or .xlsx.",
                path.display()
            ))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Csv => "CSV",
            ExportFormat::Xlsx => "Excel",
        }
    }
}

/// How a value is written to a spreadsheet cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Text,
    Number,
    Bool,
}

/// The cell kind for a `format_type` name (`integer`, `numeric(10,2)`, …), or a type SQLite
/// tables declare (`INTEGER`, `REAL`, `INT`, `DOUBLE`, any case).
pub fn cell_kind(data_type: &str) -> CellKind {
    let t = data_type.trim().to_ascii_lowercase();
    if t.ends_with(']') {
        return CellKind::Text;
    }
    match t.split('(').next().unwrap_or(&t).trim() {
        "smallint" | "integer" | "bigint" | "real" | "double precision" | "numeric" => {
            CellKind::Number
        }
        // SQLite's spellings.
        "int" | "tinyint" | "mediumint" | "int2" | "int8" | "float" | "double" | "decimal" => {
            CellKind::Number
        }
        "boolean" | "bool" => CellKind::Bool,
        _ => CellKind::Text,
    }
}

/// Excel's last row (the header takes the first).
pub const XLSX_MAX_ROWS: u64 = 1_048_576;

/// What was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSummary {
    pub rows: u64,
    /// Rows were left out: Excel's row limit was reached.
    pub truncated: bool,
}

/// Writes a header and then rows in either format.
pub struct RowWriter<W: Write> {
    inner: Inner<W>,
    kinds: Vec<CellKind>,
    rows: u64,
    truncated: bool,
}

enum Inner<W: Write> {
    Csv(W),
    Xlsx(Xlsx<W>),
}

impl<W: Write> RowWriter<W> {
    /// `kinds` (by column; missing ones are text) only matter for Excel.
    pub fn new(
        out: W,
        format: ExportFormat,
        sheet: &str,
        columns: &[String],
        kinds: Vec<CellKind>,
    ) -> io::Result<Self> {
        let mut inner = match format {
            ExportFormat::Csv => Inner::Csv(out),
            ExportFormat::Xlsx => Inner::Xlsx(Xlsx::new(out, sheet)?),
        };
        let header: Vec<Option<&str>> = columns.iter().map(|c| Some(c.as_str())).collect();
        match &mut inner {
            Inner::Csv(w) => write_csv_row(w, &header)?,
            Inner::Xlsx(x) => x.row(1, &header, &[], true)?,
        }
        Ok(Self {
            inner,
            kinds,
            rows: 0,
            truncated: false,
        })
    }

    /// No more rows fit (Excel's limit).
    pub fn is_full(&self) -> bool {
        matches!(self.inner, Inner::Xlsx(_)) && self.rows + 1 >= XLSX_MAX_ROWS
    }

    pub fn row(&mut self, values: &[Option<&str>]) -> io::Result<()> {
        if self.is_full() {
            self.truncated = true;
            return Ok(());
        }
        self.rows += 1;
        match &mut self.inner {
            Inner::Csv(w) => write_csv_row(w, values),
            Inner::Xlsx(x) => x.row(self.rows + 1, values, &self.kinds, false),
        }
    }

    pub fn finish(self) -> io::Result<ExportSummary> {
        match self.inner {
            Inner::Csv(mut w) => w.flush()?,
            Inner::Xlsx(x) => x.finish()?.flush()?,
        }
        Ok(ExportSummary {
            rows: self.rows,
            truncated: self.truncated,
        })
    }
}

/// One RFC 4180 record; NULL is an empty field.
fn write_csv_row(w: &mut impl Write, values: &[Option<&str>]) -> io::Result<()> {
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            w.write_all(b",")?;
        }
        let v = v.unwrap_or("");
        if v.contains([',', '"', '\n', '\r']) || v.starts_with(' ') || v.ends_with(' ') {
            write!(w, "\"{}\"", v.replace('"', "\"\""))?;
        } else {
            w.write_all(v.as_bytes())?;
        }
    }
    w.write_all(b"\r\n")
}

/// Kinds for `columns`, by name from the schema tree.
pub(super) fn kinds_for(columns: &[String], types: &[ColumnInfo]) -> Vec<CellKind> {
    columns
        .iter()
        .map(|c| {
            types
                .iter()
                .find(|t| &t.name == c)
                .map(|t| cell_kind(&t.data_type))
                .unwrap_or(CellKind::Text)
        })
        .collect()
}

/// Writes `path`, removing it again when writing fails part way.
pub(super) fn write_file<T>(
    path: &Path,
    f: impl FnOnce(BufWriter<std::fs::File>) -> Result<T>,
) -> Result<T> {
    let file = std::fs::File::create(path)?;
    let result = f(BufWriter::with_capacity(1 << 16, file));
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Rows already in memory (the page shown) to `path`.
pub fn export_result_set(
    set: &ResultSet,
    format: ExportFormat,
    sheet: &str,
    types: &[ColumnInfo],
    path: &Path,
) -> Result<ExportSummary> {
    write_file(path, |out| {
        let mut w = RowWriter::new(
            out,
            format,
            sheet,
            &set.columns,
            kinds_for(&set.columns, types),
        )?;
        for row in &set.rows {
            let values: Vec<Option<&str>> = row.iter().map(|v| v.as_deref()).collect();
            w.row(&values)?;
        }
        Ok(w.finish()?)
    })
}

/// Every row of `schema.table` matching `query`, in the browser's order,
/// streamed to `path`. `progress` counts rows written; setting `stop`
/// abandons the export (the file is removed).
#[allow(clippy::too_many_arguments)]
pub async fn export_rows(
    client: &Client,
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
    let sql = select_sql(schema, table, order_by, query, None);
    let stream = client.simple_query_raw(&sql).await.map_err(sql_error)?;
    let file = std::fs::File::create(path)?;
    let result = async {
        pin_mut!(stream);
        let mut out = Some(BufWriter::with_capacity(1 << 16, file));
        let mut writer: Option<RowWriter<BufWriter<std::fs::File>>> = None;
        while let Some(msg) = stream.next().await {
            if stop.load(Ordering::Relaxed) {
                return Err(CoreError::Sql("Export canceled.".into()));
            }
            match msg.map_err(sql_error)? {
                SimpleQueryMessage::RowDescription(cols) => {
                    let columns: Vec<String> = cols.iter().map(|c| c.name().to_string()).collect();
                    let kinds = kinds_for(&columns, types);
                    writer = Some(RowWriter::new(
                        out.take().expect("one result"),
                        format,
                        table,
                        &columns,
                        kinds,
                    )?);
                }
                SimpleQueryMessage::Row(row) => {
                    let Some(w) = writer.as_mut() else { continue };
                    let full = w.is_full();
                    let values: Vec<Option<&str>> = (0..row.len()).map(|i| row.get(i)).collect();
                    w.row(&values)?;
                    if full {
                        // Excel holds no more: marked truncated, the rest dropped.
                        break;
                    }
                    progress.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
        }
        match writer {
            Some(w) => Ok(w.finish()?),
            None => Err(CoreError::Sql(
                "The query returned no rows to export.".into(),
            )),
        }
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Every row of a table or view to `path` (CSV or Excel, by its extension),
/// in primary-key order. `schema` as in `find_relation`.
pub async fn export_table(
    client: &Client,
    schema: Option<&str>,
    table: &str,
    path: &Path,
) -> Result<ExportSummary> {
    let format = ExportFormat::for_path(path)?;
    let schemas = load_schema(client).await?;
    let (schema, relation) = find_relation(&schemas, schema, table)?;
    export_rows(
        client,
        schema,
        &relation.name,
        &relation.primary_key,
        &RowQuery::default(),
        format,
        &relation.columns,
        path,
        &AtomicU64::new(0),
        &AtomicBool::new(false),
    )
    .await
}

/// The rows of one read-only statement to `path` (CSV or Excel, by its
/// extension). Writes are refused, as in `run_read_only`.
pub async fn export_query(client: &Client, sql: &str, path: &Path) -> Result<ExportSummary> {
    let format = ExportFormat::for_path(path)?;
    let outcome = run_read_only(client, sql, XLSX_MAX_ROWS as usize).await?;
    let Some(set) = outcome.results.last() else {
        return Err(CoreError::Sql(
            "The query returned no rows to export.".into(),
        ));
    };
    let mut summary = export_result_set(set, format, "query", &[], path)?;
    summary.truncated |= set.truncated;
    Ok(summary)
}

#[cfg(test)]
mod tests;
