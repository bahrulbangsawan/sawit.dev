//! The Effects section's fields beyond a shadow's and a blur's radius: a blur's progressive ramp,
//! a noise's grain, a texture's roughness and a glass's light, bend and frost. Each kind lists
//! its fields ([`fields_of`]); a field reads from the effect as the engine wrote it, under its
//! own name or its camelCase alias, and an edit replaces the one effect through `effect.set` with
//! that value changed and every other key kept.

use serde_json::{Map, Value, json};

use super::super::lists::{refused, rows};
use super::super::paints::{byte, hex_of, parse_hex, parse_rgb};
use super::super::{DesignSelection, Edit};
use super::KEY;

/// What a field holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldKind {
    /// A number from `min` to `max`; `percent` ones show 0–1 as 0–100%.
    Number { min: f64, max: f64, percent: bool },
    /// One of these values, each its label.
    Choice(&'static [(&'static str, &'static str)]),
    /// On or off.
    Toggle,
    /// A colour (`{r, g, b, a}`), shown as a hex.
    Colour,
}

/// One field of an effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    /// Its key as the engine writes it, and its camelCase alias.
    pub key: &'static str,
    pub alias: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    /// Shown only when the effect's `when.0` holds `when.1` (a noise's secondary colour for Duo).
    pub when: Option<(&'static str, &'static str)>,
}

const fn number(
    key: &'static str,
    alias: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
) -> Field {
    Field {
        key,
        alias,
        label,
        kind: FieldKind::Number {
            min,
            max,
            percent: false,
        },
        when: None,
    }
}

const fn percent(key: &'static str, alias: &'static str, label: &'static str) -> Field {
    Field {
        key,
        alias,
        label,
        kind: FieldKind::Number {
            min: 0.0,
            max: 1.0,
            percent: true,
        },
        when: None,
    }
}

const BLUR_TYPES: &[(&str, &str)] = &[("uniform", "Uniform"), ("progressive", "Progressive")];
const NOISE_MODES: &[(&str, &str)] = &[("mono", "Mono"), ("duo", "Duo"), ("multi", "Multi")];

const BLUR: &[Field] = &[
    Field {
        key: "blur_type",
        alias: "blurType",
        label: "Type",
        kind: FieldKind::Choice(BLUR_TYPES),
        when: None,
    },
    Field {
        when: Some(("blur_type", "progressive")),
        ..number("start_radius", "startRadius", "Start blur", 0.0, 250.0)
    },
];

const NOISE: &[Field] = &[
    Field {
        key: "mode",
        alias: "mode",
        label: "Mode",
        kind: FieldKind::Choice(NOISE_MODES),
        when: None,
    },
    number("size", "size", "Size", 0.1, 100.0),
    percent("density", "density", "Density"),
    Field {
        key: "color",
        alias: "color",
        label: "Colour",
        kind: FieldKind::Colour,
        when: None,
    },
    Field {
        key: "secondary",
        alias: "secondary",
        label: "Second colour",
        kind: FieldKind::Colour,
        when: Some(("mode", "duo")),
    },
    Field {
        when: Some(("mode", "multi")),
        ..percent("opacity", "opacity", "Opacity")
    },
];

const TEXTURE: &[Field] = &[
    number("size", "size", "Size", 0.1, 100.0),
    number("radius", "radius", "Radius", 0.0, 100.0),
    Field {
        key: "clip_to_shape",
        alias: "clipToShape",
        label: "Clip to shape",
        kind: FieldKind::Toggle,
        when: None,
    },
];

const GLASS: &[Field] = &[
    number("light_angle", "lightAngle", "Light angle", 0.0, 360.0),
    percent("light_intensity", "lightIntensity", "Light"),
    percent("refraction", "refraction", "Refraction"),
    number("depth", "depth", "Depth", 1.0, 1000.0),
    percent("dispersion", "dispersion", "Dispersion"),
    number("frost", "frost", "Frost", 0.0, 100.0),
    percent("splay", "splay", "Splay"),
];

/// The fields of an effect of type `tag` beyond those the row shows (a shadow's and a blur's
/// radius): none for a shadow.
pub fn fields_of(tag: &str) -> &'static [Field] {
    match tag {
        "layerBlur" | "backgroundBlur" => BLUR,
        "noise" => NOISE,
        "texture" => TEXTURE,
        "glass" => GLASS,
        _ => &[],
    }
}

