//! The Export section: a layer's export settings as rows of a format and a scale, and the bytes
//! of one row.
//!
//! The settings are the layer's `exports` list: one `ExportSetting` of the engine per file the
//! layer exports, which also holds a suffix for the file name and options for SVG, PDF and Android
//! drawables. A row keeps the two things the section edits, the format and the scale.
//! [`set_export_rows`] writes the whole list, and keeps the setting of every row it is given back
//! unchanged, so the rows the section leaves alone keep their suffix, their size by width and
//! their options.
//!
//! The options need a step of their own. `node.set` of `exports` does not take a setting back
//! whole: at the pinned FigCraft revision it reads the list through a conversion of the keys to
//! snake_case that the settings' camelCase names don't match, so every option named with two words
//! goes back to its default when the list is written: `contentsOnly`, `includeBoundingBox`,
//! `svgOutlineText`, `svgIdAttribute`, `svgSimplifyStroke`, `vdCompat`, and `cropMarks` and
//! `bleedMarks` of `print`. The suffix, the size, the format and the other print options survive.
//! `export.update`, which changes a setting in place, takes every option, so a row that keeps its
//! setting gets its options back that way.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_doc::{ExportFormat, ExportSetting, NodeId};
use serde_json::{Map, Value, json};

use super::{DesignSelection, Edit, Mixed};
use crate::doc::{BoardDoc, BoardError};

/// The options of an export setting that `node.set` of the `exports` list drops: the ones named
/// with two words, which the engine reads back wrongly.
const DROPPED_OPTIONS: [&str; 6] = [
    "contentsOnly",
    "includeBoundingBox",
    "svgOutlineText",
    "svgIdAttribute",
    "svgSimplifyStroke",
    "vdCompat",
];

/// The same for the print settings of a PDF, which the setting holds as `print`.
const DROPPED_PRINT_OPTIONS: [&str; 2] = ["cropMarks", "bleedMarks"];

/// One row of the Export section: a format at a scale.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportRow {
    /// The format by its file extension: `png`, `jpg`, `svg`, `pdf`, `webp`, or `xml` for an
    /// Android Vector Drawable. It is also the extension of the file the row exports to.
    pub format: String,
    /// How many times the size of the layer the export is. Only the pixel formats (`png`, `jpg`
    /// and `webp`) have a size to scale: the others are one file whatever the scale. The engine
    /// keeps a scale between 0.01 and 64.
    pub scale: f64,
}

impl Default for ExportRow {
    /// A PNG at 1×: the row of a layer that has no export settings.
    fn default() -> Self {
        Self {
            format: "png".to_string(),
            scale: 1.0,
        }
    }
}

/// The rows the section shows for the selection, in the order of the layers' settings.
///
/// - Layers that share the same settings show one row per setting.
/// - Layers with no setting show the default row, a PNG at 1×. It stands for what exporting such a
///   layer does, and is not in the file until the section writes it.
/// - A setting sized by width or height reads as the scale that gives the layer's box that size
///   (the first layer's box, when several are selected). The engine measures what the layer paints
///   instead, which a shadow or an outside stroke makes larger, so the pixels can differ a little.
/// - Layers with different settings have no rows in common, so the list is empty and the section
///   shows `get("exports")` as mixed. Such layers are exported one by one, with the rows of each.
///   A selection of no layers has no rows either.
pub fn export_rows(sel: &DesignSelection) -> Vec<ExportRow> {
    match sel.get("exports") {
        Mixed::Same(Value::Array(settings)) if settings.is_empty() => vec![ExportRow::default()],
        Mixed::Same(Value::Array(settings)) => {
            let size = first_size(sel);
            let row = |setting| row_of(setting, size);
            settings.iter().filter_map(row).collect()
        }
        _ => Vec::new(),
    }
}

