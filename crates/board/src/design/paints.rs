//! The Fill and Stroke sections' model: a layer's paints as rows, and the edits that change them.
//!
//! `fills` and `strokes` are lists of paints. A solid paint reads as
//! `{type: "solid", color: "#RRGGBB", opacity?, blend?, visible?}`, and a gradient, an image or a
//! pattern as the engine's own paint JSON. An edit sends the whole list back through `node.set`,
//! so every row it doesn't change goes back as it was read, and a gradient or an image is never
//! rewritten as a solid: only [`replace_with_solid`] does that.
//!
//! Where a list can't be edited row by row, because the layers hold different lists, the panel
//! shows "Mixed" with a "Replace all" button. [`add_paint`] is that button: it makes one new paint
//! the only paint of every layer. The other edits have no row to change then, and are refused.
//!
//! An edit is made from the selection it is given and sends that selection's whole list, so make
//! each edit from a selection read after the last one.

use serde_json::{Map, Value, json};

use super::lists::{change_row, remove_row, replace, rows};
use super::{DesignSelection, Edit};

/// A new fill's colour: the default of a new shape in Figma.
const DEFAULT_FILL: &str = "#D9D9D9";
/// A new stroke's colour.
const DEFAULT_STROKE: &str = "#000000";

/// What a paint row paints with.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintKind {
    /// One colour. `hex` is `#RRGGBB` in upper case, and `opacity` (0 to 1) is how much of it
    /// shows: the paint's opacity times the alpha the colour carries.
    Solid {
        hex: String,
        opacity: f64,
    },
    /// A gradient, by its kind: "Linear", "Radial", "Angular" or "Diamond".
    Gradient(String),
    Image,
    /// Any other paint, a pattern for one.
    Other,
}

/// One paint of a fill or stroke list.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintRow {
    pub kind: PaintKind,
    /// The eye: whether the paint shows.
    pub visible: bool,
}

/// The paints of list `key`, `"fills"` or `"strokes"`, in the order the engine holds them: row 0
/// paints first and the last row is on top. FigCraft's own panel shows them in reverse, the top
/// row first; the `index` the edits below take is the position in this list either way. `None`
/// when the selected layers hold different lists. A layer without a fill or a stroke holds an
/// empty list, which is a list like any other: `Some` of no rows.
pub fn paints(sel: &DesignSelection, key: &str) -> Option<Vec<PaintRow>> {
    Some(rows(sel, key)?.iter().map(paint_row).collect())
}

/// The edit that gives solid row `index` of list `key` the colour `hex` and the opacity `opacity`
/// (a fraction, kept between 0 and 1), leaving its blend and its eye and every other row as they
/// are.
///
/// `None` when the input is no good and the field shows its current value again: `hex` isn't a
/// colour (see [`parse_hex`]), `opacity` isn't a finite number, there is no row `index`, the layers
/// hold different lists, or the row is not a solid. A gradient or an image is never rewritten as a
/// solid by this, whatever the colour; [`replace_with_solid`] does that when asked to.
pub fn set_solid(
    sel: &DesignSelection,
    key: &str,
    index: usize,
    hex: &str,
    opacity: f64,
) -> Option<Edit> {
    let hex = parse_hex(hex)?;
    if !opacity.is_finite() {
        return None;
    }
    let mut list = rows(sel, key)?;
    let fields = list.get_mut(index)?.as_object_mut()?;
    if fields.get("type").and_then(Value::as_str) != Some("solid") {
        return None;
    }
    fields.insert("color".into(), json!(hex));
    fields.insert("opacity".into(), json!(opacity.clamp(0.0, 1.0)));
    Some(replace(sel, key, list))
}

/// The edit that adds a solid paint after the rows of list `key`: `#D9D9D9` for the fills, the
/// default of a new shape, and `#000000` for the strokes.
///
/// When the layers hold different lists there is none to add to, and the new paint becomes the
/// only one of every layer: the "Replace all" of a mixed list.
pub fn add_paint(sel: &DesignSelection, key: &str) -> Edit {
    let mut list = rows(sel, key).unwrap_or_default();
    list.push(json!({"type": "solid", "color": default_hex(key)}));
    replace(sel, key, list)
}

/// The edit that removes row `index` of list `key`. A row that isn't there, or lists that differ
/// from layer to layer, make an edit the engine refuses and that changes nothing.
pub fn remove_paint(sel: &DesignSelection, key: &str, index: usize) -> Edit {
    remove_row(sel, key, index)
}

