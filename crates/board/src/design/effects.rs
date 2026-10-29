//! The Effects section's model: a layer's effects as rows, and the edits that change them.
//!
//! `effects` is a list of effects, each `{type, ...}` with the keys its kind takes. A shadow has a
//! `color` (`{r, g, b, a}`), an `offset` (`[x, y]`), a `radius` (its blur), a `spread`, a `blend`
//! and `showBehind`; a blur has a `radius` and its progressive settings. The engine has three
//! kinds more (noise, texture and glass) that the panel lists, hides and removes but doesn't edit.
//! An edit sends the whole list back through `node.set`, so every effect it doesn't change goes
//! back as it was read, and an effect the panel can't edit is neither lost nor rewritten.
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

/// The edit that removes effect `index`. A row that isn't there, or lists that differ from layer
/// to layer, make an edit the engine refuses and that changes nothing.
pub fn remove_effect(sel: &DesignSelection, index: usize) -> Edit {
    remove_row(sel, KEY, index)
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
mod tests {
    use figcraft_color::{BlendMode, Color};
    use figcraft_doc::{BlurType, Effect, EffectKind as Held, NodeId};
    use serde_json::json;

    use super::*;
    use crate::design::lists::testing::{
        apply, design_file, edit, node_set, read, rectangle, undo_steps,
    };
    use crate::doc::{BoardDoc, BoardError};

    /// The effects the engine holds for layer `id`.
    fn held(board: &BoardDoc, id: NodeId) -> Vec<Effect> {
        board.doc().unwrap().node(id).unwrap().effects.clone()
    }

    fn rows_of(board: &BoardDoc, ids: &[NodeId]) -> Vec<EffectRow> {
        effects(&read(board, ids)).unwrap()
    }

    /// A row of `kind` with the fields a kind that doesn't use them reads as.
    fn row(kind: EffectKind) -> EffectRow {
        EffectRow {
            kind,
            visible: true,
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            spread: 0.0,
            hex: "#000000".into(),
            opacity: 1.0,
        }
    }

    fn other(tag: &str) -> EffectKind {
        EffectKind::Other(tag.into())
    }

    /// A layer with `effects`, given as `node.set` takes them.
    fn with_effects(board: &mut BoardDoc, effects: serde_json::Value) -> NodeId {
        let id = rectangle(board, "R", 0.0);
        node_set(board, &[id], json!({ "effects": effects }));
        id
    }

    #[test]
    fn drop_shadow_round_trip() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        // A rectangle starts without an effect: an empty list, not a mixed one.
        assert_eq!(effects(&read(&board, &[r])), Some(vec![]));

        let edit = add_effect(&read(&board, &[r]), EffectKind::DropShadow);
        assert_eq!(edit.command, "node.set");
        apply(&mut board, &edit);
        let sel = read(&board, &[r]);
        let mut rows = effects(&sel).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, EffectKind::DropShadow);
        assert!(rows[0].visible);

        let mut shadow = rows.remove(0);
        shadow.x = 4.0;
        shadow.y = 4.0;
        shadow.blur = 8.0;
        shadow.hex = "#000000".into();
        shadow.opacity = 0.25;
        apply(&mut board, &set_effect(&sel, 0, &shadow));

        assert_eq!(effects(&read(&board, &[r])), Some(vec![shadow.clone()]));
        // The engine holds what the row says.
        let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
            panic!("{:?}", held(&board, r));
        };
        assert_eq!(held_shadow.offset, (4.0, 4.0));
        assert_eq!(held_shadow.radius, 8.0);
        assert_eq!(held_shadow.spread, 0.0);
        assert_eq!(held_shadow.color, Color::new(0.0, 0.0, 0.0, 0.25));
    }

    #[test]
    fn a_shadow_colour_and_spread_round_trip() {
        let mut board = design_file();
        let r = rectangle(&mut board, "R", 0.0);
        edit(&mut board, &[r], |sel| {
            add_effect(sel, EffectKind::InnerShadow)
        });
        let sel = read(&board, &[r]);
        let mut shadow = effects(&sel).unwrap().remove(0);
        assert_eq!(shadow.kind, EffectKind::InnerShadow);
        shadow.x = -2.0;
        shadow.y = 6.5;
        shadow.blur = 12.0;
        shadow.spread = -3.0;
        shadow.hex = "#ff8800".into();
        shadow.opacity = 0.3;
        apply(&mut board, &set_effect(&sel, 0, &shadow));

        let back = rows_of(&board, &[r]).remove(0);
        // A colour is stored in single precision: 30% reads back as the nearest float.
        assert!((back.opacity - 0.3).abs() < 1e-6, "{}", back.opacity);
        assert_eq!(
            back,
            EffectRow {
                hex: "#FF8800".into(),
                opacity: back.opacity,
                ..shadow
            }
        );
    }

    #[test]
    fn add_effect_gives_each_kind_the_engines_defaults() {
        let cases = [
            (EffectKind::DropShadow, Effect::drop_shadow()),
            (EffectKind::InnerShadow, Effect::inner_shadow()),
            (EffectKind::LayerBlur, Effect::layer_blur()),
            (EffectKind::BackgroundBlur, Effect::background_blur()),
            (other("noise"), Effect::noise()),
            (other("texture"), Effect::texture()),
            (other("glass"), Effect::glass()),
        ];
        for (kind, engine) in cases {
            let mut board = design_file();
            let r = rectangle(&mut board, "R", 0.0);
            edit(&mut board, &[r], |sel| add_effect(sel, kind.clone()));
            assert_eq!(held(&board, r), [engine], "{kind:?}");
            assert_eq!(rows_of(&board, &[r])[0].kind, kind);
        }
    }

    #[test]
    fn rows_read_what_each_kind_holds() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([
                {"type": "dropShadow", "color": {"r": 1, "g": 0, "b": 0, "a": 0.5}, "offset": [2, 3], "radius": 5, "spread": 1},
                {"type": "layerBlur", "radius": 9, "visible": false},
                {"type": "backgroundBlur", "radius": 20},
            ]),
        );
        assert_eq!(
            rows_of(&board, &[r]),
            [
                EffectRow {
                    kind: EffectKind::DropShadow,
                    visible: true,
                    x: 2.0,
                    y: 3.0,
                    blur: 5.0,
                    spread: 1.0,
                    hex: "#FF0000".into(),
                    opacity: 0.5,
                },
                EffectRow {
                    visible: false,
                    blur: 9.0,
                    ..row(EffectKind::LayerBlur)
                },
                EffectRow {
                    blur: 20.0,
                    ..row(EffectKind::BackgroundBlur)
                },
            ]
        );
    }

    /// A glass, noise or texture effect is a row of its own kind and stays what it is.
    #[test]
    fn editing_a_shadow_keeps_a_glass_neighbour() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{"type": "glass"}, {"type": "dropShadow"}, {"type": "noise"}, {"type": "texture"}]),
        );
        let before = held(&board, r);
        let sel = read(&board, &[r]);
        let rows = effects(&sel).unwrap();
        let kinds: Vec<_> = rows.iter().map(|row| row.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                other("glass"),
                EffectKind::DropShadow,
                other("noise"),
                other("texture")
            ],
            "every effect is listed, in order"
        );

        let mut shadow = rows[1].clone();
        shadow.x = 6.0;
        shadow.blur = 12.0;
        apply(&mut board, &set_effect(&sel, 1, &shadow));
        let after = held(&board, r);
        assert_eq!(after.len(), 4);
        for neighbour in [0, 2, 3] {
            assert_eq!(after[neighbour], before[neighbour], "effect {neighbour}");
        }
        assert_ne!(after[1], before[1]);
        assert_eq!(rows_of(&board, &[r])[1].x, 6.0);

        // Removing the shadow leaves them as well, and so does adding another effect.
        let neighbours = vec![before[0].clone(), before[2].clone(), before[3].clone()];
        edit(&mut board, &[r], |sel| remove_effect(sel, 1));
        assert_eq!(held(&board, r), neighbours);
        edit(&mut board, &[r], |sel| {
            add_effect(sel, EffectKind::LayerBlur)
        });
        let after = held(&board, r);
        assert_eq!(after.len(), 4);
        assert_eq!(after[..3], neighbours[..]);
        assert_eq!(after[3], Effect::layer_blur());
    }

    #[test]
    fn a_row_the_panel_cannot_edit_changes_only_its_eye() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{"type": "glass"}, {"type": "noise", "mode": "duo"}]),
        );
        let before = held(&board, r);
        let sel = read(&board, &[r]);

        // The eye of the noise.
        let mut noise = effects(&sel).unwrap()[1].clone();
        noise.visible = false;
        apply(&mut board, &set_effect(&sel, 1, &noise));
        let after = held(&board, r);
        assert_eq!(after[0], before[0]);
        assert!(!after[1].visible);
        assert_eq!(after[1].kind, before[1].kind, "everything else is the same");
        assert!(!rows_of(&board, &[r])[1].visible);

        // The fields it doesn't use are ignored, not written.
        let sel = read(&board, &[r]);
        let noisy = EffectRow {
            visible: true,
            x: 9.0,
            blur: 7.0,
            hex: "#FF0000".into(),
            opacity: 0.1,
            ..noise
        };
        apply(&mut board, &set_effect(&sel, 1, &noisy));
        assert_eq!(held(&board, r), before);
    }

    #[test]
    fn hidden_effects_stay_listed() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{"type": "dropShadow", "visible": false}]),
        );
        let rows = rows_of(&board, &[r]);
        assert!(!rows[0].visible);
        assert_eq!(rows[0].kind, EffectKind::DropShadow);

        let mut shown = rows[0].clone();
        shown.visible = true;
        edit(&mut board, &[r], |sel| set_effect(sel, 0, &shown));
        assert_eq!(held(&board, r), [Effect::drop_shadow()]);
    }

    #[test]
    fn a_shadow_edit_keeps_its_blend_and_show_behind() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{"type": "dropShadow", "blend": "multiply", "showBehind": true}]),
        );
        let sel = read(&board, &[r]);
        let mut shadow = effects(&sel).unwrap().remove(0);
        shadow.x = 5.0;
        apply(&mut board, &set_effect(&sel, 0, &shadow));

        let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
            panic!("{:?}", held(&board, r));
        };
        assert_eq!(held_shadow.offset.0, 5.0);
        assert_eq!(held_shadow.blend, BlendMode::Multiply);
        assert!(held_shadow.show_behind);
    }

    #[test]
    fn a_blur_edit_keeps_its_progressive_settings() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{
                "type": "layerBlur", "radius": 10, "blurType": "progressive",
                "startRadius": 2, "start": [0.5, 0.1], "end": [0.5, 0.9],
            }]),
        );
        let sel = read(&board, &[r]);
        let mut blur = effects(&sel).unwrap().remove(0);
        assert_eq!(blur.blur, 10.0);
        blur.blur = 20.0;
        apply(&mut board, &set_effect(&sel, 0, &blur));

        let held_blur = held(&board, r)[0].blur().unwrap().clone();
        assert_eq!(held_blur.radius, 20.0);
        assert_eq!(held_blur.blur_type, BlurType::Progressive);
        assert_eq!(held_blur.start_radius, 2.0);
        assert_eq!(held_blur.start, (0.5, 0.1));
        assert_eq!(held_blur.end, (0.5, 0.9));
    }

    #[test]
    fn a_stored_shadow_colour_is_not_rounded_by_an_edit_that_leaves_it_alone() {
        let mut board = design_file();
        // Not a colour of 8 bits, as the engine stores it: JSON numbers are doubles, colours floats.
        let color = Color::new(0.123_f64 as f32, 0.456_f64 as f32, 0.789_f64 as f32, 0.5);
        let r = with_effects(
            &mut board,
            json!([{"type": "dropShadow", "color": {"r": 0.123, "g": 0.456, "b": 0.789, "a": 0.5}}]),
        );
        let sel = read(&board, &[r]);
        let mut shadow = effects(&sel).unwrap().remove(0);
        assert_eq!(shadow.hex, "#1F74C9");

        // The offset changes, and the colour as the row still shows it stays as stored.
        shadow.x = 3.0;
        apply(&mut board, &set_effect(&sel, 0, &shadow));
        let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
            panic!("{:?}", held(&board, r));
        };
        assert_eq!(held_shadow.offset.0, 3.0);
        assert_eq!(held_shadow.color, color);

        // A colour that was typed is the colour, to the bit.
        shadow.hex = "#1F74CA".into();
        edit(&mut board, &[r], |sel| set_effect(sel, 0, &shadow));
        let Held::DropShadow(held_shadow) = &held(&board, r)[0].kind else {
            panic!("{:?}", held(&board, r));
        };
        assert_eq!(held_shadow.color.to_rgba8(), [0x1F, 0x74, 0xCA, 128]);
    }

    #[test]
    fn changing_the_kind_of_a_shadow_or_a_blur_keeps_everything() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([
                {"type": "dropShadow", "offset": [4, 5], "radius": 8, "spread": 2,
                 "color": {"r": 1, "g": 0, "b": 0, "a": 0.5}, "blend": "multiply"},
                {"type": "layerBlur", "radius": 10, "blurType": "progressive", "startRadius": 2},
            ]),
        );
        let sel = read(&board, &[r]);
        let rows = effects(&sel).unwrap();
        let mut inner = rows[0].clone();
        inner.kind = EffectKind::InnerShadow;
        let mut background = rows[1].clone();
        background.kind = EffectKind::BackgroundBlur;
        apply(&mut board, &set_effect(&sel, 0, &inner));
        edit(&mut board, &[r], |sel| set_effect(sel, 1, &background));

        let after = held(&board, r);
        let Held::InnerShadow(shadow) = &after[0].kind else {
            panic!("{after:?}");
        };
        assert_eq!(shadow.offset, (4.0, 5.0));
        assert_eq!((shadow.radius, shadow.spread), (8.0, 2.0));
        assert_eq!(shadow.color, Color::new(1.0, 0.0, 0.0, 0.5));
        assert_eq!(shadow.blend, BlendMode::Multiply);
        let Held::BackgroundBlur(blur) = &after[1].kind else {
            panic!("{after:?}");
        };
        assert_eq!(blur.radius, 10.0);
        assert_eq!(blur.blur_type, BlurType::Progressive);
        assert_eq!(blur.start_radius, 2.0);
    }

    #[test]
    fn changing_a_shadow_into_a_blur_or_back_starts_from_the_defaults_and_keeps_the_radius() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{"type": "dropShadow", "offset": [4, 5], "radius": 8, "visible": false}]),
        );
        let sel = read(&board, &[r]);
        let mut blur = effects(&sel).unwrap().remove(0);
        blur.kind = EffectKind::LayerBlur;
        apply(&mut board, &set_effect(&sel, 0, &blur));
        let mut expected = Effect::layer_blur();
        expected.blur_mut().unwrap().radius = 8.0;
        expected.visible = false;
        assert_eq!(held(&board, r), [expected]);

        let sel = read(&board, &[r]);
        let mut shadow = effects(&sel).unwrap().remove(0);
        assert_eq!(shadow.kind, EffectKind::LayerBlur);
        shadow.kind = EffectKind::InnerShadow;
        apply(&mut board, &set_effect(&sel, 0, &shadow));
        let rows = rows_of(&board, &[r]);
        assert_eq!(
            rows,
            [EffectRow {
                kind: EffectKind::InnerShadow,
                visible: false,
                x: 0.0,
                y: 4.0,
                blur: 8.0,
                spread: 0.0,
                hex: "#000000".into(),
                opacity: 0.25,
            }],
            "the engine's inner shadow, at the radius the blur had"
        );

        // To an effect the panel doesn't edit: its defaults, and the eye.
        let sel = read(&board, &[r]);
        let mut glass = rows[0].clone();
        glass.kind = other("glass");
        apply(&mut board, &set_effect(&sel, 0, &glass));
        let mut expected = Effect::glass();
        expected.visible = false;
        assert_eq!(held(&board, r), [expected]);

        // One of those into another: nothing of the glass goes into the noise.
        let sel = read(&board, &[r]);
        let mut noise = effects(&sel).unwrap().remove(0);
        noise.kind = other("noise");
        apply(&mut board, &set_effect(&sel, 0, &noise));
        let mut expected = Effect::noise();
        expected.visible = false;
        assert_eq!(held(&board, r), [expected]);
    }

    #[test]
    fn remove_effect_leaves_the_others() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{"type": "dropShadow"}, {"type": "layerBlur"}, {"type": "backgroundBlur"}]),
        );
        let before = held(&board, r);
        edit(&mut board, &[r], |sel| remove_effect(sel, 1));
        assert_eq!(held(&board, r), vec![before[0].clone(), before[2].clone()]);
        edit(&mut board, &[r], |sel| remove_effect(sel, 0));
        edit(&mut board, &[r], |sel| remove_effect(sel, 0));
        assert_eq!(held(&board, r), []);
        assert_eq!(effects(&read(&board, &[r])), Some(vec![]));
    }

    #[test]
    fn mixed_effects_are_none() {
        let mut board = design_file();
        let r1 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
        let r2 = rectangle(&mut board, "R2", 300.0);
        assert_eq!(effects(&read(&board, &[r1, r2])), None);
        // Layers that agree hold a list.
        edit(&mut board, &[r2], |sel| {
            add_effect(sel, EffectKind::DropShadow)
        });
        assert_eq!(rows_of(&board, &[r1, r2]).len(), 1);
        // And so do layers with no effect at all.
        let r3 = rectangle(&mut board, "R3", 600.0);
        let r4 = rectangle(&mut board, "R4", 900.0);
        assert_eq!(effects(&read(&board, &[r3, r4])), Some(vec![]));
        // Nothing selected has no effects.
        assert_eq!(effects(&read(&board, &[])), Some(vec![]));
    }

    #[test]
    fn replace_all_makes_one_new_effect_the_only_effect_of_every_layer() {
        let mut board = design_file();
        let r1 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
        let r2 = with_effects(&mut board, json!([{"type": "glass"}, {"type": "noise"}]));
        let sel = read(&board, &[r1, r2]);
        assert_eq!(effects(&sel), None);
        let steps = undo_steps(&board);

        let edit = add_effect(&sel, EffectKind::LayerBlur);
        assert_eq!(edit.params["ids"], json!([r1, r2]));
        apply(&mut board, &edit);
        assert_eq!(undo_steps(&board), steps + 1);
        for id in [r1, r2] {
            assert_eq!(held(&board, id), [Effect::layer_blur()]);
        }
        board.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(held(&board, r1), [Effect::drop_shadow()]);
        assert_eq!(held(&board, r2), [Effect::glass(), Effect::noise()]);
    }

    #[test]
    fn effect_edits_apply_to_every_selected_layer_in_one_undo_step() {
        let mut board = design_file();
        let r1 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
        let r2 = with_effects(&mut board, json!([{"type": "dropShadow"}]));
        let sel = read(&board, &[r1, r2]);
        let steps = undo_steps(&board);

        let mut shadow = effects(&sel).unwrap().remove(0);
        shadow.blur = 16.0;
        apply(&mut board, &set_effect(&sel, 0, &shadow));
        assert_eq!(undo_steps(&board), steps + 1);
        for id in [r1, r2] {
            assert_eq!(rows_of(&board, &[id])[0].blur, 16.0);
        }
        board.execute("edit.undo", &json!({})).unwrap();
        for id in [r1, r2] {
            assert_eq!(held(&board, id), [Effect::drop_shadow()]);
        }
    }

    /// With no list to change a row of, no such row, or a row the engine could not take, an edit
    /// says so by being refused, and the file stays as it was.
    #[test]
    fn edits_that_cannot_apply_are_refused_and_change_nothing() {
        let mut board = design_file();
        let r1 = with_effects(
            &mut board,
            json!([{"type": "dropShadow"}, {"type": "layerBlur"}]),
        );
        let r2 = rectangle(&mut board, "R2", 300.0);
        let one = read(&board, &[r1]);
        let differing = read(&board, &[r1, r2]);
        let shadow = effects(&one).unwrap().remove(0);
        let blur = effects(&one).unwrap().remove(1);

        let edits = [
            remove_effect(&one, 2),
            remove_effect(&one, usize::MAX),
            remove_effect(&differing, 0),
            set_effect(&one, 2, &shadow),
            set_effect(&differing, 0, &shadow),
            // A shadow needs a colour and numbers.
            set_effect(
                &one,
                0,
                &EffectRow {
                    hex: "#12".into(),
                    ..shadow.clone()
                },
            ),
            set_effect(
                &one,
                0,
                &EffectRow {
                    hex: "#FF000080".into(),
                    ..shadow.clone()
                },
            ),
            set_effect(
                &one,
                0,
                &EffectRow {
                    x: f64::NAN,
                    ..shadow.clone()
                },
            ),
            set_effect(
                &one,
                0,
                &EffectRow {
                    opacity: f64::INFINITY,
                    ..shadow.clone()
                },
            ),
            // So does a blur.
            set_effect(
                &one,
                1,
                &EffectRow {
                    blur: f64::NEG_INFINITY,
                    ..blur
                },
            ),
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

    /// A hex that isn't a colour, and numbers that aren't finite, don't matter for a row whose
    /// kind doesn't use them.
    #[test]
    fn a_blur_or_other_row_ignores_the_fields_it_does_not_use() {
        let mut board = design_file();
        let r = with_effects(
            &mut board,
            json!([{"type": "layerBlur", "radius": 4}, {"type": "glass"}]),
        );
        let sel = read(&board, &[r]);
        let rows = effects(&sel).unwrap();
        let junk = |row: &EffectRow| EffectRow {
            hex: "not a colour".into(),
            x: f64::NAN,
            y: f64::NAN,
            spread: f64::NAN,
            opacity: f64::NAN,
            ..row.clone()
        };
        let mut blur = junk(&rows[0]);
        blur.blur = 6.0;
        apply(&mut board, &set_effect(&sel, 0, &blur));
        assert_eq!(rows_of(&board, &[r])[0].blur, 6.0);

        // Each edit sends the whole list it was made from, so the next one reads the file again.
        let sel = read(&board, &[r]);
        apply(&mut board, &set_effect(&sel, 1, &junk(&rows[1])));
        assert_eq!(rows_of(&board, &[r])[0].blur, 6.0);
        assert_eq!(held(&board, r)[1], Effect::glass());
    }
}
