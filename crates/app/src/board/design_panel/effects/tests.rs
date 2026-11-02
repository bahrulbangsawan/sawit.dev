//! The Effects section: its rows and editors, "Mixed", and the values typed in its fields.

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
    let path = tmp.0.join("design.sawit");
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
    let path = tmp.0.join("design.sawit");
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
    let path = tmp.0.join("design.sawit");
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
