//! The panel's fields: the keys a layer is read and written under, the defaults of the keys the
//! engine leaves out when they hold their default, and how typed input becomes a number.
//!
//! The keys are the ones `node.set` takes (documented in `figcraft_engine::cmd::props::KEYS`; no
//! code iterates that list). A layer reads as `inspect::node_json` describes it, with three
//! changes so that every key reads under the name it is written with: [`DEFAULTS`] fill the keys
//! `node_json` leaves out, the nested values `node_json` gives are flattened, and a few keys it
//! never gives are added from the document.

use std::sync::LazyLock;

use figcraft_doc::{Document, Node};
use figcraft_engine::inspect::node_json;
use serde_json::{Map, Number, Value, json};

/// The properties `node_json` leaves out while they hold their default, each with that default as
/// JSON. A layer that leaves one out holds its default. Without these, a layer that leaves a key
/// out wouldn't count for it: a hidden layer selected with a shown one would read `Same(false)`
/// instead of mixed, and a layer without a stroke would have no `strokes` at all, not an empty list.
///
/// `blend` isn't here: its default depends on the layer (frames, groups and sections pass through,
/// shapes are normal), and `node_json` leaves out both, so `layer_props` reads it from the layer.
pub const DEFAULTS: &[(&str, &str)] = &[
    ("visible", "true"),
    ("locked", "false"),
    ("opacity", "1"),
    ("rotation", "0"),
    ("cornerRadius", "0"),
    ("fills", "[]"),
    ("strokes", "[]"),
    ("effects", "[]"),
    ("exports", "[]"),
    // `node_json` gives a stroke's dashes only while there are some.
    ("dashes", "[]"),
];

/// The properties only text layers have. `node.set` refuses them on any other layer, so an edit
/// of one goes to the text layers of a selection.
pub(super) const TEXT_KEYS: &[&str] = &[
    "characters",
    "fontFamily",
    "fontStyle",
    "fontSize",
    "lineHeight",
    "letterSpacing",
    "textAlign",
    "textAlignVertical",
    "textAutoResize",
];

/// The keys of a frame's auto layout: the ones [`flatten_layout`] lifts out of `node_json`'s
/// `layout` object. A frame without auto layout has none of them, and `node.set` of any of them
/// gives it an auto layout, so [`DesignSelection::get`](super::DesignSelection::get) counts such a
/// frame as a value of its own for each of them, instead of leaving it out.
pub(super) const LAYOUT_KEYS: &[&str] = &[
    "layout",
    "gap",
    "crossGap",
    "padding",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
    "mainAlign",
    "crossAlign",
    "wrap",
];

/// The sides of a layout's padding, in the order of its `[top, right, bottom, left]` list.
const PADDING: [&str; 4] = ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"];

/// The corners, in the order of a layer's `[top left, top right, bottom right, bottom left]` list.
const CORNERS: [&str; 4] = [
    "topLeftRadius",
    "topRightRadius",
    "bottomRightRadius",
    "bottomLeftRadius",
];

/// [`DEFAULTS`] as values, read once.
static DEFAULT_VALUES: LazyLock<Vec<(&'static str, Value)>> = LazyLock::new(|| {
    DEFAULTS
        .iter()
        .filter_map(|(key, json)| Some((*key, floats(serde_json::from_str(json).ok()?))))
        .collect()
});

/// What typed `input` means for the number field `key`, or `None` when it is no good, and the
/// field shows its current value again.
///
/// - Surrounding space is ignored.
/// - A number that isn't finite (`NaN`, `inf`, `1e400`) is no good.
/// - A size, a gap, a padding, a stroke weight, a font size and a corner radius can't be below 0.
///   The engine would clamp such a value to 0, so the field refuses it instead of showing a
///   number that was not applied. Position and rotation can be negative.
/// - Opacity is a fraction, or a percentage with a `%`, and is kept between 0 and 1.
pub fn parse_number(key: &str, input: &str) -> Option<f64> {
    let text = input.trim();
    let value = match (key, text.strip_suffix('%')) {
        ("opacity", Some(percent)) => percent.trim().parse::<f64>().ok()? / 100.0,
        _ => text.parse::<f64>().ok()?,
    };
    if !value.is_finite() {
        return None;
    }
    // Minus zero is zero, so a field never shows "-0".
    let value = value + 0.0;
    if key == "opacity" {
        return Some(value.clamp(0.0, 1.0));
    }
    if value < 0.0 && is_never_negative(key) {
        return None;
    }
    Some(value)
}

/// Whether field `key` can't hold a negative number.
fn is_never_negative(key: &str) -> bool {
    matches!(
        key,
        "width" | "height" | "strokeWeight" | "fontSize" | "gap" | "crossGap" | "cornerRadius"
    ) || key.starts_with("padding")
        || key.ends_with("Radius")
}

