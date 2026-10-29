//! Rows to a CSV or Excel (.xlsx) file, written as they arrive so a whole
//! table never has to fit in memory. The .xlsx is a minimal workbook (one
//! sheet of inline strings) zipped here with a streamed sheet entry.

use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use flate2::Compression;
use flate2::write::DeflateEncoder;
use futures::{StreamExt, pin_mut};
use tokio_postgres::{Client, SimpleQueryMessage};

use super::admin::find_relation;
use super::query::{ColumnInfo, ResultSet, load_schema, run_read_only, select_sql, sql_error};
use super::rows::RowQuery;
use crate::{CoreError, Result};

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

/// The cell kind for a `format_type` name (`integer`, `numeric(10,2)`, …).
pub fn cell_kind(data_type: &str) -> CellKind {
    let t = data_type.trim();
    if t.ends_with(']') {
        return CellKind::Text;
    }
    match t.split('(').next().unwrap_or(t).trim() {
        "smallint" | "integer" | "bigint" | "real" | "double precision" | "numeric" => {
            CellKind::Number
        }
        "boolean" => CellKind::Bool,
        _ => CellKind::Text,
    }
}

/// Excel's last row (the header takes the first).
pub const XLSX_MAX_ROWS: u64 = 1_048_576;

/// Excel's longest cell text.
const XLSX_MAX_CELL_CHARS: usize = 32_767;

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

/// Column letters: 0 is A, 25 is Z, 26 is AA.
fn column_letters(mut ix: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (ix % 26) as u8);
        if ix < 26 {
            break;
        }
        ix = ix / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

