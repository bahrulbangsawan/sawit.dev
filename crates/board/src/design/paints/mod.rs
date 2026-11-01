//! The Fill and Stroke sections' model: a layer's paints as rows, and the edits that change them.
//!
//! `fills` and `strokes` are lists of paints. A solid paint reads as
//! `{type: "solid", color: "#RRGGBB", opacity?, blend?, visible?}`, and a gradient, an image or a
//! pattern as the engine's own paint JSON. An edit that changes or adds a row sends the whole list
//! back through `node.set`, so every row it doesn't change goes back as it was read, and a
//! gradient or an image is never rewritten as a solid: only [`replace_with_solid`] does that.
//! [`remove_paint`] uses the engine's command for one row, so the variable bindings of the rows
//! after it move with them.
//!
//! The one thing a whole list sent back can lose is a binding a solid row holds: the row goes back
//! as its 8-bit hex, and a colour bound to a variable that isn't on the 8-bit grid (or isn't
//! opaque) lets go of the variable when another row of the list is edited. Variables are a
//! non-goal of the spec, and the final review accepts this (its m1).
//!
//! Where a list can't be edited row by row, because the layers hold different lists, the panel
//! shows "Mixed" with a "Replace all" button. [`add_paint`] is that button: it makes one new paint
//! the only paint of every layer. The other edits have no row to change then, and are refused.
//!
//! An edit is made from the selection it is given and sends that selection's whole list, so make
//! each edit from a selection read after the last one.

use serde_json::{Map, Value, json};

use super::lists::{change_row, refused, remove_row, replace, rows};
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

/// The edit that removes row `index` of list `key`, with the engine's `fill.remove` or
/// `stroke.remove`: the rows after it keep their variable bindings. A row that isn't there, a
/// list that is neither `fills` nor `strokes`, or lists that differ from layer to layer make an
/// edit the engine refuses and that changes nothing.
pub fn remove_paint(sel: &DesignSelection, key: &str, index: usize) -> Edit {
    let command = match key {
        "fills" => "fill.remove",
        "strokes" => "stroke.remove",
        _ => return refused(),
    };
    remove_row(sel, key, index, command)
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
mod tests;
