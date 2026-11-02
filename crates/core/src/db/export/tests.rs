//! CSV quoting, the workbook's cells and zip, and the format a file name asks for.

use super::xlsx::{column_letters, excel_number, sheet_name, xml_escape};
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
