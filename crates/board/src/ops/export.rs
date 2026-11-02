//! The `export_svg` and `export_png` ops, and the files File ▸ Export makes of the page.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::elements::page_layers;
use super::failure;
use crate::design::export::{ExportRow, export_bytes, exported_file};
use crate::doc::{BoardDoc, BoardError};

/// The scale the `export_svg` and `export_png` ops export at. They take no parameters (spec,
/// "Agent wire contract"), so it is 1×.
const EXPORT_OP_SCALE: f64 = 1.0;

/// The `export_svg` op: the page on show as one SVG file, answered as text.
pub(super) fn export_svg(board: &mut BoardDoc) -> Value {
    match export_page(board, "svg", EXPORT_OP_SCALE).and_then(svg_text) {
        Ok(svg) => json!({"ok": true, "svg": svg}),
        Err(error) => failure(error),
    }
}

/// The `export_png` op: the page on show as one PNG file, answered as base64.
pub(super) fn export_png(board: &mut BoardDoc) -> Value {
    match export_page(board, "png", EXPORT_OP_SCALE) {
        Ok(png) => json!({"ok": true, "png_base64": STANDARD.encode(png)}),
        Err(error) => failure(error),
    }
}

/// The page on show as one `format` file at `scale`: its top-level layers exported together,
/// whatever is selected, as [`export_bytes`] exports layers (`format` is the file's extension:
/// `png`, `jpg`, `svg`, `pdf`, `webp` or `xml`). It is what File ▸ Export makes with nothing
/// selected.
///
/// Nothing in the document changes, its selection included. A page with nothing on it has nothing
/// to export: that is the error, as is a format that is none of those.
pub fn export_page(board: &mut BoardDoc, format: &str, scale: f64) -> Result<Vec<u8>, BoardError> {
    let layers = page_layers(board)?;
    let row = ExportRow {
        format: format.to_string(),
        scale,
    };
    export_bytes(board, &layers, &row)
}

/// The frames of the page on show as one PDF, a page for each visible top-level frame in reading
/// order: `file.exportFramesPdf`, which answers with the file as base64. Nothing in the document
/// changes. A page without a visible frame is the engine's refusal, which says so ("the page has
/// no visible frames").
pub fn frames_pdf(board: &mut BoardDoc) -> Result<Vec<u8>, BoardError> {
    exported_file(&board.execute("file.exportFramesPdf", &json!({}))?)
}

/// The SVG file `bytes` as text.
pub(super) fn svg_text(bytes: Vec<u8>) -> Result<String, BoardError> {
    String::from_utf8(bytes)
        .map_err(|_| BoardError::Engine("the SVG export isn't text".to_string()))
}
