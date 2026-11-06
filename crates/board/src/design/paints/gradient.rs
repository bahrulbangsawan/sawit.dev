//! Gradient and image paints in the Fill and Stroke sections: what a gradient row's editor shows
//! (its kind, its angle and its stops) and an image row's (its fill mode, tile scale, rotation and
//! adjustments), and the edits that change them. Every edit replaces the one paint through
//! `fill.set` / `stroke.set` with the paint as read, changed where the edit says: the keys it
//! doesn't show (the handles of a radial gradient, an image's crop) stay as they are, and so do
//! the other rows.
//!
//! A paint reads as the engine writes it: a gradient as `{type: "gradient", kind, stops:
//! [{position, color: {r, g, b, a}}], from: [x, y], to: [x, y], width: [x, y]}`, its handles in
//! the layer's unit box, and an image as `{type: "image", image, scaleMode, scale, rotation,
//! crop?, filters: {exposure, …}}`.

use serde_json::{Map, Value, json};

use super::{DesignSelection, Edit, byte, default_hex, hex_of, parse_rgb, refused, rows};

/// The kinds of paint a row can be turned into.
pub const KINDS: [(&str, &str); 5] = [
    ("solid", "Solid"),
    ("linear", "Linear"),
    ("radial", "Radial"),
    ("angular", "Angular"),
    ("diamond", "Diamond"),
];

/// One stop of a gradient row.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    /// Where it is along the gradient, 0 to 1.
    pub position: f64,
    /// Its colour as `#RRGGBB` in upper case, and its alpha, 0 to 1.
    pub hex: String,
    pub opacity: f64,
}

/// What a gradient row's editor shows.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientRow {
    /// `linear`, `radial`, `angular` or `diamond`.
    pub kind: String,
    /// The stops in the order of their positions; [`Stop`]s' indices in the edits are into this
    /// order.
    pub stops: Vec<Stop>,
    /// The direction from the start handle to the end one, in degrees, 0 pointing right and 90
    /// down, as the layer's box is drawn.
    pub angle: f64,
}

/// The paint of row `index` of list `key`, as read; `None` when there is no such row or the
/// layers hold different lists.
fn paint_at(sel: &DesignSelection, key: &str, index: usize) -> Option<Map<String, Value>> {
    rows(sel, key)?.get(index)?.as_object().cloned()
}

/// The edit that puts `paint` in place of row `index` of list `key`.
fn put(sel: &DesignSelection, key: &str, index: usize, paint: Map<String, Value>) -> Edit {
    let command = match key {
        "fills" => "fill.set",
        "strokes" => "stroke.set",
        _ => return refused(),
    };
    match rows(sel, key) {
        Some(list) if index < list.len() => Edit {
            command,
            params: json!({"ids": sel.ids, "index": index, "paint": Value::Object(paint)}),
        },
        _ => refused(),
    }
}

/// Changes row `index` of list `key` with `change`, refused when it gives `None`.
fn change(
    sel: &DesignSelection,
    key: &str,
    index: usize,
    change: impl FnOnce(&mut Map<String, Value>) -> Option<()>,
) -> Edit {
    let Some(mut paint) = paint_at(sel, key, index) else {
        return refused();
    };
    match change(&mut paint) {
        Some(()) => put(sel, key, index, paint),
        None => refused(),
    }
}

/// A gradient row's editor, or `None` when row `index` isn't a gradient.
pub fn gradient(sel: &DesignSelection, key: &str, index: usize) -> Option<GradientRow> {
    let paint = paint_at(sel, key, index)?;
    gradient_of(&paint)
}

fn is_gradient(paint: &Map<String, Value>) -> bool {
    let kind = paint
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    kind == "gradient" || kind.ends_with("Gradient")
}

fn gradient_of(paint: &Map<String, Value>) -> Option<GradientRow> {
    if !is_gradient(paint) {
        return None;
    }
    let kind = paint
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            let tag = paint.get("type")?.as_str()?;
            Some(tag.strip_suffix("Gradient")?.to_string())
        })
        .unwrap_or_else(|| "linear".to_string());
    let mut stops: Vec<Stop> = stops_of(paint).iter().filter_map(stop_of).collect();
    stops.sort_by(|a, b| a.position.total_cmp(&b.position));
    let (from, to) = (
        point(paint, "from", (0.5, 0.0)),
        point(paint, "to", (0.5, 1.0)),
    );
    let angle = (to.1 - from.1).atan2(to.0 - from.0).to_degrees();
    Some(GradientRow {
        kind,
        stops,
        angle: round(angle.rem_euclid(360.0), 2),
    })
}