/// Effect `index` as read, when every layer holds the same list.
fn effect_at(sel: &DesignSelection, index: usize) -> Option<Map<String, Value>> {
    rows(sel, KEY)?.get(index)?.as_object().cloned()
}

/// The fields effect `index` shows now: its kind's, less those that apply to another mode.
pub fn shown_fields(sel: &DesignSelection, index: usize) -> Vec<Field> {
    let Some(effect) = effect_at(sel, index) else {
        return Vec::new();
    };
    let tag = effect
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    fields_of(tag)
        .iter()
        .filter(|field| match field.when {
            None => true,
            Some((key, value)) => text(&effect, key, &camel(key)).as_deref() == Some(value),
        })
        .copied()
        .collect()
}

/// `key` in camelCase: `blur_type` is `blurType`.
fn camel(key: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in key.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn get<'a>(effect: &'a Map<String, Value>, key: &str, alias: &str) -> Option<&'a Value> {
    effect.get(key).or_else(|| effect.get(alias))
}

fn text(effect: &Map<String, Value>, key: &str, alias: &str) -> Option<String> {
    Some(get(effect, key, alias)?.as_str()?.to_ascii_lowercase())
}

/// The name a field is written under: the one the effect holds it under, else its own.
fn written_key(effect: &Map<String, Value>, field: &Field) -> String {
    if !effect.contains_key(field.key) && effect.contains_key(field.alias) {
        field.alias.to_string()
    } else {
        field.key.to_string()
    }
}

/// What field `field` of effect `index` shows: a number as typed (a percent as 0–100), a choice's
/// value, `"true"` or `"false"`, a colour's `#RRGGBB`.
pub fn value(sel: &DesignSelection, index: usize, field: &Field) -> Option<String> {
    let effect = effect_at(sel, index)?;
    let held = get(&effect, field.key, field.alias);
    Some(match field.kind {
        FieldKind::Number { percent, .. } => {
            let n = held.and_then(Value::as_f64).unwrap_or(0.0);
            let shown = if percent { n * 100.0 } else { n };
            trimmed((shown * 100.0).round() / 100.0)
        }
        FieldKind::Choice(options) => held
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| options[0].0.to_string()),
        FieldKind::Toggle => held.and_then(Value::as_bool).unwrap_or(false).to_string(),
        FieldKind::Colour => {
            let c = held?;
            let channel = |name: &str| c.get(name).and_then(Value::as_f64);
            hex_of([
                byte(channel("r")?),
                byte(channel("g")?),
                byte(channel("b")?),
            ])
        }
    })
}

/// The edit that gives field `field` of effect `index` what was typed or picked: a number in its
/// range (a percent as 0–100), a choice's value, `"true"` / `"false"`, or a colour, which keeps its
/// alpha. Refused when it is no good.
pub fn set_value(sel: &DesignSelection, index: usize, field: &Field, typed: &str) -> Edit {
    let Some(mut effect) = effect_at(sel, index) else {
        return refused();
    };
    let typed = typed.trim();
    let new = match field.kind {
        FieldKind::Number { min, max, percent } => {
            let Ok(n) = typed.trim_end_matches(['%', '°']).parse::<f64>() else {
                return refused();
            };
            let n = if percent { n / 100.0 } else { n };
            if !n.is_finite() || n < min || n > max {
                return refused();
            }
            json!(n)
        }
        FieldKind::Choice(options) => {
            if !options.iter().any(|(v, _)| *v == typed) {
                return refused();
            }
            json!(typed)
        }
        FieldKind::Toggle => json!(typed == "true"),
        FieldKind::Colour => {
            let Some(hex) = parse_hex(typed) else {
                return refused();
            };
            let Some([r, g, b]) = parse_rgb(hex.trim_start_matches('#')) else {
                return refused();
            };
            let alpha = get(&effect, field.key, field.alias)
                .and_then(|c| c.get("a"))
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let unit = |c: u8| f64::from(c) / 255.0;
            json!({"r": unit(r), "g": unit(g), "b": unit(b), "a": alpha})
        }
    };
    let key = written_key(&effect, field);
    effect.insert(key, new);
    Edit {
        command: "effect.set",
        params: json!({"ids": sel.ids, "index": index, "effect": Value::Object(effect)}),
    }
}

fn trimmed(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests;
