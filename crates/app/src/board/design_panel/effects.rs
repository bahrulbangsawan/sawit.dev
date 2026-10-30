//! The Effects section (spec "Design panel" 7): the layers' effects, the top one first as in
//! FigCraft's own panel. Each has its kind, its eye and a button that removes it, and an editor
//! that opens below it: a drop or inner shadow's x, y, blur, spread and colour, a layer or
//! background blur's radius. Noise, texture and glass, which the panel doesn't edit, show their
//! name, eye and remove button only (ruling P5-C4). Layers whose effects differ show "Mixed", and
//! its "Replace all" gives them all one drop shadow.
//!
//! The effects change only through `sawit_board::design::effects`. A typed value is checked before
//! it goes into a row, and the field shows the effect's value again when it is refused (delegate
//! ruling on P5.T2): a hex must be a colour, x, y and spread finite numbers, a blur a finite
//! number not below 0, and an opacity a percentage (ruling P5-C7). The shadow's colour picker is
//! the paints' ([`paints::colour_swatch`]).

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::effects::{
    EffectKind, EffectRow, add_effect, effects, remove_effect, set_effect,
};
use sawit_board::design::paints::parse_hex;
use sawit_board::design::{DesignSelection, parse_number};

use super::number_field::{self, Rule, TextField};
use super::paints::{self, ADD_ICON, ColourOf, OPACITY_WIDTH, REMOVE_ICON};
use super::{
    Choices, choice_menu, eye_button, guarded, icon_button, mixed_row, on_click_edit, row,
    row_button, section,
};
use crate::board::BoardView;

/// The picture of the button that opens and closes an effect's editor.
pub(super) const EDIT_ICON: Lucide = Lucide::Settings2;

/// The kinds an effect row can be switched to, and what the menu calls them.
const KINDS: [(&str, &str); 4] = [
    ("dropShadow", "Drop shadow"),
    ("innerShadow", "Inner shadow"),
    ("layerBlur", "Layer blur"),
    ("backgroundBlur", "Background blur"),
];

/// A part of an effect that a field of its editor shows and edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    X,
    Y,
    Blur,
    Spread,
    Hex,
    Opacity,
}

/// The Effects section for the layers of `sel`.
pub(super) fn render(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    if view.design.effects_of != sel.ids {
        view.design.effects_of.clone_from(&sel.ids);
        view.design.effects_open.clear();
    }
    let add_shadow = |sel: &DesignSelection| Some(add_effect(sel, EffectKind::DropShadow));
    let add = icon_button("design-effects-add", ADD_ICON, "Add effect")
        .debug_selector(|| "design-effects-add".into())
        .on_click(on_click_edit(&this, add_shadow))
        .into_any_element();
    let rows = match guarded("ui.effects", Some(Vec::new()), || effects(sel)) {
        None => vec![mixed_row("effects", &this, add_shadow, cx)],
        Some(rows) => {
            let mut drawn = Vec::new();
            for (index, effect) in rows.into_iter().enumerate().rev() {
                drawn.extend(effect_rows(view, sel, index, effect, window, cx));
            }
            drawn
        }
    };
    section("effects", "Effects", vec![add], rows, cx)
}

/// The rows of effect `index`: its kind, eye and remove button, and its editor while it is open.
/// An effect the panel doesn't edit shows its name instead of its kind, and has no editor.
fn effect_rows(
    view: &mut BoardView,
    sel: &DesignSelection,
    index: usize,
    effect: EffectRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    let this = cx.entity().downgrade();
    let id = format!("effects-{index}");
    let visible = effect.visible;
    let eye = eye_button(&id, visible, &this, move |sel| {
        let mut effect = effect_at(sel, index)?;
        effect.visible = !visible;
        Some(set_effect(sel, index, &effect))
    });
    let remove = row_button(
        &id,
        "remove",
        REMOVE_ICON,
        "Remove effect",
        &this,
        move |sel| Some(remove_effect(sel, index)),
    );
    if matches!(effect.kind, EffectKind::Other(_)) {
        let selector = format!("design-{id}-name");
        let name = div()
            .debug_selector(move || selector)
            .flex_1()
            .min_w_0()
            .truncate()
            .child(kind_name(&effect.kind));
        let row = row().items_center().min_h(px(28.));
        return vec![row.child(name).child(eye).child(remove).into_any_element()];
    }
    let open = view.design.effects_open.contains(&index);
    let toggle = {
        let view = this.clone();
        let selector = format!("design-{id}-edit");
        icon_button(
            SharedString::from(selector.clone()),
            EDIT_ICON,
            "Effect settings",
        )
        .selected(open)
        .debug_selector(move || selector)
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |view, cx| {
                if !view.design.effects_open.remove(&index) {
                    view.design.effects_open.insert(index);
                }
                cx.notify();
            });
        })
    };
    let kind = kind_menu(&id, index, &effect.kind, &this);
    let heading = row()
        .items_center()
        .child(toggle)
        .child(kind)
        .child(eye)
        .child(remove);
    let mut rows = vec![heading.into_any_element()];
    if open {
        rows.extend(editor(view, sel, index, &effect, window, cx));
    }
    rows
}