/// What layer `node` holds for each key the panel reads: [`node_json`] without its `id`, with the
/// [`DEFAULTS`] filled in, and with every number an `f64`.
///
/// Where `node_json` nests or hides a value, it reads under the key `node.set` takes:
/// - `layout` is its mode (`"horizontal"`, `"vertical"` or `"grid"`), and `gap`, `crossGap`,
///   `mainAlign`, `crossAlign` and `wrap` are its own keys. `padding` is one number when the four
///   sides agree and else the list `[top, right, bottom, left]`, the two shapes `node.set` takes;
///   `paddingTop`, `paddingRight`, `paddingBottom` and `paddingLeft` give the sides one by one.
///   A layer without auto layout has none of these.
/// - `sizing` is `sizingH` and `sizingV` (`"fixed"`, `"hug"` or `"fill"`). A layer that hasn't a
///   sizing to choose, such as a rectangle outside auto layout, has none.
/// - `cornerRadius` stays a number, or the list `[top left, top right, bottom right, bottom left]`
///   when the corners differ, and `topLeftRadius`, `topRightRadius`, `bottomRightRadius` and
///   `bottomLeftRadius` give the corners one by one.
/// - `blend` is the layer's own, `"normal"` or `"passThrough"` included.
/// - `constrainProportions` comes from the layer, and so does `textAlignVertical` for a text.
pub(super) fn layer_props(doc: &Document, node: &Node) -> Map<String, Value> {
    let Value::Object(mut props) = floats(node_json(doc, node.id, 0)) else {
        return Map::new();
    };
    props.remove("id");
    for (key, default) in DEFAULT_VALUES.iter() {
        props.entry(*key).or_insert_with(|| default.clone());
    }
    flatten_layout(&mut props);
    flatten_sizing(&mut props);
    flatten_corners(&mut props);
    props.insert("blend".into(), json!(node.blend));
    props.insert(
        "constrainProportions".into(),
        json!(node.constrain_proportions),
    );
    if let Some(text) = node.kind.text() {
        props.insert("textAlignVertical".into(), json!(text.align_v));
    }
    props
}

/// `value` with every number held as an `f64`. `serde_json` compares numbers by the way they are
/// stored, so `100` and `100.0` differ, and the engine reports sizes as floats but some counts as
/// integers, while [`DEFAULTS`] are plain JSON.
fn floats(value: Value) -> Value {
    match value {
        Value::Number(n) => match n.as_f64().and_then(Number::from_f64) {
            Some(float) => Value::Number(float),
            None => Value::Number(n),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(floats).collect()),
        Value::Object(fields) => {
            Value::Object(fields.into_iter().map(|(k, v)| (k, floats(v))).collect())
        }
        other => other,
    }
}

/// Replaces the `layout` object with the keys described at [`layer_props`].
fn flatten_layout(props: &mut Map<String, Value>) {
    let Some(Value::Object(layout)) = props.remove("layout") else {
        return;
    };
    if let Some(mode) = layout.get("mode") {
        props.insert("layout".into(), mode.clone());
    }
    for key in ["gap", "crossGap", "mainAlign", "crossAlign", "wrap"] {
        if let Some(value) = layout.get(key) {
            props.insert(key.into(), value.clone());
        }
    }
    if let Some(Value::Array(sides)) = layout.get("padding")
        && sides.len() == PADDING.len()
    {
        for (key, side) in PADDING.into_iter().zip(sides) {
            props.insert(key.into(), side.clone());
        }
        let uniform = sides
            .first()
            .filter(|first| sides.iter().all(|s| s == *first));
        let padding = uniform.map_or_else(|| Value::Array(sides.clone()), Value::clone);
        props.insert("padding".into(), padding);
    }
}

/// Replaces the `sizing` object with `sizingH` and `sizingV`.
fn flatten_sizing(props: &mut Map<String, Value>) {
    let Some(Value::Object(sizing)) = props.remove("sizing") else {
        return;
    };
    for (from, to) in [("horizontal", "sizingH"), ("vertical", "sizingV")] {
        if let Some(value) = sizing.get(from) {
            props.insert(to.into(), value.clone());
        }
    }
}

/// Adds the corners one by one, from the `cornerRadius` that is a number or a list of four.
fn flatten_corners(props: &mut Map<String, Value>) {
    let radii = match props.get("cornerRadius") {
        Some(Value::Array(radii)) if radii.len() == CORNERS.len() => radii.clone(),
        Some(radius @ Value::Number(_)) => vec![radius.clone(); CORNERS.len()],
        _ => vec![json!(0.0); CORNERS.len()],
    };
    for (key, radius) in CORNERS.into_iter().zip(radii) {
        props.insert(key.into(), radius);
    }
}