/// The edit that turns row `index` of list `key` into a solid, the explicit "Replace with solid"
/// of a gradient or an image row. A gradient becomes its first colour, the colour of the stop with
/// the lowest position, and an image or a pattern the default colour of [`add_paint`]. The row's
/// opacity, blend and eye stay; a solid row stays as it is. A row that isn't there, or lists that
/// differ from layer to layer, make an edit the engine refuses and that changes nothing.
pub fn replace_with_solid(sel: &DesignSelection, key: &str, index: usize) -> Edit {
    change_row(sel, key, index, |row| Some(solid_in_place_of(row, key)))
}

/// The edit that shows or hides row `index` of list `key`, whatever it paints with, and changes
/// nothing else about it. A row that isn't there, or lists that differ from layer to layer, make
/// an edit the engine refuses and that changes nothing.
pub fn set_visible(sel: &DesignSelection, key: &str, index: usize, visible: bool) -> Edit {
    change_row(sel, key, index, |row| {
        let mut fields = row.as_object()?.clone();
        fields.insert("visible".into(), json!(visible));
        Some(Value::Object(fields))
    })
}

/// The Stroke section's fields besides its paints, as the keys [`DesignSelection::get`] reads and
/// [`DesignSelection::set`] writes them under: the weight, the alignment (`inside`, `center` or
/// `outside`) and the dashes (a list of lengths; dashed is `[4, 4]` and solid is `[]`). A layer
/// without a stroke holds no weight or alignment, only empty dashes.
pub fn stroke_fields() -> &'static [&'static str] {
    &["strokeWeight", "strokeAlign", "dashes"]
}

/// `input` as a colour, in the form the hex field takes: `RRGGBB` or `RGB`, with or without the
/// `#`, in either case, with space around it ignored. The answer is the colour as `#RRGGBB` in
/// upper case. A colour that carries alpha (`#RRGGBBAA`) is no good: opacity is the field beside
/// the hex.
pub fn parse_hex(input: &str) -> Option<String> {
    parse_rgb(input).map(hex_of)
}

/// The bytes of the colour `input` spells, as [`parse_hex`] reads it.
pub(super) fn parse_rgb(input: &str) -> Option<[u8; 3]> {
    let text = input.trim();
    match channels(text.strip_prefix('#').unwrap_or(text))?.as_slice() {
        [r, g, b] => Some([*r, *g, *b]),
        _ => None,
    }
}

/// `#RRGGBB` in upper case for the bytes of a colour.
pub(super) fn hex_of([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// The byte of a colour channel held as a fraction of 1, rounded as the engine does.
pub(super) fn byte(channel: f64) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The channels of the colour that `digits` spell in the `RGB`, `RGBA`, `RRGGBB` or `RRGGBBAA`
/// form: three or four bytes. `None` for anything else, including a character that is not an
/// ASCII hex digit.
fn channels(digits: &str) -> Option<Vec<u8>> {
    let mut nibbles: Vec<u8> = digits
        .chars()
        .map(|digit| {
            digit
                .to_digit(16)
                .and_then(|nibble| u8::try_from(nibble).ok())
        })
        .collect::<Option<_>>()?;
    if matches!(nibbles.len(), 3 | 4) {
        // `#RGB` is `#RRGGBB`.
        nibbles = nibbles
            .into_iter()
            .flat_map(|nibble| [nibble, nibble])
            .collect();
    }
    if !matches!(nibbles.len(), 6 | 8) {
        return None;
    }
    Some(
        nibbles
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[high, low]| high * 16 + low)
            .collect(),
    )
}

/// The default colour of a new paint in list `key`.
fn default_hex(key: &str) -> &'static str {
    if key == "strokes" {
        DEFAULT_STROKE
    } else {
        DEFAULT_FILL
    }
}

/// What row `row` of the list shows.
fn paint_row(row: &Value) -> PaintRow {
    let kind = match row.get("type").and_then(Value::as_str) {
        Some("solid") => solid_kind(row).unwrap_or(PaintKind::Other),
        Some("gradient") => PaintKind::Gradient(gradient_label(row)),
        Some("image") => PaintKind::Image,
        _ => PaintKind::Other,
    };
    PaintRow {
        kind,
        visible: row.get("visible").and_then(Value::as_bool).unwrap_or(true),
    }
}

/// The colour and opacity of a solid row. The engine writes the colour as `#RRGGBB`, or
/// `#RRGGBBAA` while the colour carries alpha; the opacity is the paint's own on top of that.
fn solid_kind(row: &Value) -> Option<PaintKind> {
    let color = row.get("color")?.as_str()?.strip_prefix('#')?;
    let (hex, alpha) = match channels(color)?.as_slice() {
        [r, g, b] => (hex_of([*r, *g, *b]), 1.0),
        [r, g, b, a] => (hex_of([*r, *g, *b]), f64::from(*a) / 255.0),
        _ => return None,
    };
    let own = row.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    Some(PaintKind::Solid {
        hex,
        opacity: own * alpha,
    })
}

