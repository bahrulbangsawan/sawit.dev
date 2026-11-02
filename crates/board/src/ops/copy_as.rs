//! What Edit ▸ Copy as copies of the selection or the page: its text, CSS, SVG or PNG.

use figcraft_doc::{ExportSetting, NodeId};
use figcraft_engine::cmd::file::export_area;
use figcraft_engine::codegen::{self, CodeLang};
use figcraft_engine::guard;

use super::elements::page_layers;
use super::export::svg_text;
use crate::design::export::{ExportRow, export_bytes};
use crate::doc::{BoardDoc, BoardError};
use crate::summary;

/// The longest side, in pixels, of the image Copy as PNG puts on the clipboard
/// ([`copy_png_scale`]).
pub const COPY_PNG_MAX_SIDE: f64 = 4096.0;
/// The scale Copy as PNG wants, as FigCraft's own app copies at, before [`copy_png_scale`] caps it.
const COPY_PNG_SCALE: f64 = 2.0;
/// The least scale `file.export` takes: it raises any less to it, which would make a PNG longer
/// than [`COPY_PNG_MAX_SIDE`] (ruling P6-C14).
const LEAST_EXPORT_SCALE: f64 = 0.01;

/// What Edit ▸ Copy as puts on the system clipboard ([`copy_as`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Copied {
    /// The layers' text, CSS or SVG.
    Text(String),
    /// A PNG file.
    Png(Vec<u8>),
}

/// The scale Copy as PNG exports layers at whose export area is `bounds` (its width and height on
/// the page): `wanted`, or less, for an area that would be more than [`COPY_PNG_MAX_SIDE`] pixels
/// long at it, so that its longest side is that long.
pub fn copy_png_scale(bounds: (f64, f64), wanted: f64) -> f64 {
    let longest = bounds.0.max(bounds.1);
    if longest * wanted > COPY_PNG_MAX_SIDE {
        COPY_PNG_MAX_SIDE / longest
    } else {
        wanted
    }
}

/// What Edit ▸ Copy as `id` copies: `edit.copyAsText`, `edit.copyAsCss`, `edit.copyAsSvg` or
/// `edit.copyAsPng`, as FigCraft's own app copies it.
///
/// - Text: the text of each selected layer, a line each: a text layer's, a sticky's or a shape's,
///   a connector's label's, or the text layers' inside a frame or a group.
/// - CSS: the first selected layer's, as Dev Mode writes it.
/// - SVG: the selected layers together, as one SVG file.
/// - PNG: the selected layers together, as one PNG at 2×, or at the scale that makes its longest
///   side [`COPY_PNG_MAX_SIDE`] pixels long when 2× would make it longer ([`copy_png_scale`]). An
///   area too long even at the least scale the engine exports at is refused.
///
/// With nothing selected, SVG and PNG copy the page's top-level layers, as File ▸ Export exports
/// them; text and CSS have nothing to copy. A selection without text has no text to copy. That is
/// the error, as is any other `id`. Nothing in the document changes.
pub fn copy_as(board: &mut BoardDoc, id: &str) -> Result<Copied, BoardError> {
    let selection = board.session.selection();
    match id {
        "edit.copyAsText" => text_of(board, &selection).map(Copied::Text),
        "edit.copyAsCss" => css_of(board, &selection).map(Copied::Text),
        "edit.copyAsSvg" | "edit.copyAsPng" => {
            let (ids, what) = if selection.is_empty() {
                (page_layers(board)?, "page")
            } else {
                (selection, "selection")
            };
            if id == "edit.copyAsSvg" {
                svg_of(board, &ids).map(Copied::Text)
            } else {
                png_of(board, &ids, what).map(Copied::Png)
            }
        }
        _ => Err(BoardError::Engine(format!("`{id}` is no Copy as command"))),
    }
}

/// The error of a Copy as that needs a selection, with nothing selected: it doesn't apply then,
/// as the engine's commands on the selection don't ([`BoardError::Unavailable`]).
fn nothing_selected() -> BoardError {
    BoardError::Unavailable {
        command: "edit.copyAs".to_string(),
        reason: "nothing is selected".to_string(),
    }
}

/// The text of layers `ids`, a line each, for Copy as text ([`copy_as`]). Each text is copied
/// once, however many of the layers hold it, and an empty one not at all.
fn text_of(board: &BoardDoc, ids: &[NodeId]) -> Result<String, BoardError> {
    if ids.is_empty() {
        return Err(nothing_selected());
    }
    let doc = board.doc()?;
    let is_text = |layer: &NodeId| {
        doc.node(*layer)
            .is_some_and(|node| node.kind.text().is_some())
    };
    let mut texts: Vec<NodeId> = Vec::new();
    for &id in ids {
        let held = match summary::text_layer(&doc, id) {
            Some(text) => vec![text],
            None => doc.subtree(id).into_iter().filter(is_text).collect(),
        };
        for text in held {
            if !texts.contains(&text) {
                texts.push(text);
            }
        }
    }
    let lines: Vec<&str> = (texts.iter())
        .filter_map(|text| Some(doc.node(*text)?.kind.text()?.characters.as_str()))
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(BoardError::Engine("the selection has no text".to_string()));
    }
    Ok(lines.join("\n"))
}

/// The CSS of the first of layers `ids`, as Dev Mode writes it, for Copy as CSS ([`copy_as`]).
fn css_of(board: &BoardDoc, ids: &[NodeId]) -> Result<String, BoardError> {
    let first = *ids.first().ok_or_else(nothing_selected)?;
    let doc = board.doc()?;
    Ok(guard::run("ui.copyAsCss", || {
        Ok(codegen::code_for(&doc, first, CodeLang::Css))
    })?)
}

/// Layers `ids` together as one SVG file, for Copy as SVG ([`copy_as`]).
fn svg_of(board: &mut BoardDoc, ids: &[NodeId]) -> Result<String, BoardError> {
    let row = ExportRow {
        format: "svg".to_string(),
        scale: 1.0,
    };
    svg_text(export_bytes(board, ids, &row)?)
}

/// Layers `ids` together as one PNG, for Copy as PNG ([`copy_as`]): at 2×, or smaller, so that its
/// longest side is at most [`COPY_PNG_MAX_SIDE`] pixels. `what` they are (the selection, the page)
/// names them when they are too long for that.
fn png_of(board: &mut BoardDoc, ids: &[NodeId], what: &str) -> Result<Vec<u8>, BoardError> {
    let doc = board.doc()?;
    let setting = ExportSetting::default();
    let area = guard::run("ui.copyAsPng", || Ok(export_area(&doc, ids, &setting)))?;
    // Without an area the layers paint nothing, and the export says so.
    let scale = area.map_or(COPY_PNG_SCALE, |area| {
        copy_png_scale((area.width(), area.height()), COPY_PNG_SCALE)
    });
    if scale < LEAST_EXPORT_SCALE {
        return Err(BoardError::Engine(format!(
            "The {what} is too large to copy as PNG"
        )));
    }
    let row = ExportRow {
        format: "png".to_string(),
        scale,
    };
    export_bytes(board, ids, &row)
}