/// Text for XML, without the characters XML 1.0 can't hold.
fn xml_escape(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

/// A number Excel reads back unchanged (it keeps 15 significant digits).
fn excel_number(value: &str) -> Option<&str> {
    let v = value.trim();
    let digits = v
        .split(['e', 'E'])
        .next()
        .unwrap_or(v)
        .chars()
        .filter(char::is_ascii_digit)
        .count();
    (digits <= 15 && v.parse::<f64>().is_ok_and(f64::is_finite)).then_some(v)
}

/// A sheet name Excel accepts: at most 31 characters, none of `[]:*?/\`.
fn sheet_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();
    let clean = clean.trim_matches('\'').to_string();
    if clean.is_empty() {
        "Sheet1".into()
    } else {
        clean
    }
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;

/// Style 1 is the bold header.
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

/// The header row stays in view while scrolling.
const SHEET_START: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetData>"#;

const SHEET_END: &str = "</sheetData></worksheet>";

/// A one-sheet workbook whose sheet is deflated as rows come in.
struct Xlsx<W: Write> {
    sheet: DeflateEncoder<Zip<W>>,
    crc: crc32fast::Hasher,
    size: u64,
    /// Where the sheet's compressed data starts.
    data_start: u64,
    line: String,
}

impl<W: Write> Xlsx<W> {
    fn new(out: W, sheet: &str) -> io::Result<Self> {
        let mut zip = Zip::new(out);
        let mut name = String::new();
        xml_escape(&sheet_name(sheet), &mut name);
        let workbook = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{name}" sheetId="1" r:id="rId1"/></sheets></workbook>"#
        );
        zip.file("[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
        zip.file("_rels/.rels", ROOT_RELS.as_bytes())?;
        zip.file("xl/workbook.xml", workbook.as_bytes())?;
        zip.file("xl/_rels/workbook.xml.rels", WORKBOOK_RELS.as_bytes())?;
        zip.file("xl/styles.xml", STYLES.as_bytes())?;
        zip.start_streamed("xl/worksheets/sheet1.xml")?;
        let data_start = zip.written;
        let mut this = Self {
            sheet: DeflateEncoder::new(zip, Compression::default()),
            crc: crc32fast::Hasher::new(),
            size: 0,
            data_start,
            line: String::new(),
        };
        this.put(SHEET_START)?;
        Ok(this)
    }

    fn put(&mut self, text: &str) -> io::Result<()> {
        self.crc.update(text.as_bytes());
        self.size += text.len() as u64;
        self.sheet.write_all(text.as_bytes())
    }

    /// Row `r` (1-based); NULL cells are left out.
    fn row(
        &mut self,
        r: u64,
        values: &[Option<&str>],
        kinds: &[CellKind],
        header: bool,
    ) -> io::Result<()> {
        let mut line = std::mem::take(&mut self.line);
        line.clear();
        line.push_str(&format!("<row r=\"{r}\">"));
        for (i, v) in values.iter().enumerate() {
            let Some(v) = v else { continue };
            let at = format!("{}{r}", column_letters(i));
            let kind = if header {
                CellKind::Text
            } else {
                kinds.get(i).copied().unwrap_or(CellKind::Text)
            };
            match (kind, *v) {
                (CellKind::Number, v) if excel_number(v).is_some() => {
                    line.push_str(&format!("<c r=\"{at}\"><v>{}</v></c>", v.trim()));
                }
                (CellKind::Bool, "t" | "true") => {
                    line.push_str(&format!("<c r=\"{at}\" t=\"b\"><v>1</v></c>"))
                }
                (CellKind::Bool, "f" | "false") => {
                    line.push_str(&format!("<c r=\"{at}\" t=\"b\"><v>0</v></c>"))
                }
                (_, v) => {
                    let style = if header { " s=\"1\"" } else { "" };
                    line.push_str(&format!(
                        "<c r=\"{at}\" t=\"inlineStr\"{style}><is><t xml:space=\"preserve\">"
                    ));
                    match v.char_indices().nth(XLSX_MAX_CELL_CHARS) {
                        Some((cut, _)) => xml_escape(&v[..cut], &mut line),
                        None => xml_escape(v, &mut line),
                    }
                    line.push_str("</t></is></c>");
                }
            }
        }
        line.push_str("</row>");
        let result = self.put(&line);
        self.line = line;
        result
    }

    fn finish(mut self) -> io::Result<W> {
        self.put(SHEET_END)?;
        let mut zip = self.sheet.finish()?;
        let compressed = zip.written - self.data_start;
        zip.end_streamed(self.crc.finalize(), compressed, self.size)?;
        zip.finish()
    }
}

/// Just enough of a zip writer for a workbook: deflated entries, the last
/// one streamed with a data descriptor. No zip64, so 4 GiB per entry.
struct Zip<W: Write> {
    out: W,
    written: u64,
    entries: Vec<ZipEntry>,
}

struct ZipEntry {
    name: String,
    flags: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

/// 1980-01-01 00:00, the earliest DOS date.
const DOS_DATE: u16 = 0x21;

fn too_large() -> io::Error {
    io::Error::other("The sheet is too large for an Excel file. Export as CSV instead.")
}

impl<W: Write> Write for Zip<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.out.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

impl<W: Write> Zip<W> {
    fn new(out: W) -> Self {
        Self {
            out,
            written: 0,
            entries: Vec::new(),
        }
    }

    fn header(&mut self, entry: &ZipEntry) -> io::Result<()> {
        let mut h = Vec::with_capacity(30 + entry.name.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes()); // version needed
        h.extend_from_slice(&entry.flags.to_le_bytes());
        h.extend_from_slice(&8u16.to_le_bytes()); // deflate
        h.extend_from_slice(&0u16.to_le_bytes()); // time
        h.extend_from_slice(&DOS_DATE.to_le_bytes());
        h.extend_from_slice(&entry.crc.to_le_bytes());
        h.extend_from_slice(&entry.compressed.to_le_bytes());
        h.extend_from_slice(&entry.size.to_le_bytes());
        h.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes()); // extra
        h.extend_from_slice(entry.name.as_bytes());
        self.write_all(&h)
    }

    fn offset(&self) -> io::Result<u32> {
        u32::try_from(self.written).map_err(|_| too_large())
    }

    /// A whole entry at once.
    fn file(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data)?;
        let deflated = enc.finish()?;
        let entry = ZipEntry {
            name: name.into(),
            flags: 0,
            crc: crc32fast::hash(data),
            compressed: deflated.len() as u32,
            size: data.len() as u32,
            offset: self.offset()?,
        };
        self.header(&entry)?;
        self.write_all(&deflated)?;
        self.entries.push(entry);
        Ok(())
    }

    /// An entry whose sizes and CRC follow its data (`end_streamed`).
    fn start_streamed(&mut self, name: &str) -> io::Result<()> {
        let entry = ZipEntry {
            name: name.into(),
            flags: 0x08,
            crc: 0,
            compressed: 0,
            size: 0,
            offset: self.offset()?,
        };
        self.header(&entry)?;
        self.entries.push(entry);
        Ok(())
    }

    fn end_streamed(&mut self, crc: u32, compressed: u64, size: u64) -> io::Result<()> {
        let compressed = u32::try_from(compressed).map_err(|_| too_large())?;
        let size = u32::try_from(size).map_err(|_| too_large())?;
        let mut d = Vec::with_capacity(16);
        d.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
        d.extend_from_slice(&crc.to_le_bytes());
        d.extend_from_slice(&compressed.to_le_bytes());
        d.extend_from_slice(&size.to_le_bytes());
        self.write_all(&d)?;
        let entry = self.entries.last_mut().expect("started");
        entry.crc = crc;
        entry.compressed = compressed;
        entry.size = size;
        Ok(())
    }

    /// Writes the central directory and hands back the output.
    fn finish(mut self) -> io::Result<W> {
        let start = self.offset()?;
        let entries = std::mem::take(&mut self.entries);
        for e in &entries {
            let mut c = Vec::with_capacity(46 + e.name.len());
            c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            c.extend_from_slice(&20u16.to_le_bytes()); // made by
            c.extend_from_slice(&20u16.to_le_bytes()); // needed
            c.extend_from_slice(&e.flags.to_le_bytes());
            c.extend_from_slice(&8u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&DOS_DATE.to_le_bytes());
            c.extend_from_slice(&e.crc.to_le_bytes());
            c.extend_from_slice(&e.compressed.to_le_bytes());
            c.extend_from_slice(&e.size.to_le_bytes());
            c.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
            c.extend_from_slice(&e.offset.to_le_bytes());
            c.extend_from_slice(e.name.as_bytes());
            self.write_all(&c)?;
        }
        let size = self.offset()? - start;
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&[0; 4]); // disk numbers
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&size.to_le_bytes());
        end.extend_from_slice(&start.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.write_all(&end)?;
        Ok(self.out)
    }
}