fn stops_of(paint: &Map<String, Value>) -> Vec<Value> {
    paint
        .get("stops")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn stop_of(stop: &Value) -> Option<Stop> {
    let position = stop.get("position")?.as_f64()?;
    let (rgb, alpha) = color_of(stop.get("color")?)?;
    Some(Stop {
        position,
        hex: hex_of(rgb),
        opacity: alpha,
    })
}

/// A colour read as `{r, g, b, a}` (0 to 1) or as a hex: its bytes and its alpha.
fn color_of(color: &Value) -> Option<([u8; 3], f64)> {
    if let Some(text) = color.as_str() {
        let digits = text.strip_prefix('#').unwrap_or(text);
        let rgb = parse_rgb(&digits[..digits.len().min(6)])?;
        let alpha = (digits.len() == 8)
            .then(|| u8::from_str_radix(&digits[6..8], 16).ok())
            .flatten()
            .map_or(1.0, |a| f64::from(a) / 255.0);
        return Some((rgb, alpha));
    }
    let channel = |name: &str| color.get(name).and_then(Value::as_f64);
    let rgb = [
        byte(channel("r")?),
        byte(channel("g")?),
        byte(channel("b")?),
    ];
    Some((rgb, channel("a").unwrap_or(1.0)))
}

fn color_json(hex: &str, opacity: f64) -> Option<Value> {
    let [r, g, b] = parse_rgb(hex.trim_start_matches('#'))?;
    let unit = |c: u8| f64::from(c) / 255.0;
    Some(json!({"r": unit(r), "g": unit(g), "b": unit(b), "a": opacity.clamp(0.0, 1.0)}))
}

fn point(paint: &Map<String, Value>, key: &str, or: (f64, f64)) -> (f64, f64) {
    let pair = paint.get(key).and_then(Value::as_array);
    let at = |i: usize| pair.and_then(|p| p.get(i)).and_then(Value::as_f64);
    match (at(0), at(1)) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => (x, y),
        _ => or,
    }
}

fn round(n: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (n * k).round() / k
}

/// The handles a new gradient of `kind` starts with, as FigCraft's own default: a linear one top
/// to bottom, the others from the middle.
fn default_handles(kind: &str) -> [(f64, f64); 3] {
    match kind {
        "linear" => [(0.5, 0.0), (0.5, 1.0), (0.0, 0.5)],
        _ => [(0.5, 0.5), (0.5, 1.0), (1.0, 0.5)],
    }
}

fn set_handles(paint: &mut Map<String, Value>, [from, to, width]: [(f64, f64); 3]) {
    paint.insert("from".into(), json!([from.0, from.1]));
    paint.insert("to".into(), json!([to.0, to.1]));
    paint.insert("width".into(), json!([width.0, width.1]));
}

/// The colour a row of any kind shows, as a hex and its alpha: a solid's, a gradient's first stop,
/// or the default of list `key`.
fn base_colour(paint: &Map<String, Value>, key: &str) -> (String, f64) {
    if paint.get("type").and_then(Value::as_str) == Some("solid")
        && let Some((rgb, alpha)) = paint.get("color").and_then(color_of)
    {
        return (hex_of(rgb), alpha);
    }
    if let Some(first) = gradient_of(paint).and_then(|g| g.stops.into_iter().next()) {
        return (first.hex, first.opacity);
    }
    (default_hex(key).to_string(), 1.0)
}

/// The edit that turns row `index` of list `key` into a paint of `kind` (one of [`KINDS`]): a
/// gradient of another kind keeps its stops and takes that kind's handles; a solid becomes a
/// gradient from its colour to the same colour transparent, as in Figma; anything becomes a solid
/// of the colour it shows. The opacity, blend and eye stay.
pub fn set_kind(sel: &DesignSelection, key: &str, index: usize, kind: &str) -> Edit {
    if !KINDS.iter().any(|(k, _)| *k == kind) {
        return refused();
    }
    let key_owned = key.to_string();
    change(sel, key, index, move |paint| {
        let (hex, alpha) = base_colour(paint, &key_owned);
        let mut next = Map::new();
        for kept in ["opacity", "blend", "visible"] {
            if let Some(value) = paint.get(kept) {
                next.insert(kept.into(), value.clone());
            }
        }
        if kind == "solid" {
            next.insert("type".into(), json!("solid"));
            next.insert("color".into(), json!(hex));
            let own = paint.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
            next.insert("opacity".into(), json!((own * alpha).clamp(0.0, 1.0)));
        } else {
            next.insert("type".into(), json!("gradient"));
            next.insert("kind".into(), json!(kind));
            let stops = match gradient_of(paint) {
                Some(_) => Value::Array(stops_of(paint)),
                None => json!([
                    {"position": 0.0, "color": color_json(&hex, 1.0)?},
                    {"position": 1.0, "color": color_json(&hex, 0.0)?},
                ]),
            };
            next.insert("stops".into(), stops);
            let same_kind = gradient_of(paint).is_some_and(|g| g.kind == kind);
            let handles = if same_kind {
                [
                    point(paint, "from", default_handles(kind)[0]),
                    point(paint, "to", default_handles(kind)[1]),
                    point(paint, "width", default_handles(kind)[2]),
                ]
            } else {
                default_handles(kind)
            };
            set_handles(&mut next, handles);
        }
        *paint = next;
        Some(())
    })
}