/// The menu of effect `index`'s kind. A shadow switched to the other shadow, or a blur to the
/// other blur, keeps what it holds; any other switch starts the new kind from the engine's
/// defaults (`sawit_board::design::effects::set_effect`).
fn kind_menu(
    id: &str,
    index: usize,
    kind: &EffectKind,
    view: &WeakEntity<BoardView>,
) -> AnyElement {
    let choices = || {
        let named = KINDS.iter();
        named
            .map(|(tag, name)| (kind_of(tag), (*name).into()))
            .collect()
    };
    let view = view.clone();
    let pick = move |kind: &EffectKind, cx: &mut App| {
        let kind = kind.clone();
        let _ = view.update(cx, |view, cx| {
            view.edit_layers(
                |sel| {
                    let mut effect = effect_at(sel, index)?;
                    effect.kind = kind;
                    Some(set_effect(sel, index, &effect))
                },
                cx,
            );
        });
    };
    let menu_id = format!("{id}-kind");
    let choices = Choices::new(choices, Some(kind.clone()), pick);
    choice_menu(&menu_id, kind_name(kind), "Effect", choices, false).into_any_element()
}

/// The editor of effect `index`: a shadow's x and y, blur and spread, and colour; a blur's radius.
fn editor(
    view: &mut BoardView,
    sel: &DesignSelection,
    index: usize,
    effect: &EffectRow,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Vec<AnyElement> {
    match effect.kind {
        EffectKind::DropShadow | EffectKind::InnerShadow => {
            let x = field(view, sel, index, Part::X, window, cx);
            let y = field(view, sel, index, Part::Y, window, cx);
            let blur = field(view, sel, index, Part::Blur, window, cx);
            let spread = field(view, sel, index, Part::Spread, window, cx);
            let of = ColourOf::Shadow { index };
            let id = format!("effects-{index}");
            let swatch =
                paints::colour_swatch(view, &id, of, &effect.hex, effect.opacity, window, cx);
            let hex = field(view, sel, index, Part::Hex, window, cx);
            let opacity = field(view, sel, index, Part::Opacity, window, cx);
            vec![
                row().child(x).child(y).into_any_element(),
                row().child(blur).child(spread).into_any_element(),
                row()
                    .items_center()
                    .child(swatch)
                    .child(hex)
                    .child(opacity)
                    .into_any_element(),
            ]
        }
        EffectKind::LayerBlur | EffectKind::BackgroundBlur => {
            let blur = field(view, sel, index, Part::Blur, window, cx);
            // Half the row, as wide as a shadow's fields.
            vec![row().child(blur).child(div().flex_1()).into_any_element()]
        }
        EffectKind::Other(_) => Vec::new(),
    }
}

/// The field of `part` of effect `index`.
fn field(
    view: &mut BoardView,
    sel: &DesignSelection,
    index: usize,
    part: Part,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let field = TextField::new(
        format!("effects.{index}.{}", part.name()),
        rule(index, part),
    );
    let field = match part.label() {
        Some(label) => field.label(label),
        None => field,
    };
    let field = if part == Part::Opacity {
        field.width(OPACITY_WIDTH)
    } else {
        field
    };
    field.render(view, sel, window, cx)
}

/// The rule of the field of `part` of effect `index`: the part's value, and a typed value checked
/// before it goes into the effect ([`Part::read`]).
fn rule(index: usize, part: Part) -> Rule {
    Rule::new(
        move |sel| effect_at(sel, index).map_or_else(SharedString::default, |e| part.show(&e)),
        move |sel, typed| {
            let mut effect = effect_at(sel, index)?;
            part.read(typed, &mut effect)?;
            Some(set_effect(sel, index, &effect))
        },
    )
}

/// Effect `index` of the layers of `sel`, when they hold the same effects and it is one of them.
fn effect_at(sel: &DesignSelection, index: usize) -> Option<EffectRow> {
    effects(sel)?.into_iter().nth(index)
}

impl Part {
    /// The part's name among the effect's fields.
    fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Blur => "blur",
            Self::Spread => "spread",
            Self::Hex => "hex",
            Self::Opacity => "opacity",
        }
    }

    /// The label of the part's field; the colour's fields go beside its swatch without one.
    fn label(self) -> Option<&'static str> {
        match self {
            Self::X => Some("X"),
            Self::Y => Some("Y"),
            Self::Blur => Some("Blur"),
            Self::Spread => Some("Spread"),
            Self::Hex | Self::Opacity => None,
        }
    }

    /// The part of `effect` as its field shows it.
    fn show(self, effect: &EffectRow) -> SharedString {
        match self {
            Self::X => number_field::number_text(effect.x),
            Self::Y => number_field::number_text(effect.y),
            Self::Blur => number_field::number_text(effect.blur),
            Self::Spread => number_field::number_text(effect.spread),
            Self::Hex => paints::hex_text(&effect.hex),
            Self::Opacity => number_field::percent(effect.opacity),
        }
    }

    /// Puts `typed` into this part of `effect`, when it is a value the part takes: x, y and
    /// spread a finite number, a blur one not below 0 (the engine would store a negative radius
    /// as it is), a hex a colour ([`parse_hex`]), an opacity a percentage. `None` when it isn't,
    /// and the effect is left as it was.
    fn read(self, typed: &str, effect: &mut EffectRow) -> Option<()> {
        match self {
            Self::X => effect.x = parse_number("x", typed)?,
            Self::Y => effect.y = parse_number("y", typed)?,
            Self::Spread => effect.spread = parse_number("spread", typed)?,
            Self::Blur => effect.blur = parse_number("blur", typed).filter(|blur| *blur >= 0.0)?,
            Self::Hex => effect.hex = parse_hex(typed)?,
            Self::Opacity => effect.opacity = number_field::parse("opacity", typed)?,
        }
        Some(())
    }
}

