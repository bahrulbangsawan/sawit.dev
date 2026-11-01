//! The Effects section's model: a layer's effects as rows, and the edits that change them.
//!
//! `effects` is a list of effects, each `{type, ...}` with the keys its kind takes. A shadow has a
//! `color` (`{r, g, b, a}`), an `offset` (`[x, y]`), a `radius` (its blur), a `spread`, a `blend`
//! and `showBehind`; a blur has a `radius` and its progressive settings. The engine has three
//! kinds more (noise, texture and glass) that the panel lists, hides and removes but doesn't edit.
//! An edit that changes or adds an effect sends the whole list back through `node.set`, so every
//! effect it doesn't change goes back as it was read, and an effect the panel can't edit is
//! neither lost nor rewritten. [`remove_effect`] uses the engine's command for one effect, whose
//! variable bindings follow the effects that move (`node.set` would leave them by place).
//!
//! Where the layers hold different lists there is no row to change. [`add_effect`] then makes the
//! one new effect the only effect of every layer, and the other edits are refused.
//!
//! An edit is made from the selection it is given and sends that selection's whole list, so make
//! each edit from a selection read after the last one.

use serde_json::{Map, Value, json};

use super::lists::{change_row, remove_row, replace, rows};
use super::paints::{byte, hex_of, parse_rgb};
use super::{DesignSelection, Edit};

/// The property the effects are held in.
const KEY: &str = "effects";

/// The kind of an effect row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectKind {
    DropShadow,
    InnerShadow,
    LayerBlur,
    BackgroundBlur,
    /// An effect the panel doesn't edit, by its type tag: `noise`, `texture` or `glass`. Its row
    /// can be hidden, shown and removed, and goes back to the engine untouched.
    Other(String),
}

/// One effect of a layer's list.
///
/// A shadow uses every field. A blur uses `blur`, its radius, and `visible`; and a row of kind
/// [`EffectKind::Other`] uses `visible` alone. The fields a kind doesn't use read as 0, `#000000`
/// and 1, and are ignored when the row is written.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectRow {
    pub kind: EffectKind,
    /// The eye: whether the effect shows.
    pub visible: bool,
    /// A shadow's offset.
    pub x: f64,
    pub y: f64,
    /// A shadow's or a blur's radius.
    pub blur: f64,
    /// A shadow's spread.
    pub spread: f64,
    /// A shadow's colour as `#RRGGBB` in upper case.
    pub hex: String,
    /// A shadow's colour alpha, 0 to 1.
    pub opacity: f64,
}

/// How the keys of an effect are laid out, which decides what a row edits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `color`, `offset`, `radius`, `spread`, `blend` and `showBehind`.
    Shadow,
    /// `radius` and the progressive settings.
    Blur,
    /// A kind this model doesn't edit.
    Other,
}

impl EffectKind {
    /// The kind of the effect with type tag `tag`.
    fn of(tag: &str) -> Self {
        match tag {
            "dropShadow" => Self::DropShadow,
            "innerShadow" => Self::InnerShadow,
            "layerBlur" => Self::LayerBlur,
            "backgroundBlur" => Self::BackgroundBlur,
            other => Self::Other(other.to_string()),
        }
    }

    /// The type tag the engine knows the kind by.
    fn tag(&self) -> &str {
        match self {
            Self::DropShadow => "dropShadow",
            Self::InnerShadow => "innerShadow",
            Self::LayerBlur => "layerBlur",
            Self::BackgroundBlur => "backgroundBlur",
            Self::Other(tag) => tag,
        }
    }

    fn shape(&self) -> Shape {
        match self {
            Self::DropShadow | Self::InnerShadow => Shape::Shadow,
            Self::LayerBlur | Self::BackgroundBlur => Shape::Blur,
            Self::Other(_) => Shape::Other,
        }
    }

    /// Whether an effect of this kind can become one of kind `to` and keep what it holds: a shadow
    /// the other kind of shadow, a blur the other kind of blur, and an effect the panel doesn't
    /// edit only itself.
    fn keeps_its_keys_as(&self, to: &Self) -> bool {
        match (self.shape(), to.shape()) {
            (Shape::Other, Shape::Other) => self == to,
            (from, to) => from == to,
        }
    }
}

/// The effects of the layers, every one of them, in the order the engine holds them: row 0 applies
/// first and the last row is on top. FigCraft's own panel shows them in reverse, the top row
/// first; the `index` the edits below take is the position in this list either way. A kind the
/// panel doesn't edit is an [`EffectKind::Other`] row in its place. `None` when the selected
/// layers hold different lists. A layer without an effect holds an empty list, which is a list
/// like any other: `Some` of no rows.
pub fn effects(sel: &DesignSelection) -> Option<Vec<EffectRow>> {
    Some(rows(sel, KEY)?.iter().map(effect_row).collect())
}

/// The edit that adds an effect of `kind`, with the engine's defaults for that kind, after the
/// effects there are. When the layers hold different lists there is none to add to, and the new
/// effect becomes the only one of every layer: the "Replace all" of a mixed list.
pub fn add_effect(sel: &DesignSelection, kind: EffectKind) -> Edit {
    let mut list = rows(sel, KEY).unwrap_or_default();
    list.push(json!({"type": kind.tag()}));
    replace(sel, KEY, list)
}

