//! The Excel workbook: one sheet of inline strings, deflated into the zip as rows come in.

use std::io::{self, Write};

use flate2::Compression;
use flate2::write::DeflateEncoder;

use super::CellKind;
use super::zip::Zip;

/// Excel's longest cell text.
const XLSX_MAX_CELL_CHARS: usize = 32_767;

/// Column letters: 0 is A, 25 is Z, 26 is AA.
pub(super) fn column_letters(mut ix: usize) -> String {
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
pub(super) fn xml_escape(value: &str, out: &mut String) {
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
pub(super) fn excel_number(value: &str) -> Option<&str> {
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
pub(super) fn sheet_name(name: &str) -> String {
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
pub(super) struct Xlsx<W: Write> {
    sheet: DeflateEncoder<Zip<W>>,
    crc: crc32fast::Hasher,
    size: u64,
    /// Where the sheet's compressed data starts.
    data_start: u64,
    line: String,
}

impl<W: Write> Xlsx<W> {
    pub(super) fn new(out: W, sheet: &str) -> io::Result<Self> {
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
    pub(super) fn row(
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

    pub(super) fn finish(mut self) -> io::Result<W> {
        self.put(SHEET_END)?;
        let mut zip = self.sheet.finish()?;
        let compressed = zip.written - self.data_start;
        zip.end_streamed(self.crc.finalize(), compressed, self.size)?;
        zip.finish()
    }
}