/// The edit that turns a gradient row to point at `degrees` (0 right, 90 down), around the
/// middle of its start and end handles, its length and its width handle kept square to it.
pub fn set_angle(sel: &DesignSelection, key: &str, index: usize, degrees: f64) -> Edit {
    if !degrees.is_finite() {
        return refused();
    }
    change(sel, key, index, |paint| {
        gradient_of(paint)?;
        let (from, to) = (
            point(paint, "from", (0.5, 0.0)),
            point(paint, "to", (0.5, 1.0)),
        );
        let width = point(paint, "width", (0.0, 0.5));
        let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
        let half = ((to.0 - from.0).hypot(to.1 - from.1) / 2.0).max(0.01);
        let spread = (width.0 - from.0).hypot(width.1 - from.1).max(0.01);
        let (sin, cos) = degrees.to_radians().sin_cos();
        let kind = paint
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("linear");
        let start = if kind == "linear" {
            (mid.0 - cos * half, mid.1 - sin * half)
        } else {
            from
        };
        // A linear gradient turns around its middle, the others around their centre (`from`),
        // each keeping the length of its axis.
        let length = 2.0 * half;
        let end = (start.0 + cos * length, start.1 + sin * length);
        // Square to the axis, a quarter turn back, as FigCraft's own width handle.
        let side = (start.0 + sin * spread, start.1 - cos * spread);
        set_handles(paint, [start, end, side]);
        Some(())
    })
}

/// The edit that changes stop `stop` (in position order) of a gradient row: its position (0 to
/// 1), its colour (`#RRGGBB`) and its alpha, each when given.
pub fn set_stop(
    sel: &DesignSelection,
    key: &str,
    index: usize,
    stop: usize,
    position: Option<f64>,
    hex: Option<&str>,
    opacity: Option<f64>,
) -> Edit {
    if position.is_some_and(|p| !p.is_finite()) || opacity.is_some_and(|o| !o.is_finite()) {
        return refused();
    }
    change(sel, key, index, |paint| {
        let row = gradient_of(paint)?;
        let current = row.stops.get(stop)?;
        let hex = match hex {
            Some(hex) => super::parse_hex(hex)?,
            None => current.hex.clone(),
        };
        let opacity = opacity.unwrap_or(current.opacity);
        let mut sorted = row.stops.clone();
        sorted[stop] = Stop {
            position: position.unwrap_or(current.position).clamp(0.0, 1.0),
            hex,
            opacity: opacity.clamp(0.0, 1.0),
        };
        write_stops(paint, &sorted)
    })
}

/// The edit that adds a stop to a gradient row halfway along the widest gap between two stops,
/// in the colour between them.
pub fn add_stop(sel: &DesignSelection, key: &str, index: usize) -> Edit {
    change(sel, key, index, |paint| {
        let mut stops = gradient_of(paint)?.stops;
        let (at, new) = match stops.len() {
            0 => (
                0,
                Stop {
                    position: 0.5,
                    hex: "#000000".into(),
                    opacity: 1.0,
                },
            ),
            1 => {
                let only = stops[0].clone();
                (
                    1,
                    Stop {
                        position: if only.position < 0.5 { 1.0 } else { 0.0 },
                        ..only
                    },
                )
            }
            _ => {
                let widest = (1..stops.len())
                    .max_by(|a, b| {
                        let gap = |i: usize| stops[i].position - stops[i - 1].position;
                        gap(*a).total_cmp(&gap(*b))
                    })
                    .unwrap_or(1);
                let (a, b) = (&stops[widest - 1], &stops[widest]);
                (widest, mix(a, b))
            }
        };
        stops.insert(at, new);
        write_stops(paint, &stops)
    })
}

/// The edit that removes stop `stop` of a gradient row; a gradient keeps two at least.
pub fn remove_stop(sel: &DesignSelection, key: &str, index: usize, stop: usize) -> Edit {
    change(sel, key, index, |paint| {
        let mut stops = gradient_of(paint)?.stops;
        if stops.len() <= 2 || stop >= stops.len() {
            return None;
        }
        stops.remove(stop);
        write_stops(paint, &stops)
    })
}

/// The stop halfway between `a` and `b`.
fn mix(a: &Stop, b: &Stop) -> Stop {
    let (ca, cb) = (
        parse_rgb(a.hex.trim_start_matches('#')).unwrap_or([0; 3]),
        parse_rgb(b.hex.trim_start_matches('#')).unwrap_or([0; 3]),
    );
    let channel = |i: usize| ((u16::from(ca[i]) + u16::from(cb[i])) / 2) as u8;
    Stop {
        position: (a.position + b.position) / 2.0,
        hex: hex_of([channel(0), channel(1), channel(2)]),
        opacity: (a.opacity + b.opacity) / 2.0,
    }
}

fn write_stops(paint: &mut Map<String, Value>, stops: &[Stop]) -> Option<()> {
    let list = stops
        .iter()
        .map(|s| Some(json!({"position": s.position, "color": color_json(&s.hex, s.opacity)?})))
        .collect::<Option<Vec<_>>>()?;
    paint.insert("stops".into(), Value::Array(list));
    Some(())
}

pub use image::*;

mod image;

#[cfg(test)]
mod tests;
