//! An element's fields as the importer reads them: strings, flags, numbers, sides, colours, points.

use figcraft_color::Color;
use serde_json::Value;

use super::{Fields, MAX_SIDE, MIN_SIDE};

pub(super) fn string<'a>(fields: &'a Fields, key: &str) -> Option<&'a str> {
    fields.get(key)?.as_str()
}

pub(super) fn flag(fields: &Fields, key: &str) -> Option<bool> {
    fields.get(key)?.as_bool()
}

/// A number that can be drawn: JSON has no other.
pub(super) fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}

pub(super) fn number(fields: &Fields, key: &str) -> Option<f64> {
    finite(fields.get(key)?)
}

/// A width or a height, held to the sides the engine takes ([`MIN_SIDE`]).
pub(super) fn side(fields: &Fields, key: &str) -> Option<f64> {
    number(fields, key).map(|side| side.clamp(MIN_SIDE, MAX_SIDE))
}

/// The colour in `key`, if the engine can read it: a CSS colour (hex, `rgb()`, a name), or
/// transparent. One it can't is left out, and the element keeps the colour it would have had.
pub(super) fn colour(fields: &Fields, key: &str) -> Option<String> {
    let colour = string(fields, key)?.trim();
    let readable = Color::parse_css(colour).is_some() || colour.eq_ignore_ascii_case("none");
    readable.then(|| colour.to_string())
}

/// What a text says: the text as it was typed (`originalText`, which Excalidraw has already
/// wrapped to a width in `text`), else `text`. `None` for no characters.
pub(super) fn text_of(fields: &Fields) -> Option<&str> {
    ["originalText", "text"]
        .into_iter()
        .filter_map(|key| string(fields, key))
        .find(|text| !text.is_empty())
}

/// The `points` of a line or a freedraw, relative to its `x`, `y`. A point that isn't a pair of
/// numbers is dropped. `None` for no points.
pub(super) fn points_of(fields: &Fields) -> Option<Vec<[f64; 2]>> {
    let points: Vec<[f64; 2]> = fields
        .get("points")?
        .as_array()?
        .iter()
        .filter_map(|point| {
            let pair = point.as_array()?;
            Some([finite(pair.first()?)?, finite(pair.get(1)?)?])
        })
        .collect();
    (!points.is_empty()).then_some(points)
}