/// The edit that makes effect `index` what `row` says. The effects around it, and the keys of the
/// effect that the row doesn't show (a shadow's blend, a blur's progressive settings), stay as
/// they were.
///
/// Changing the kind of the row changes the effect: a shadow can become the other kind of shadow,
/// and a blur the other kind of blur, keeping everything; any other change starts from the
/// engine's defaults for the new kind and keeps only the radius and the eye.
///
/// A shadow colour that the row still shows as it was read is kept exactly as stored, however many
/// bits it has; the hex is only rounded to 8 bits when it changes.
///
/// A row that isn't there, lists that differ from layer to layer, a shadow row whose `hex` isn't
/// a colour or whose numbers aren't finite, and a blur row whose radius isn't finite make an
/// edit the engine refuses and that changes nothing. Check typed input first:
/// [`parse_hex`](super::paints::parse_hex).
pub fn set_effect(sel: &DesignSelection, index: usize, row: &EffectRow) -> Edit {
    change_row(sel, KEY, index, |old| written(old, row))
}

/// The edit that removes effect `index`, with the engine's `effect.remove`: the variable bindings
/// of the effects after it move with them, and none is left on the effect that moves into its
/// place. A row that isn't there, or lists that differ from layer to layer, make an edit the
/// engine refuses and that changes nothing.
pub fn remove_effect(sel: &DesignSelection, index: usize) -> Edit {
    remove_row(sel, KEY, index, "effect.remove")
}

/// What the row for `effect` shows.
fn effect_row(effect: &Value) -> EffectRow {
    let kind = EffectKind::of(
        effect
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let number = |key: &str| effect.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let mut row = EffectRow {
        kind,
        visible: effect
            .get("visible")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        x: 0.0,
        y: 0.0,
        blur: 0.0,
        spread: 0.0,
        hex: hex_of([0, 0, 0]),
        opacity: 1.0,
    };
    match row.kind.shape() {
        Shape::Shadow => {
            let offset = effect.get("offset").and_then(Value::as_array);
            let at = |i: usize| {
                let coordinate = offset.and_then(|pair| pair.get(i));
                coordinate.and_then(Value::as_f64).unwrap_or(0.0)
            };
            row.x = at(0);
            row.y = at(1);
            row.blur = number("radius");
            row.spread = number("spread");
            if let Some(color) = effect.get("color") {
                if let Some(rgb) = stored_rgb(color) {
                    row.hex = hex_of(rgb.map(byte));
                }
                row.opacity = color.get("a").and_then(Value::as_f64).unwrap_or(1.0);
            }
        }
        Shape::Blur => row.blur = number("radius"),
        Shape::Other => {}
    }
    row
}

/// Effect `old` as `row` says, or `None` when the row can't be written (see [`set_effect`]).
fn written(old: &Value, row: &EffectRow) -> Option<Value> {
    let old_kind = EffectKind::of(old.get("type").and_then(Value::as_str).unwrap_or_default());
    let in_place = old_kind.keeps_its_keys_as(&row.kind);
    // A new kind starts from nothing, and the engine fills in its defaults.
    let mut effect = if in_place {
        old.as_object().cloned().unwrap_or_default()
    } else {
        Map::new()
    };
    effect.insert("type".into(), json!(row.kind.tag()));
    effect.insert("visible".into(), json!(row.visible));
    match row.kind.shape() {
        Shape::Shadow => {
            let rgb = parse_rgb(&row.hex)?;
            let numbers = [row.x, row.y, row.blur, row.spread, row.opacity];
            if !numbers.iter().all(|number| number.is_finite()) {
                return None;
            }
            effect.insert("radius".into(), json!(row.blur));
            if in_place {
                effect.insert("offset".into(), json!([row.x, row.y]));
                effect.insert("spread".into(), json!(row.spread));
                let color = shadow_color(old.get("color"), rgb, row.opacity);
                effect.insert("color".into(), color);
            }
        }
        Shape::Blur => {
            if !row.blur.is_finite() {
                return None;
            }
            effect.insert("radius".into(), json!(row.blur));
        }
        Shape::Other => {}
    }
    Some(Value::Object(effect))
}

/// A shadow's `color` for the hex bytes `rgb` and the alpha `opacity`. The colour the shadow
/// already holds stays exactly as stored while its hex is the one asked for, so that a colour with
/// more than 8 bits to a channel is rounded only when it changes.
fn shadow_color(stored: Option<&Value>, rgb: [u8; 3], opacity: f64) -> Value {
    let kept = stored
        .and_then(stored_rgb)
        .filter(|channels| channels.map(byte) == rgb);
    let [r, g, b] = kept.unwrap_or_else(|| rgb.map(|channel| f64::from(channel) / 255.0));
    json!({"r": r, "g": g, "b": b, "a": opacity.clamp(0.0, 1.0)})
}

/// The red, green and blue of a stored colour `{r, g, b, a}`, as fractions of 1.
fn stored_rgb(color: &Value) -> Option<[f64; 3]> {
    let channel = |name: &str| color.get(name)?.as_f64();
    Some([channel("r")?, channel("g")?, channel("b")?])
}

#[cfg(test)]
mod tests;
