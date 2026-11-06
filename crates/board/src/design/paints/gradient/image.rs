//! Image paints in the Fill and Stroke sections: an image row's fill mode, tile scale, quarter
//! turns and adjustments, and the edits that change them (see [`super`]).

use serde_json::{Map, Value, json};

use super::super::super::lists::refused;
use super::{DesignSelection, Edit, change, paint_at, round};

/// What an image row's editor shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRow {
    /// `fill`, `fit`, `crop` or `tile`.
    pub scale_mode: String,
    /// The tile scale, 1 being the image's own size.
    pub scale: f64,
    /// Turns of 90° clockwise, 0 to 3.
    pub rotation: i64,
    /// Each of [`ADJUSTMENTS`], in its order.
    pub adjustments: Vec<f64>,
}

/// The image adjustments, each -1 to 1 (0 leaves the image as it is).
pub const ADJUSTMENTS: [(&str, &str); 7] = [
    ("exposure", "Exposure"),
    ("contrast", "Contrast"),
    ("saturation", "Saturation"),
    ("temperature", "Temperature"),
    ("tint", "Tint"),
    ("highlights", "Highlights"),
    ("shadows", "Shadows"),
];

/// The image fill modes, as the engine names them and as the panel shows them.
pub const SCALE_MODES: [(&str, &str); 4] = [
    ("fill", "Fill"),
    ("fit", "Fit"),
    ("crop", "Crop"),
    ("tile", "Tile"),
];

/// An image row's editor, or `None` when row `index` isn't an image.
pub fn image(sel: &DesignSelection, key: &str, index: usize) -> Option<ImageRow> {
    let paint = paint_at(sel, key, index)?;
    if paint.get("type").and_then(Value::as_str) != Some("image") {
        return None;
    }
    let get = |a: &str, b: &str| paint.get(a).or_else(|| paint.get(b));
    let filters = get("filters", "filters");
    Some(ImageRow {
        scale_mode: get("scaleMode", "scale_mode")
            .and_then(Value::as_str)
            .unwrap_or("fill")
            .to_ascii_lowercase(),
        scale: get("scale", "scale").and_then(Value::as_f64).unwrap_or(1.0),
        rotation: get("rotation", "rotation")
            .and_then(Value::as_f64)
            .map_or(0, |r| (r as i64).rem_euclid(4)),
        adjustments: ADJUSTMENTS
            .iter()
            .map(|(name, _)| {
                filters
                    .and_then(|f| f.get(*name))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
            })
            .collect(),
    })
}

/// The key an image paint keeps `camel` under, as it was read (`scaleMode`, or `scale_mode`).
fn image_key(paint: &Map<String, Value>, camel: &'static str, snake: &'static str) -> &'static str {
    if paint.contains_key(snake) && !paint.contains_key(camel) {
        snake
    } else {
        camel
    }
}

/// The edit that gives an image row the fill mode `mode` (one of [`SCALE_MODES`]).
pub fn set_scale_mode(sel: &DesignSelection, key: &str, index: usize, mode: &str) -> Edit {
    if !SCALE_MODES.iter().any(|(m, _)| *m == mode) {
        return refused();
    }
    change(sel, key, index, |paint| {
        image(sel, key, index)?;
        let k = image_key(paint, "scaleMode", "scale_mode");
        paint.insert(k.into(), json!(mode));
        Some(())
    })
}

/// The edit that gives a tiled image row the tile scale `scale` (0.01 to 100).
pub fn set_tile_scale(sel: &DesignSelection, key: &str, index: usize, scale: f64) -> Edit {
    if !scale.is_finite() || !(0.01..=100.0).contains(&scale) {
        return refused();
    }
    change(sel, key, index, |paint| {
        image(sel, key, index)?;
        paint.insert("scale".into(), json!(scale));
        Some(())
    })
}

/// The edit that turns an image row a quarter turn clockwise.
pub fn rotate_image(sel: &DesignSelection, key: &str, index: usize) -> Edit {
    change(sel, key, index, |paint| {
        let row = image(sel, key, index)?;
        paint.insert("rotation".into(), json!((row.rotation + 1).rem_euclid(4)));
        Some(())
    })
}

/// The edit that sets adjustment `name` (one of [`ADJUSTMENTS`]) of an image row to `value`,
/// -1 to 1.
pub fn set_adjustment(
    sel: &DesignSelection,
    key: &str,
    index: usize,
    name: &str,
    value: f64,
) -> Edit {
    if !value.is_finite() || !ADJUSTMENTS.iter().any(|(n, _)| *n == name) {
        return refused();
    }
    change(sel, key, index, |paint| {
        image(sel, key, index)?;
        let mut filters = paint
            .get("filters")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        filters.insert(name.into(), json!(round(value.clamp(-1.0, 1.0), 3)));
        paint.insert("filters".into(), Value::Object(filters));
        Some(())
    })
}

/// The edit that sets every adjustment of an image row back to 0.
pub fn reset_adjustments(sel: &DesignSelection, key: &str, index: usize) -> Edit {
    change(sel, key, index, |paint| {
        image(sel, key, index)?;
        paint.remove("filters");
        Some(())
    })
}
