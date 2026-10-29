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
    let reply = board.execute("file.export", &params)?;
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
mod tests {
    use serde_json::json;

    use super::*;
    use crate::design::Mixed;
    use crate::design::testing::{
        apply, design_file, inspect, make, node_set, read, run, undo_steps,
    };

    fn row(format: &str, scale: f64) -> ExportRow {
        ExportRow {
            format: format.into(),
            scale,
        }
    }

    /// A 100 × 60 rectangle.
    fn rectangle(board: &mut BoardDoc) -> NodeId {
        make(
            board,
            json!({"type": "rectangle", "name": "Card", "width": 100, "height": 60}),
        )
    }

    /// The width in pixels a PNG says it has.
    fn png_width(bytes: &[u8]) -> u32 {
        u32::from_be_bytes(bytes[16..20].try_into().unwrap())
    }

    #[test]
    fn export_defaults_to_png_1x() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let sel = read(&board, &[id]);
        assert_eq!(sel.get("exports"), Mixed::Same(json!([])));
        assert_eq!(export_rows(&sel), [row("png", 1.0)]);
        assert_eq!(ExportRow::default(), row("png", 1.0));
    }

    #[test]
    fn export_bytes_png_has_signature() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let bytes = export_bytes(&mut board, &[id], &row("png", 1.0)).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"), "{:02x?}", &bytes[..8]);
    }

    #[test]
    fn the_scale_sets_the_size_of_the_pixels() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let once = export_bytes(&mut board, &[id], &row("png", 1.0)).unwrap();
        let twice = export_bytes(&mut board, &[id], &row("png", 2.0)).unwrap();
        assert_eq!(png_width(&once), 100);
        assert_eq!(png_width(&twice), 200);
    }

    #[test]
    fn every_format_a_row_names_exports() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let starts: [(&str, &[u8]); 5] = [
            ("png", b"\x89PNG"),
            ("jpg", b"\xff\xd8\xff"),
            ("pdf", b"%PDF"),
            ("webp", b"RIFF"),
            ("svg", b"<"),
        ];
        for (format, start) in starts {
            let bytes = export_bytes(&mut board, &[id], &row(format, 1.0)).unwrap();
            assert!(bytes.starts_with(start), "{format}: {:?}", &bytes[..8]);
        }
        let svg = export_bytes(&mut board, &[id], &row("svg", 1.0)).unwrap();
        assert!(String::from_utf8(svg).unwrap().contains("<svg"));
        let xml = export_bytes(&mut board, &[id], &row("xml", 1.0)).unwrap();
        assert!(String::from_utf8(xml).unwrap().contains("<vector"));
    }

    #[test]
    fn exporting_changes_nothing_in_the_file() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let steps = undo_steps(&board);
        let layer = inspect(&board, id);
        export_bytes(&mut board, &[id], &row("png", 2.0)).unwrap();
        assert_eq!(undo_steps(&board), steps);
        assert_eq!(inspect(&board, id), layer);
    }

    /// `file.export` without layers exports the page, and `fig` and `penpot` export the whole
    /// file: a row asks for neither.
    #[test]
    fn export_bytes_refuses_no_layers_unknown_formats_and_missing_layers() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let none = export_bytes(&mut board, &[], &row("png", 1.0));
        assert!(matches!(none, Err(BoardError::Engine(_))), "{none:?}");
        for format in ["fig", "penpot", "gif", ""] {
            let refused = export_bytes(&mut board, &[id], &row(format, 1.0));
            assert!(
                matches!(refused, Err(BoardError::Engine(_))),
                "{format:?}: {refused:?}"
            );
        }
        let missing = export_bytes(&mut board, &[NodeId(9_999)], &row("png", 1.0));
        assert!(matches!(missing, Err(BoardError::Engine(_))), "{missing:?}");
    }

    #[test]
    fn rows_set_are_the_rows_read() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let rows = [row("png", 2.0), row("svg", 1.0), row("xml", 1.0)];
        let steps = undo_steps(&board);

        let edit = set_export_rows(&read(&board, &[id]), &rows);
        assert_eq!(edit.command, "node.set");
        apply(&mut board, &edit);

        assert_eq!(undo_steps(&board), steps + 1);
        assert_eq!(export_rows(&read(&board, &[id])), rows);
        // The engine holds them as it names them.
        let exports = inspect(&board, id)["exports"].clone();
        assert_eq!(exports[0]["format"], json!("png"));
        assert_eq!(exports[0]["size"], json!({"by": "scale", "value": 2.0}));
        assert_eq!(exports[2]["format"], json!("vectorDrawable"));
    }

    #[test]
    fn no_rows_clear_the_settings_and_the_default_row_shows_again() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let rows = [row("jpg", 3.0)];
        run(&mut board, &[id], |sel| set_export_rows(sel, &rows));
        assert_eq!(export_rows(&read(&board, &[id])), rows);

        run(&mut board, &[id], |sel| set_export_rows(sel, &[]));
        assert_eq!(read(&board, &[id]).get("exports"), Mixed::Same(json!([])));
        assert_eq!(export_rows(&read(&board, &[id])), [ExportRow::default()]);
    }

    /// A size by width or height is the scale that gives the layer's box that size.
    #[test]
    fn a_size_by_width_or_height_reads_as_the_scale_that_gives_it() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        node_set(
            &mut board,
            &[id],
            json!({"exports": [
                {"format": "png", "size": {"by": "width", "value": 300}},
                {"format": "jpg", "size": {"by": "height", "value": 30}},
                {"format": "png", "size": {"by": "scale", "value": 4}},
            ]}),
        );
        assert_eq!(
            export_rows(&read(&board, &[id])),
            [row("png", 3.0), row("jpg", 0.5), row("png", 4.0)]
        );
    }

    #[test]
    fn a_row_left_alone_keeps_its_setting_and_a_changed_one_is_new() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        node_set(
            &mut board,
            &[id],
            json!({"exports": [
                {"format": "png", "size": {"by": "scale", "value": 2}, "suffix": "@retina"},
                {"format": "jpg", "size": {"by": "width", "value": 300}, "suffix": "-wide"},
            ]}),
        );

        // Add a row: the two that were there are as they were, even the one sized by width.
        let sel = read(&board, &[id]);
        let mut rows = export_rows(&sel);
        rows.push(row("svg", 1.0));
        apply(&mut board, &set_export_rows(&sel, &rows));
        let exports = inspect(&board, id)["exports"].clone();
        assert_eq!(exports[0]["suffix"], json!("@retina"));
        assert_eq!(exports[1]["suffix"], json!("-wide"));
        assert_eq!(exports[1]["size"], json!({"by": "width", "value": 300.0}));
        assert_eq!(exports[2]["format"], json!("svg"));

        // Change a row: that one is a new setting, from its format and scale alone.
        let sel = read(&board, &[id]);
        let mut rows = export_rows(&sel);
        rows[0].scale = 3.0;
        apply(&mut board, &set_export_rows(&sel, &rows));
        let exports = inspect(&board, id)["exports"].clone();
        assert_eq!(exports[0]["suffix"], json!(""));
        assert_eq!(exports[0]["size"], json!({"by": "scale", "value": 3.0}));
        assert_eq!(exports[1]["suffix"], json!("-wide"), "the others still are");
    }

    #[test]
    fn removing_a_row_keeps_the_settings_of_the_rows_left() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        node_set(
            &mut board,
            &[id],
            json!({"exports": [
                {"format": "png", "size": {"by": "scale", "value": 1}, "suffix": "-a"},
                {"format": "png", "size": {"by": "scale", "value": 2}, "suffix": "-b"},
                {"format": "png", "size": {"by": "scale", "value": 3}, "suffix": "-c"},
            ]}),
        );
        let sel = read(&board, &[id]);
        let mut rows = export_rows(&sel);
        rows.remove(0);
        apply(&mut board, &set_export_rows(&sel, &rows));

        let exports = inspect(&board, id)["exports"].clone();
        assert_eq!(exports.as_array().unwrap().len(), 2);
        assert_eq!(exports[0]["suffix"], json!("-b"));
        assert_eq!(exports[1]["suffix"], json!("-c"));
    }

    /// Two rows that read the same are two settings: each keeps its own suffix.
    #[test]
    fn rows_that_look_alike_each_keep_their_own_setting() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        node_set(
            &mut board,
            &[id],
            json!({"exports": [
                {"format": "png", "size": {"by": "scale", "value": 2}, "suffix": "-a"},
                {"format": "png", "size": {"by": "scale", "value": 2}, "suffix": "-b"},
            ]}),
        );
        let sel = read(&board, &[id]);
        let mut rows = export_rows(&sel);
        assert_eq!(rows, [row("png", 2.0), row("png", 2.0)]);
        rows.push(row("png", 3.0));
        apply(&mut board, &set_export_rows(&sel, &rows));

        let exports = inspect(&board, id)["exports"].clone();
        assert_eq!(exports[0]["suffix"], json!("-a"));
        assert_eq!(exports[1]["suffix"], json!("-b"));
        assert_eq!(exports[2]["size"], json!({"by": "scale", "value": 3.0}));
    }

    #[test]
    fn layers_with_different_settings_have_no_rows_to_show_in_common() {
        let mut board = design_file();
        let a = rectangle(&mut board);
        let b = rectangle(&mut board);
        node_set(
            &mut board,
            &[a],
            json!({"exports": [{"format": "svg", "size": {"by": "scale", "value": 1}}]}),
        );
        let sel = read(&board, &[a, b]);
        assert_eq!(sel.get("exports"), Mixed::Mixed);
        assert!(export_rows(&sel).is_empty());

        // Writing rows over them gives every layer the same ones.
        let rows = [row("png", 2.0)];
        apply(&mut board, &set_export_rows(&sel, &rows));
        let sel = read(&board, &[a, b]);
        assert_eq!(export_rows(&sel), rows);
    }

    #[test]
    fn layers_with_the_same_settings_show_them() {
        let mut board = design_file();
        let a = rectangle(&mut board);
        let b = rectangle(&mut board);
        let rows = [row("png", 2.0), row("pdf", 1.0)];
        run(&mut board, &[a, b], |sel| set_export_rows(sel, &rows));
        assert_eq!(export_rows(&read(&board, &[a, b])), rows);
        assert_eq!(export_rows(&read(&board, &[a])), rows);
        assert_eq!(export_rows(&read(&board, &[b])), rows);
    }

    /// `export.add` makes a setting with the options of an SVG, PDF or Android export set, which
    /// `node.set` of the list cannot.
    fn add_with_options(board: &mut BoardDoc, id: NodeId, setting: serde_json::Value) {
        board
            .execute("export.add", &json!({"ids": [id], "setting": setting}))
            .unwrap();
    }

    /// The rows left alone keep every option of their setting, though `node.set` of the list drops
    /// the ones with two-word names: the edit gives them back with `export.update`, in one step.
    #[test]
    fn a_row_left_alone_keeps_its_options_as_well() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        add_with_options(
            &mut board,
            id,
            json!({"format": "svg", "contentsOnly": false, "svgOutlineText": false, "svgIdAttribute": true}),
        );
        add_with_options(
            &mut board,
            id,
            json!({"format": "pdf", "bleed": 3, "cropMarks": true, "includeBoundingBox": true}),
        );
        let before = inspect(&board, id)["exports"].clone();
        assert_eq!(before[0]["contentsOnly"], json!(false), "{before}");

        // Add a row: the two that were there are as they were, options and all.
        let sel = read(&board, &[id]);
        let mut rows = export_rows(&sel);
        rows.push(row("png", 2.0));
        let steps = undo_steps(&board);
        let edit = set_export_rows(&sel, &rows);
        assert_eq!(edit.command, "command.batch");
        apply(&mut board, &edit);

        assert_eq!(undo_steps(&board), steps + 1);
        let after = inspect(&board, id)["exports"].clone();
        assert_eq!(after[0], before[0]);
        assert_eq!(after[1], before[1]);
        assert_eq!(after[2]["format"], json!("png"));

        // Remove the first row: the second is the first now, still with its options.
        let sel = read(&board, &[id]);
        let mut rows = export_rows(&sel);
        rows.remove(0);
        apply(&mut board, &set_export_rows(&sel, &rows));
        let after = inspect(&board, id)["exports"].clone();
        assert_eq!(after.as_array().unwrap().len(), 2);
        assert_eq!(after[0], before[1]);
        assert_eq!(after[1]["format"], json!("png"));
    }

    /// A changed row is a new setting, and has the engine's defaults for the options.
    #[test]
    fn a_changed_row_has_the_default_options() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        add_with_options(
            &mut board,
            id,
            json!({"format": "svg", "svgOutlineText": false}),
        );
        let sel = read(&board, &[id]);
        let mut rows = export_rows(&sel);
        rows[0].format = "pdf".to_string();
        apply(&mut board, &set_export_rows(&sel, &rows));
        let after = inspect(&board, id)["exports"].clone();
        assert_eq!(after[0]["format"], json!("pdf"));
        assert!(after[0].get("svgOutlineText").is_none(), "{after}");
    }

    /// Layers with the same settings and options keep them all.
    #[test]
    fn layers_that_share_a_setting_with_options_all_keep_it() {
        let mut board = design_file();
        let a = rectangle(&mut board);
        let b = rectangle(&mut board);
        for id in [a, b] {
            add_with_options(
                &mut board,
                id,
                json!({"format": "svg", "svgOutlineText": false}),
            );
        }
        let sel = read(&board, &[a, b]);
        assert!(
            matches!(sel.get("exports"), Mixed::Same(_)),
            "layers share it"
        );
        let mut rows = export_rows(&sel);
        rows.push(row("png", 1.0));
        apply(&mut board, &set_export_rows(&sel, &rows));
        for id in [a, b] {
            let after = inspect(&board, id)["exports"].clone();
            assert_eq!(after[0]["svgOutlineText"], json!(false), "{after}");
            assert_eq!(after[1]["format"], json!("png"));
        }
    }

    /// What the module's doc says `node.set` does to the options of a setting. If this fails,
    /// FigCraft takes them now: the caveat in the module doc goes, and so does this test.
    #[test]
    fn a_write_puts_the_two_word_options_of_a_setting_back_to_their_defaults() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        node_set(
            &mut board,
            &[id],
            json!({"exports": [{
                "format": "pdf", "suffix": "-x", "size": {"by": "scale", "value": 2},
                "print": {"bleed": 3, "cmyk": true, "cropMarks": true},
                "contentsOnly": false, "includeBoundingBox": true,
            }]}),
        );
        let setting = inspect(&board, id)["exports"][0].clone();
        // What a write keeps.
        assert_eq!(setting["suffix"], json!("-x"));
        assert_eq!(setting["print"]["bleed"], json!(3.0));
        assert_eq!(setting["print"]["cmyk"], json!(true));
        // What it does not: the inspector leaves out the options that hold their defaults.
        assert!(setting.get("contentsOnly").is_none(), "{setting}");
        assert!(setting.get("includeBoundingBox").is_none(), "{setting}");
        assert_eq!(setting["print"]["cropMarks"], json!(false));
    }

    #[test]
    fn nothing_selected_has_no_rows() {
        let board = design_file();
        assert!(export_rows(&read(&board, &[])).is_empty());
    }

    #[test]
    fn a_row_the_engine_cannot_take_is_refused_and_changes_nothing() {
        let mut board = design_file();
        let id = rectangle(&mut board);
        let sel = read(&board, &[id]);
        let revision = board.revision();
        for rows in [[row("gif", 1.0)], [row("png", f64::NAN)]] {
            let edit = set_export_rows(&sel, &rows);
            let refused = board.execute(edit.command, &edit.params);
            assert!(matches!(refused, Err(BoardError::Engine(_))), "{refused:?}");
        }
        assert_eq!(board.revision(), revision);
        assert_eq!(read(&board, &[id]), sel);
    }
}