/// "Linear", "Radial", "Angular" or "Diamond" for a gradient row.
fn gradient_label(row: &Value) -> String {
    let kind = row
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("gradient");
    let mut letters = kind.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

/// The solid that takes the place of `row`: a solid stays as it is, a gradient becomes its first
/// colour, and an image or a pattern the default colour of list `key`. The opacity, blend and eye
/// of the paint stay.
fn solid_in_place_of(row: &Value, key: &str) -> Value {
    let kind = row.get("type").and_then(Value::as_str);
    if kind == Some("solid") {
        return row.clone();
    }
    let first = if kind == Some("gradient") {
        first_stop(row)
    } else {
        None
    };
    let (hex, alpha) = first.unwrap_or_else(|| (default_hex(key).to_string(), 1.0));
    let own = row.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    let mut solid = Map::new();
    solid.insert("type".into(), json!("solid"));
    solid.insert("color".into(), json!(hex));
    solid.insert("opacity".into(), json!((own * alpha).clamp(0.0, 1.0)));
    for kept in ["blend", "visible"] {
        if let Some(value) = row.get(kept) {
            solid.insert(kept.into(), value.clone());
        }
    }
    Value::Object(solid)
}

/// The colour of the stop with the lowest position of a gradient row, as a hex and its alpha.
fn first_stop(gradient: &Value) -> Option<(String, f64)> {
    let first = gradient
        .get("stops")?
        .as_array()?
        .iter()
        .filter_map(|stop| Some((stop.get("position")?.as_f64()?, stop.get("color")?)))
        .min_by(|(a, _), (b, _)| a.total_cmp(b))?
        .1;
    let channel = |name: &str| first.get(name).and_then(Value::as_f64);
    let rgb = [
        byte(channel("r")?),
        byte(channel("g")?),
        byte(channel("b")?),
    ];
    Some((hex_of(rgb), channel("a").unwrap_or(1.0)))
}

#[cfg(test)]
mod tests {
    use figcraft_color::{BlendMode, Paint, PaintKind as Held};
    use figcraft_doc::NodeId;
    use serde_json::json;

    use super::*;
    use crate::design::Mixed;
    use crate::design::lists::testing::{
        apply, design_file, node_set, read, rectangle, undo_steps,
    };
    use crate::design::lists::whole_numbers;
    use crate::doc::{BoardDoc, BoardError};

    /// A 1 x 1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    /// The paints the engine holds for layer `id`, in list `key`.
    fn held(board: &BoardDoc, id: NodeId, key: &str) -> Vec<Paint> {
        let doc = board.doc().unwrap();
        let node = doc.node(id).unwrap();
        if key == "fills" {
            node.fills.clone()
        } else {
            node.strokes.clone()
        }
    }

    fn solid(hex: &str, opacity: f64) -> PaintRow {
        PaintRow {
            kind: PaintKind::Solid {
                hex: hex.into(),
                opacity,
            },
            visible: true,
        }
    }

    fn gradient(label: &str) -> PaintRow {
        PaintRow {
            kind: PaintKind::Gradient(label.into()),
            visible: true,
        }
    }

    /// The rows of `key` for `ids`; the layers must agree.
    fn rows_of(board: &BoardDoc, ids: &[NodeId], key: &str) -> Vec<PaintRow> {
        paints(&read(board, ids), key).unwrap()
    }

    /// A layer holding an image paint: the file's placed image.
    fn image_layer(board: &mut BoardDoc) -> NodeId {
        let placed = board
            .execute("image.place", &json!({"data": PNG, "x": 200, "y": 0}))
            .unwrap();
        NodeId(placed["id"].as_u64().unwrap())
    }

    #[test]
    fn solid_fill_reads_hex_and_opacity() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        // Figma's default fill.
        assert_eq!(rows_of(&board, &[r], "fills"), [solid("#D9D9D9", 1.0)]);

        node_set(
            &mut board,
            &[r],
            json!({"fills": [
                {"type": "solid", "color": "#ff8800", "opacity": 0.5},
                {"type": "solid", "color": "#00FF0080"},
                {"type": "solid", "color": "#0000FF", "visible": false},
            ]}),
        );
        let rows = rows_of(&board, &[r], "fills");
        assert_eq!(rows[0], solid("#FF8800", 0.5));
        // A colour that carries alpha of its own shows it as opacity.
        let PaintKind::Solid { hex, opacity } = &rows[1].kind else {
            panic!("{:?}", rows[1]);
        };
        assert_eq!(hex, "#00FF00");
        assert!((opacity - 128.0 / 255.0).abs() < 1e-9, "{opacity}");
        assert!(rows[1].visible);
        // A hidden paint is listed, and says so.
        assert_eq!(rows[2].kind, solid("#0000FF", 1.0).kind);
        assert!(!rows[2].visible);
    }

    #[test]
    fn set_solid_changes_one_row() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        node_set(&mut board, &[r], json!({"fills": ["#FF0000", "#0000FF"]}));
        let before = held(&board, r, "fills");

        let edit = set_solid(&read(&board, &[r]), "fills", 1, "#00ff00", 0.5).unwrap();
        assert_eq!(edit.command, "node.set");
        apply(&mut board, &edit);

        let after = held(&board, r, "fills");
        assert_eq!(after[0], before[0], "the other row is as it was");
        assert_ne!(after[1], before[1]);
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [solid("#FF0000", 1.0), solid("#00FF00", 0.5)]
        );
    }

    #[test]
    fn set_solid_keeps_the_blend_and_the_eye_of_its_row() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        node_set(
            &mut board,
            &[r],
            json!({"fills": [{"type": "solid", "color": "#FF0000", "blend": "multiply", "visible": false}]}),
        );
        let edit = set_solid(&read(&board, &[r]), "fills", 0, "#0000FF", 0.25).unwrap();
        apply(&mut board, &edit);

        let after = &held(&board, r, "fills")[0];
        assert_eq!(after.blend, BlendMode::Multiply);
        assert!(!after.visible);
        assert_eq!(after.solid_color().unwrap().to_hex(), "0000FF");
        let rows = rows_of(&board, &[r], "fills");
        assert_eq!(rows[0].kind, solid("#0000FF", 0.25).kind);
        assert!(!rows[0].visible);
    }

    /// Review focus 4: a gradient or an image is never silently rewritten as a solid.
    #[test]
    fn editing_one_solid_keeps_gradient_neighbour() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        node_set(
            &mut board,
            &[r],
            json!({"fills": [{"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]}, "#112233"]}),
        );
        let before = held(&board, r, "fills");
        assert!(matches!(before[0].kind, Held::Gradient(_)));
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [gradient("Linear"), solid("#112233", 1.0)]
        );

        let edit = set_solid(&read(&board, &[r]), "fills", 1, "#FF8800", 1.0).unwrap();
        apply(&mut board, &edit);

        let after = held(&board, r, "fills");
        assert_eq!(after[0], before[0], "the gradient is identical afterwards");
        assert!(matches!(after[0].kind, Held::Gradient(_)));
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [gradient("Linear"), solid("#FF8800", 1.0)]
        );

        // Adding a paint after it and removing one beside it leave the gradient as it is too.
        let edit = add_paint(&read(&board, &[r]), "fills");
        apply(&mut board, &edit);
        let edit = remove_paint(&read(&board, &[r]), "fills", 1);
        apply(&mut board, &edit);
        assert_eq!(held(&board, r, "fills")[0], before[0]);
    }

    /// An image's rotation and a pattern's source are integers in the engine, and the panel reads
    /// every number as a float: they still go back as they were.
    #[test]
    fn image_and_pattern_neighbours_survive_an_edit() {
        let mut board = design_file();
        let image = image_layer(&mut board);
        let source = rectangle(&mut board, "Tile", 300.0);
        let patterned = rectangle(&mut board, "Patterned", 600.0);
        node_set(
            &mut board,
            &[patterned],
            json!({"fills": [{"type": "pattern", "source": source}, "#112233"]}),
        );
        assert_eq!(rows_of(&board, &[image], "fills")[0].kind, PaintKind::Image);
        assert_eq!(
            rows_of(&board, &[patterned], "fills")[0].kind,
            PaintKind::Other
        );

        // A solid is added after the image.
        let image_before = held(&board, image, "fills");
        let edit = add_paint(&read(&board, &[image]), "fills");
        apply(&mut board, &edit);
        let after = held(&board, image, "fills");
        assert_eq!(after.len(), 2);
        assert_eq!(after[0], image_before[0]);

        // A solid is edited after the pattern.
        let pattern_before = held(&board, patterned, "fills");
        let edit = set_solid(&read(&board, &[patterned]), "fills", 1, "#FF0000", 1.0).unwrap();
        apply(&mut board, &edit);
        let after = held(&board, patterned, "fills");
        assert_eq!(after[0], pattern_before[0]);
        assert!(matches!(after[0].kind, Held::Pattern(_)));
        assert_eq!(
            rows_of(&board, &[patterned], "fills")[1],
            solid("#FF0000", 1.0)
        );
    }

    #[test]
    fn gradient_rows_read_their_kind() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        node_set(
            &mut board,
            &[r],
            json!({"fills": [
                {"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]},
                {"type": "radialGradient", "stops": ["#FF0000", "#0000FF"]},
                {"type": "angularGradient", "stops": ["#FF0000", "#0000FF"]},
                {"type": "diamondGradient", "stops": ["#FF0000", "#0000FF"]},
            ]}),
        );
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [
                gradient("Linear"),
                gradient("Radial"),
                gradient("Angular"),
                gradient("Diamond")
            ]
        );
    }

    #[test]
    fn bad_hex_is_rejected() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        let sel = read(&board, &[r]);
        for bad in [
            "#12",
            "12",
            "",
            "#",
            "   ",
            "#GGGGGG",
            "#12345",
            "#1234567",
            // Alpha is the opacity's business.
            "#FF00",
            "#FF000080",
            "red",
            "#ff ff00",
            "0x112233",
            "##112233",
            "#11223",
            // Digits of another script are no hex digits.
            "#\u{ff11}\u{ff12}\u{ff13}\u{ff14}\u{ff15}\u{ff16}",
        ] {
            assert_eq!(set_solid(&sel, "fills", 0, bad, 1.0), None, "{bad:?}");
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        // Nothing was edited, and the row is as it was.
        assert_eq!(rows_of(&board, &[r], "fills"), [solid("#D9D9D9", 1.0)]);
    }

    #[test]
    fn hex_comes_in_the_forms_a_designer_types() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        for typed in ["#ff8800", "FF8800", "#f80", "f80", "  #Ff8800 ", "\t#F80\n"] {
            let edit = set_solid(&read(&board, &[r]), "fills", 0, typed, 1.0).unwrap();
            apply(&mut board, &edit);
            assert_eq!(
                rows_of(&board, &[r], "fills"),
                [solid("#FF8800", 1.0)],
                "{typed:?}"
            );
            assert_eq!(parse_hex(typed).as_deref(), Some("#FF8800"), "{typed:?}");
        }
        assert_eq!(parse_hex("#000"), Some("#000000".into()));
        assert_eq!(parse_hex("fFf"), Some("#FFFFFF".into()));
    }

    #[test]
    fn opacity_is_kept_between_0_and_1_and_must_be_a_number() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        for (given, kept) in [(0.5, 0.5), (1.5, 1.0), (-0.5, 0.0), (0.0, 0.0), (1.0, 1.0)] {
            let edit = set_solid(&read(&board, &[r]), "fills", 0, "#FF0000", given).unwrap();
            apply(&mut board, &edit);
            assert_eq!(
                rows_of(&board, &[r], "fills"),
                [solid("#FF0000", kept)],
                "{given}"
            );
        }
        let sel = read(&board, &[r]);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(set_solid(&sel, "fills", 0, "#FF0000", bad), None, "{bad}");
        }
    }

    #[test]
    fn set_solid_edits_only_a_solid_row_that_is_there() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        node_set(
            &mut board,
            &[r],
            json!({"fills": [{"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]}, "#112233"]}),
        );
        let sel = read(&board, &[r]);
        // The gradient is not a solid, so no colour turns it into one.
        assert_eq!(set_solid(&sel, "fills", 0, "#FF0000", 1.0), None);
        // There is no third row.
        assert_eq!(set_solid(&sel, "fills", 2, "#FF0000", 1.0), None);
        assert_eq!(set_solid(&sel, "fills", usize::MAX, "#FF0000", 1.0), None);
        assert!(set_solid(&sel, "fills", 1, "#FF0000", 1.0).is_some());

        // Nobody selected has no row either, and nor do layers that disagree.
        assert_eq!(
            set_solid(&read(&board, &[]), "fills", 0, "#FF0000", 1.0),
            None
        );
        let other = rectangle(&mut board, "Other", 300.0);
        let mixed = read(&board, &[r, other]);
        assert_eq!(set_solid(&mixed, "fills", 0, "#FF0000", 1.0), None);
    }

    #[test]
    fn add_and_remove_paint() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);

        // A fill goes after the fills there are.
        let edit = add_paint(&read(&board, &[r]), "fills");
        assert_eq!(edit.command, "node.set");
        apply(&mut board, &edit);
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [solid("#D9D9D9", 1.0), solid("#D9D9D9", 1.0)]
        );
        // A stroke where there was none, in black.
        assert_eq!(rows_of(&board, &[r], "strokes"), []);
        let edit = add_paint(&read(&board, &[r]), "strokes");
        apply(&mut board, &edit);
        assert_eq!(rows_of(&board, &[r], "strokes"), [solid("#000000", 1.0)]);
        assert_eq!(rows_of(&board, &[r], "fills").len(), 2, "the fills stay");

        // A removed row leaves the rows around it.
        node_set(
            &mut board,
            &[r],
            json!({"fills": ["#FF0000", "#00FF00", "#0000FF"]}),
        );
        let edit = remove_paint(&read(&board, &[r]), "fills", 1);
        apply(&mut board, &edit);
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [solid("#FF0000", 1.0), solid("#0000FF", 1.0)]
        );
        // Until none is left: an empty list is not a mixed one.
        let edit = remove_paint(&read(&board, &[r]), "fills", 0);
        apply(&mut board, &edit);
        let edit = remove_paint(&read(&board, &[r]), "fills", 0);
        apply(&mut board, &edit);
        assert_eq!(paints(&read(&board, &[r]), "fills"), Some(vec![]));
        assert_eq!(rows_of(&board, &[r], "strokes").len(), 1);
    }

    #[test]
    fn mixed_paints_are_none() {
        let mut board = design_file();
        let r1 = rectangle(&mut board, "R1", 0.0);
        let r2 = rectangle(&mut board, "R2", 300.0);
        node_set(&mut board, &[r2], json!({"fills": ["#FF0000"]}));

        assert_eq!(paints(&read(&board, &[r1, r2]), "fills"), None);
        // Each on its own is a list, and so are layers that agree.
        assert_eq!(rows_of(&board, &[r1], "fills"), [solid("#D9D9D9", 1.0)]);
        assert_eq!(rows_of(&board, &[r2], "fills"), [solid("#FF0000", 1.0)]);
        node_set(&mut board, &[r1], json!({"fills": ["#FF0000"]}));
        assert_eq!(rows_of(&board, &[r1, r2], "fills"), [solid("#FF0000", 1.0)]);

        // A stroke on one of them makes the strokes differ, though the fills agree now.
        node_set(&mut board, &[r1], json!({"strokes": ["#000000"]}));
        assert_eq!(paints(&read(&board, &[r1, r2]), "strokes"), None);
        assert!(paints(&read(&board, &[r1, r2]), "fills").is_some());
    }

    /// FigCraft leaves an empty list out of a layer's JSON; the panel must not read that as mixed.
    #[test]
    fn layers_without_paints_hold_empty_lists() {
        let mut board = design_file();
        let r1 = rectangle(&mut board, "R1", 0.0);
        let r2 = rectangle(&mut board, "R2", 300.0);
        node_set(&mut board, &[r1, r2], json!({"fills": []}));
        let sel = read(&board, &[r1, r2]);
        assert_eq!(paints(&sel, "fills"), Some(vec![]));
        assert_eq!(paints(&sel, "strokes"), Some(vec![]));
        // Nothing selected has no paints.
        assert_eq!(paints(&read(&board, &[]), "fills"), Some(vec![]));
    }

    #[test]
    fn paint_edits_apply_to_every_selected_layer_in_one_undo_step() {
        let mut board = design_file();
        let r1 = rectangle(&mut board, "R1", 0.0);
        let r2 = rectangle(&mut board, "R2", 300.0);
        let sel = read(&board, &[r1, r2]);
        let steps = undo_steps(&board);

        let edit = set_solid(&sel, "fills", 0, "#FF0000", 0.5).unwrap();
        apply(&mut board, &edit);
        assert_eq!(undo_steps(&board), steps + 1);
        for id in [r1, r2] {
            assert_eq!(rows_of(&board, &[id], "fills"), [solid("#FF0000", 0.5)]);
        }

        let edit = add_paint(&read(&board, &[r1, r2]), "strokes");
        apply(&mut board, &edit);
        assert_eq!(undo_steps(&board), steps + 2);
        for id in [r1, r2] {
            assert_eq!(rows_of(&board, &[id], "strokes"), [solid("#000000", 1.0)]);
        }

        board.execute("edit.undo", &json!({})).unwrap();
        board.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(undo_steps(&board), steps);
        for id in [r1, r2] {
            assert_eq!(rows_of(&board, &[id], "fills"), [solid("#D9D9D9", 1.0)]);
            assert_eq!(rows_of(&board, &[id], "strokes"), []);
        }
    }

    #[test]
    fn replace_all_makes_one_new_paint_the_only_paint_of_every_layer() {
        let mut board = design_file();
        let r1 = rectangle(&mut board, "R1", 0.0);
        let r2 = rectangle(&mut board, "R2", 300.0);
        node_set(&mut board, &[r2], json!({"fills": ["#FF0000", "#0000FF"]}));
        let sel = read(&board, &[r1, r2]);
        assert_eq!(paints(&sel, "fills"), None);
        let steps = undo_steps(&board);

        let edit = add_paint(&sel, "fills");
        assert_eq!(edit.params["ids"], json!([r1, r2]));
        apply(&mut board, &edit);
        assert_eq!(undo_steps(&board), steps + 1);
        assert_eq!(rows_of(&board, &[r1, r2], "fills"), [solid("#D9D9D9", 1.0)]);

        board.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(rows_of(&board, &[r1], "fills"), [solid("#D9D9D9", 1.0)]);
        assert_eq!(
            rows_of(&board, &[r2], "fills"),
            [solid("#FF0000", 1.0), solid("#0000FF", 1.0)]
        );
    }

    /// With no list to change a row of, or no such row, an edit says so by being refused.
    #[test]
    fn edits_that_cannot_apply_are_refused_and_change_nothing() {
        let mut board = design_file();
        let r1 = rectangle(&mut board, "R1", 0.0);
        let r2 = rectangle(&mut board, "R2", 300.0);
        node_set(&mut board, &[r2], json!({"fills": ["#FF0000"]}));
        let one = read(&board, &[r1]);
        let differing = read(&board, &[r1, r2]);

        let edits = [
            remove_paint(&one, "fills", 1),
            remove_paint(&one, "fills", usize::MAX),
            replace_with_solid(&one, "fills", 1),
            set_visible(&one, "fills", 1, false),
            remove_paint(&one, "strokes", 0),
            remove_paint(&differing, "fills", 0),
            replace_with_solid(&differing, "fills", 0),
            set_visible(&differing, "fills", 0, false),
        ];
        let snapshot = read(&board, &[r1, r2]);
        let steps = undo_steps(&board);
        for edit in edits {
            let revision = board.revision();
            let result = board.execute(edit.command, &edit.params);
            assert!(matches!(result, Err(BoardError::Engine(_))), "{edit:?}");
            assert_eq!(board.revision(), revision, "{edit:?}");
        }
        assert_eq!(read(&board, &[r1, r2]), snapshot);
        assert_eq!(undo_steps(&board), steps);
    }

    #[test]
    fn replace_with_solid_turns_a_gradient_into_its_first_colour() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        // The stops are listed out of order: the first colour is the one at position 0.
        node_set(
            &mut board,
            &[r],
            json!({"fills": [
                "#112233",
                {"type": "linearGradient", "opacity": 0.5, "blend": "multiply", "visible": false, "stops": [
                    {"position": 1.0, "color": "#0000FF"},
                    {"position": 0.0, "color": "#FF000080"},
                ]},
                "#445566",
            ]}),
        );
        let before = held(&board, r, "fills");

        let edit = replace_with_solid(&read(&board, &[r]), "fills", 1);
        apply(&mut board, &edit);

        let after = held(&board, r, "fills");
        assert_eq!(after[0], before[0]);
        assert_eq!(after[2], before[2]);
        assert!(matches!(after[1].kind, Held::Solid { .. }));
        assert_eq!(after[1].blend, BlendMode::Multiply, "the blend stays");
        assert!(!after[1].visible, "the eye stays");
        let rows = rows_of(&board, &[r], "fills");
        let PaintKind::Solid { hex, opacity } = &rows[1].kind else {
            panic!("{:?}", rows[1]);
        };
        assert_eq!(hex, "#FF0000");
        // The paint's opacity and the alpha of the stop.
        assert!((opacity - 0.5 * 128.0 / 255.0).abs() < 1e-6, "{opacity}");
        assert!(!rows[1].visible);
    }

    #[test]
    fn replace_with_solid_turns_an_image_or_a_pattern_into_the_default_colour() {
        let mut board = design_file();
        let image = image_layer(&mut board);
        let source = rectangle(&mut board, "Tile", 300.0);
        let r = rectangle(&mut board, "R", 600.0);
        let image_paint = read(&board, &[image]).get("fills");
        let Mixed::Same(image_paints) = image_paint else {
            panic!("{image_paint:?}");
        };
        node_set(
            &mut board,
            &[r],
            json!({
                "fills": [{"type": "pattern", "source": source, "opacity": 0.5}, "#112233"],
                "strokes": whole_numbers(image_paints),
            }),
        );

        let edit = replace_with_solid(&read(&board, &[r]), "fills", 0);
        apply(&mut board, &edit);
        assert_eq!(
            rows_of(&board, &[r], "fills"),
            [solid("#D9D9D9", 0.5), solid("#112233", 1.0)]
        );
        // A stroke takes the colour of a new stroke.
        assert_eq!(rows_of(&board, &[r], "strokes")[0].kind, PaintKind::Image);
        let edit = replace_with_solid(&read(&board, &[r]), "strokes", 0);
        apply(&mut board, &edit);
        assert_eq!(rows_of(&board, &[r], "strokes"), [solid("#000000", 1.0)]);
    }

    #[test]
    fn replace_with_solid_leaves_a_solid_as_it_is() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        node_set(
            &mut board,
            &[r],
            json!({"fills": [{"type": "solid", "color": "#FF0000", "opacity": 0.5}]}),
        );
        let before = held(&board, r, "fills");
        let edit = replace_with_solid(&read(&board, &[r]), "fills", 0);
        apply(&mut board, &edit);
        assert_eq!(held(&board, r, "fills"), before);
    }

    #[test]
    fn set_visible_shows_and_hides_one_row_and_changes_nothing_else() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        node_set(
            &mut board,
            &[r],
            json!({"fills": ["#FF0000", {"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]}]}),
        );
        let before = held(&board, r, "fills");

        let edit = set_visible(&read(&board, &[r]), "fills", 1, false);
        apply(&mut board, &edit);
        let rows = rows_of(&board, &[r], "fills");
        assert!(rows[0].visible);
        assert_eq!(rows[1].kind, gradient("Linear").kind);
        assert!(!rows[1].visible);
        let after = held(&board, r, "fills");
        assert_eq!(after[0], before[0]);
        assert_eq!(after[1].kind, before[1].kind);
        assert!(!after[1].visible);

        let edit = set_visible(&read(&board, &[r]), "fills", 1, true);
        apply(&mut board, &edit);
        assert_eq!(held(&board, r, "fills"), before);
    }

    #[test]
    fn strokes_are_edited_like_fills() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        let edit = add_paint(&read(&board, &[r]), "strokes");
        apply(&mut board, &edit);
        let fills = held(&board, r, "fills");

        let edit = set_solid(&read(&board, &[r]), "strokes", 0, "#336699", 0.75).unwrap();
        apply(&mut board, &edit);
        assert_eq!(rows_of(&board, &[r], "strokes"), [solid("#336699", 0.75)]);
        let edit = set_visible(&read(&board, &[r]), "strokes", 0, false);
        apply(&mut board, &edit);
        assert!(!rows_of(&board, &[r], "strokes")[0].visible);
        let edit = remove_paint(&read(&board, &[r]), "strokes", 0);
        apply(&mut board, &edit);
        assert_eq!(rows_of(&board, &[r], "strokes"), []);
        assert_eq!(
            held(&board, r, "fills"),
            fills,
            "the fills were not touched"
        );
    }

    #[test]
    fn stroke_fields_are_the_keys_the_section_reads_and_writes() {
        assert_eq!(stroke_fields(), ["strokeWeight", "strokeAlign", "dashes"]);

        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        // Without a stroke there is no weight or alignment to show, and the dashes are none.
        let sel = read(&board, &[r]);
        assert_eq!(sel.get("strokeWeight"), Mixed::Absent);
        assert_eq!(sel.get("strokeAlign"), Mixed::Absent);
        assert_eq!(sel.get("dashes"), Mixed::Same(json!([])));

        let edit = add_paint(&sel, "strokes");
        apply(&mut board, &edit);
        for key in stroke_fields() {
            let value = read(&board, &[r]).get(key);
            assert!(matches!(value, Mixed::Same(_)), "{key}: {value:?}");
        }
        assert_eq!(
            read(&board, &[r]).get("strokeWeight"),
            Mixed::Same(json!(1.0))
        );

        // Each is written back under the name it is read under.
        for (key, value) in [
            ("strokeWeight", json!(3.0)),
            ("strokeAlign", json!("center")),
            ("dashes", json!([4.0, 4.0])),
            ("dashes", json!([])),
        ] {
            let edit = read(&board, &[r]).set(key, value.clone());
            apply(&mut board, &edit);
            assert_eq!(read(&board, &[r]).get(key), Mixed::Same(value), "{key}");
        }
    }
}