/// The edit that gives every selected layer exactly `rows` as its export settings: `node.set` of
/// the `exports` list. No rows leave a layer with no setting, which the section shows as the
/// default row.
///
/// A row that is one of the settings the layers share, as [`export_rows`] reads it, keeps that
/// setting as it was: its suffix, a size by width or height, and its options. Every other row is a
/// new setting of its format and scale, with the engine's defaults for the rest. So adding a row
/// or removing one leaves the rest as they were, and changing a row makes it new. A row with a
/// format that is not one, or a scale that is not a number, is refused by the engine and nothing
/// changes.
///
/// When a kept setting has options that `node.set` would drop (see the module doc), the edit is a
/// `command.batch`, one undo step, that gives them back with an `export.update` for each such row.
pub fn set_export_rows(sel: &DesignSelection, rows: &[ExportRow]) -> Edit {
    let shared = sel.get("exports");
    let mut kept: Vec<&Value> = match &shared {
        Mixed::Same(Value::Array(settings)) => settings.iter().collect(),
        _ => Vec::new(),
    };
    let size = first_size(sel);
    // The options to give back, with the place of their row in the new list.
    let mut options = Vec::new();
    let settings = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let same = kept
                .iter()
                .position(|setting| row_of(setting, size).as_ref() == Some(row));
            match same {
                Some(at) => {
                    let setting = kept.remove(at);
                    options.extend(dropped_options(setting).map(|given| (index, given)));
                    setting.clone()
                }
                None => new_setting(row),
            }
        })
        .collect();
    let set = sel.set("exports", Value::Array(settings));
    if options.is_empty() {
        return set;
    }
    let mut commands = vec![json!({"command": set.command, "params": set.params})];
    for (index, mut update) in options {
        update.insert("ids".to_string(), json!(sel.ids));
        update.insert("index".to_string(), json!(index));
        commands.push(json!({"command": "export.update", "params": update}));
    }
    Edit {
        command: "command.batch",
        params: json!({"label": "Set properties", "commands": commands}),
    }
}

/// The bytes of exporting the layers `ids` as `row` says: `file.export` with `ids`, the format and
/// the scale, which answers with the file as base64.
///
/// Nothing in the file changes, and the layers are drawn together into one file. It fails when
/// there are no layers, when the format is none of `png`, `jpg`, `svg`, `pdf`, `webp` and `xml`,
/// and when the engine has nothing to export: a layer that isn't in the file, or paints nothing.
pub fn export_bytes(
    board: &mut BoardDoc,
    ids: &[NodeId],
    row: &ExportRow,
) -> Result<Vec<u8>, BoardError> {
    // `file.export` exports the whole page for no layers, and a whole file for `fig` and
    // `penpot`, and a row asks for neither.
    if ids.is_empty() {
        return Err(BoardError::Engine("nothing to export".to_string()));
    }
    let format = ExportFormat::from_ext(&row.format).ok_or_else(|| {
        BoardError::Engine(format!(
            "`{}` is not an export format: png, jpg, svg, pdf, webp or xml",
            row.format
        ))
    })?;
    let params = json!({"ids": ids, "format": format.ext(), "scale": row.scale});
    exported_file(&board.execute("file.export", &params)?)
}

/// The file an export command answered with (`file.export`, `file.exportFramesPdf`): its base64
/// `data`, decoded.
pub(crate) fn exported_file(reply: &Value) -> Result<Vec<u8>, BoardError> {
    let data = reply
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| BoardError::Engine("the export gave no data".to_string()))?;
    STANDARD
        .decode(data)
        .map_err(|e| BoardError::Engine(format!("the export gave data that is not base64: {e}")))
}

/// The row an export setting of a layer reads as, for a layer whose box is `size` (width, height).
fn row_of(setting: &Value, (width, height): (f64, f64)) -> Option<ExportRow> {
    let setting: ExportSetting = serde_json::from_value(setting.clone()).ok()?;
    Some(ExportRow {
        format: setting.format.ext().to_string(),
        scale: setting.size.scale_for(width, height),
    })
}

/// A new setting of `row`'s format and scale, as `node.set` takes it. The engine fills in the rest.
fn new_setting(row: &ExportRow) -> Value {
    json!({"format": format_name(&row.format), "size": {"by": "scale", "value": row.scale}})
}

/// The name a format has in the `exports` list: its extension for most, `vectorDrawable` for `xml`.
/// What is no format stays as it is, and the engine refuses the list.
fn format_name(ext: &str) -> Value {
    ExportFormat::from_ext(ext)
        .and_then(|format| serde_json::to_value(format).ok())
        .unwrap_or_else(|| json!(ext))
}

/// The options of export setting `setting` that `node.set` drops, as the params `export.update`
/// takes them. The inspector gives an option only while it is not at its default, so each one found
/// is one to give back; none found, `None`.
fn dropped_options(setting: &Value) -> Option<Map<String, Value>> {
    let mut options = Map::new();
    for key in DROPPED_OPTIONS {
        if let Some(value) = setting.get(key) {
            options.insert(key.to_string(), value.clone());
        }
    }
    let print = setting.get("print");
    for key in DROPPED_PRINT_OPTIONS {
        if let Some(value) = print.and_then(|print| print.get(key)) {
            options.insert(key.to_string(), value.clone());
        }
    }
    (!options.is_empty()).then_some(options)
}

/// The width and height of the first selected layer, which a size by width or height is measured
/// against; 0 × 0 when nothing is selected.
fn first_size(sel: &DesignSelection) -> (f64, f64) {
    let Some(first) = sel.layers.first() else {
        return (0.0, 0.0);
    };
    let side = |key: &str| first.props.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    (side("width"), side("height"))
}

#[cfg(test)]
mod tests;