/// Kinds for `columns`, by name from the schema tree.
fn kinds_for(columns: &[String], types: &[ColumnInfo]) -> Vec<CellKind> {
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
fn write_file<T>(path: &Path, f: impl FnOnce(BufWriter<std::fs::File>) -> Result<T>) -> Result<T> {
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
mod tests {
    use super::*;

    fn csv(rows: &[&[Option<&str>]]) -> String {
        let mut w = RowWriter::new(
            Vec::new(),
            ExportFormat::Csv,
            "t",
            &["a".into(), "b".into()],
            vec![],
        )
        .unwrap();
        for r in rows {
            w.row(r).unwrap();
        }
        let Inner::Csv(out) = w.inner else {
            unreachable!()
        };
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn csv_quotes_only_what_needs_it() {
        assert_eq!(
            csv(&[
                &[Some("plain"), None],
                &[Some("a,b"), Some("say \"hi\"\nok")]
            ]),
            "a,b\r\nplain,\r\n\"a,b\",\"say \"\"hi\"\"\nok\"\r\n"
        );
        assert_eq!(csv(&[&[Some(" pad"), Some("")]]), "a,b\r\n\" pad\",\r\n");
    }

    #[test]
    fn column_letters_roll_over() {
        assert_eq!(column_letters(0), "A");
        assert_eq!(column_letters(25), "Z");
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_letters(702), "AAA");
    }

    #[test]
    fn cell_kinds_from_types() {
        assert_eq!(cell_kind("integer"), CellKind::Number);
        assert_eq!(cell_kind("numeric(10,2)"), CellKind::Number);
        assert_eq!(cell_kind("double precision"), CellKind::Number);
        assert_eq!(cell_kind("integer[]"), CellKind::Text);
        assert_eq!(cell_kind("boolean"), CellKind::Bool);
        assert_eq!(cell_kind("timestamp with time zone"), CellKind::Text);
    }

    #[test]
    fn numbers_excel_would_round_stay_text() {
        assert_eq!(excel_number("42"), Some("42"));
        assert_eq!(excel_number("-1.5e3"), Some("-1.5e3"));
        assert_eq!(excel_number("NaN"), None);
        assert_eq!(excel_number("12345678901234567"), None);
    }

    #[test]
    fn xml_text_is_escaped_and_cleaned() {
        let mut out = String::new();
        xml_escape("a<b & \"c\"\u{1}\n", &mut out);
        assert_eq!(out, "a&lt;b &amp; &quot;c&quot;\n");
        assert_eq!(sheet_name("a/b[c]"), "abc");
        assert_eq!(sheet_name(""), "Sheet1");
    }

    /// The zip's central directory points at every entry's local header.
    #[test]
    fn xlsx_is_a_well_formed_zip() {
        let mut w = RowWriter::new(
            Vec::new(),
            ExportFormat::Xlsx,
            "people",
            &["id".into(), "name".into(), "ok".into()],
            vec![CellKind::Number, CellKind::Text, CellKind::Bool],
        )
        .unwrap();
        w.row(&[Some("1"), Some("Ann & Bo"), Some("t")]).unwrap();
        w.row(&[Some("2"), None, Some("f")]).unwrap();
        let Inner::Xlsx(x) = w.inner else {
            unreachable!()
        };
        let bytes = x.finish().unwrap();
        let le32 = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let le16 = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
        let end = bytes.len() - 22;
        assert_eq!(le32(end), 0x0605_4b50);
        assert_eq!(le16(end + 10), 6);
        let mut at = le32(end + 16) as usize;
        let mut names = Vec::new();
        for _ in 0..6 {
            assert_eq!(le32(at), 0x0201_4b50);
            let name_len = le16(at + 28);
            let offset = le32(at + 42) as usize;
            assert_eq!(le32(offset), 0x0403_4b50);
            let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
            if name.ends_with("sheet1.xml") {
                let data = offset + 30 + name_len;
                let compressed = le32(at + 20) as usize;
                let mut xml = String::new();
                std::io::Read::read_to_string(
                    &mut flate2::read::DeflateDecoder::new(&bytes[data..data + compressed]),
                    &mut xml,
                )
                .unwrap();
                assert_eq!(crc32fast::hash(xml.as_bytes()), le32(at + 16));
                assert!(xml.contains(r#"<c r="A2"><v>1</v></c>"#));
                assert!(xml.contains("Ann &amp; Bo"));
                assert!(xml.contains(r#"<c r="C3" t="b"><v>0</v></c>"#));
                assert!(!xml.contains(r#"r="B3""#));
            }
            names.push(name);
            at += 46 + name_len;
        }
        assert!(names.contains(&"[Content_Types].xml".to_string()));
    }

    #[test]
    fn format_comes_from_the_file_name() {
        assert_eq!(
            ExportFormat::for_path(Path::new("/x/leads.CSV")).unwrap(),
            ExportFormat::Csv
        );
        assert_eq!(
            ExportFormat::for_path(Path::new("leads.xlsx")).unwrap(),
            ExportFormat::Xlsx
        );
        let e = ExportFormat::for_path(Path::new("leads.json")).unwrap_err();
        assert!(e.to_string().contains(".csv or .xlsx"), "{e}");
    }
}