/// The kind of effect with type tag `tag`, as [`KINDS`] names them.
fn kind_of(tag: &str) -> EffectKind {
    match tag {
        "dropShadow" => EffectKind::DropShadow,
        "innerShadow" => EffectKind::InnerShadow,
        "layerBlur" => EffectKind::LayerBlur,
        "backgroundBlur" => EffectKind::BackgroundBlur,
        other => EffectKind::Other(other.to_string()),
    }
}

/// What the panel calls an effect of `kind`: "Drop shadow"; "Glass" for one it doesn't edit.
fn kind_name(kind: &EffectKind) -> SharedString {
    let tag = match kind {
        EffectKind::DropShadow => "dropShadow",
        EffectKind::InnerShadow => "innerShadow",
        EffectKind::LayerBlur => "layerBlur",
        EffectKind::BackgroundBlur => "backgroundBlur",
        EffectKind::Other(tag) => {
            let mut letters = tag.chars();
            let name: String = match letters.next() {
                Some(first) => first.to_uppercase().chain(letters).collect(),
                None => "Effect".to_string(),
            };
            return name.into();
        }
    };
    KINDS
        .iter()
        .find(|(known, _)| *known == tag)
        .map_or_else(SharedString::default, |(_, name)| (*name).into())
}

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use sawit_board::design::DesignSelection;
    use sawit_board::design::effects::{EffectKind, EffectRow, effects};
    use serde_json::{Value, json};

    // Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
    use super::super::tests::{
        Design, Scratch, click, drawn, field_text, open_in_window, prop, rectangle, undo_steps,
    };
    use super::{KINDS, Part, kind_name, kind_of};
    use crate::board::BoardView;

    /// Writes a design file at `path` with a rectangle whose effects are `effects` (as `node.set`
    /// takes them), and returns its id.
    fn with_effects(path: &std::path::Path, effects: Value) -> NodeId {
        let mut design = Design::new();
        let id = design.make(rectangle(0.0));
        design.set(&[id], json!({ "effects": effects }));
        design.save(path);
        id
    }

    /// The effects of layer `id`, as the panel reads them.
    fn rows(view: &Entity<BoardView>, id: NodeId, cx: &VisualTestContext) -> Vec<EffectRow> {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            effects(&DesignSelection::read(&doc, &[id])).unwrap()
        })
    }

    /// Types `text` over what field `selector` shows, and presses Enter.
    fn type_in(cx: &mut VisualTestContext, selector: &'static str, text: &str) {
        click(cx, selector);
        // A field that has the keyboard already keeps its caret where the click put it.
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input(text);
        cx.simulate_keystrokes("enter");
    }

    /// The Effects section: a shadow is added, its editor takes typed values once they are
    /// checked (delegate ruling on P5.T2), and its kind changes from its menu.
    #[gpui_kit::test]
    fn the_effects_section_edits_the_effects(cx: &mut TestAppContext) {
        let tmp = Scratch::new("effects");
        let path = tmp.0.join("design.figcraft");
        let id = with_effects(&path, json!([]));
        let (view, mut cx) = open_in_window(cx, &path, &[id]);
        click(&mut cx, "design-effects-add");
        assert_eq!(rows(&view, id, &cx)[0].kind, EffectKind::DropShadow);
        assert!(!drawn(&mut cx, "design-field-effects.0.x"));
        click(&mut cx, "design-effects-0-edit");
        assert!(drawn(&mut cx, "design-field-effects.0.x"));

        type_in(&mut cx, "design-field-effects.0.x", "-3");
        type_in(&mut cx, "design-field-effects.0.spread", "2");
        type_in(&mut cx, "design-field-effects.0.blur", "12");
        type_in(&mut cx, "design-field-effects.0.hex", "ff8800");
        type_in(&mut cx, "design-field-effects.0.opacity", "50");
        let shadow = rows(&view, id, &cx).remove(0);
        assert_eq!((shadow.x, shadow.spread, shadow.blur), (-3.0, 2.0, 12.0));
        assert_eq!(shadow.hex, "#FF8800");
        assert!((shadow.opacity - 0.5).abs() < 1e-6, "{}", shadow.opacity);

        // A negative blur, and a hex that is no colour, are refused: nothing changes, nothing
        // says the edit failed, and the fields show the shadow's values again.
        let steps = undo_steps(&view, &cx);
        type_in(&mut cx, "design-field-effects.0.blur", "-1");
        type_in(&mut cx, "design-field-effects.0.hex", "zz");
        assert_eq!(rows(&view, id, &cx)[0], shadow);
        assert_eq!(undo_steps(&view, &cx), steps);
        assert_eq!(field_text(&view, "effects.0.blur", &cx), "12");
        assert_eq!(field_text(&view, "effects.0.hex", &cx), "FF8800");
        assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);

        // The other shadow keeps what the shadow holds.
        click(&mut cx, "design-effects-0-kind");
        click(&mut cx, "design-effects-0-kind-Inner shadow");
        let inner = rows(&view, id, &cx).remove(0);
        assert_eq!(inner.kind, EffectKind::InnerShadow);
        assert_eq!((inner.x, inner.blur, &inner.hex), (-3.0, 12.0, &shadow.hex));
        // A blur shows only its radius.
        click(&mut cx, "design-effects-0-kind");
        click(&mut cx, "design-effects-0-kind-Layer blur");
        assert_eq!(rows(&view, id, &cx)[0].kind, EffectKind::LayerBlur);
        assert!(drawn(&mut cx, "design-field-effects.0.blur"));
        assert!(!drawn(&mut cx, "design-field-effects.0.x"));
    }

    /// Ruling P5-C4: an effect the panel doesn't edit shows its name, its eye and its remove
    /// button only, and edits of the effects beside it leave it as it is.
    #[gpui_kit::test]
    fn a_glass_effect_shows_but_is_not_edited(cx: &mut TestAppContext) {
        let tmp = Scratch::new("effects-glass");
        let path = tmp.0.join("design.figcraft");
        let id = with_effects(&path, json!([{"type": "glass"}, {"type": "dropShadow"}]));
        let (view, mut cx) = open_in_window(cx, &path, &[id]);
        let effects = |cx: &VisualTestContext| prop(&view, id, "effects", cx);
        let glass = effects(&cx)[0].clone();
        assert!(drawn(&mut cx, "design-effects-0-name"));
        assert!(!drawn(&mut cx, "design-effects-0-kind"));
        assert!(!drawn(&mut cx, "design-effects-0-edit"));
        assert!(drawn(&mut cx, "design-effects-1-kind"));
        click(&mut cx, "design-effects-1-visible");
        assert_eq!(effects(&cx)[1]["visible"], json!(false));
        assert_eq!(effects(&cx)[0], glass);
        click(&mut cx, "design-effects-0-visible");
        assert_eq!(
            rows(&view, id, &cx)[0].kind,
            EffectKind::Other("glass".into())
        );
        assert!(!rows(&view, id, &cx)[0].visible);
        click(&mut cx, "design-effects-0-remove");
        let left = rows(&view, id, &cx);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].kind, EffectKind::DropShadow);
    }

    /// Effects that differ show "Mixed", and Replace all gives every layer one drop shadow.
    #[gpui_kit::test]
    fn mixed_effects_are_replaced_all_at_once(cx: &mut TestAppContext) {
        let tmp = Scratch::new("effects-mixed");
        let path = tmp.0.join("design.figcraft");
        let mut design = Design::new();
        let ids = [design.make(rectangle(0.0)), design.make(rectangle(150.0))];
        design.set(&ids[..1], json!({"effects": [{"type": "layerBlur"}]}));
        design.save(&path);
        let (view, mut cx) = open_in_window(cx, &path, &ids);
        click(&mut cx, "design-effects-replace-all");
        for id in ids {
            let rows = rows(&view, id, &cx);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].kind, EffectKind::DropShadow);
        }
    }

    fn shadow() -> EffectRow {
        EffectRow {
            kind: EffectKind::DropShadow,
            visible: true,
            x: 0.0,
            y: 4.0,
            blur: 8.0,
            spread: 0.0,
            hex: "#000000".into(),
            opacity: 0.25,
        }
    }

    /// Delegate ruling on P5.T2: every typed value is checked before it goes into a row.
    #[test]
    fn typed_effect_values_are_checked_before_they_go_in() {
        let mut effect = shadow();
        // Offsets and spread can be negative.
        assert_eq!(Part::X.read("-2", &mut effect), Some(()));
        assert_eq!(Part::Spread.read("-3.5", &mut effect), Some(()));
        assert_eq!((effect.x, effect.spread), (-2.0, -3.5));
        // A blur can't.
        assert_eq!(Part::Blur.read("-1", &mut effect), None);
        assert_eq!(Part::Blur.read("12", &mut effect), Some(()));
        assert_eq!(effect.blur, 12.0);
        for bad in ["abc", "", "1e400", "NaN"] {
            assert_eq!(Part::Y.read(bad, &mut effect), None, "{bad}");
        }
        assert_eq!(effect.y, 4.0);
        // A hex is a colour, and an opacity a percentage.
        assert_eq!(Part::Hex.read("#12", &mut effect), None);
        assert_eq!(Part::Hex.read("ff8800", &mut effect), Some(()));
        assert_eq!(effect.hex, "#FF8800");
        assert_eq!(Part::Opacity.read("50", &mut effect), Some(()));
        assert_eq!(effect.opacity, 0.5);
        assert_eq!(Part::Opacity.read("half", &mut effect), None);
    }

    #[test]
    fn effect_values_show_as_their_fields_read() {
        let effect = shadow();
        assert_eq!(Part::Y.show(&effect).as_ref(), "4");
        assert_eq!(Part::Hex.show(&effect).as_ref(), "000000");
        assert_eq!(Part::Opacity.show(&effect).as_ref(), "25%");
    }

    #[test]
    fn effect_kinds_have_names() {
        for (tag, name) in KINDS {
            assert_eq!(kind_name(&kind_of(tag)).as_ref(), name);
        }
        assert_eq!(
            kind_name(&EffectKind::Other("glass".into())).as_ref(),
            "Glass"
        );
    }
}
