//! The rows of the Fill and Stroke sections: a paint's hex and opacity typed, a gradient replaced,
//! "Mixed" replaced all at once, and the stroke's weight, position and dashes.

use figcraft_doc::NodeId;
use gpui_kit::{TestAppContext, VisualTestContext};
use serde_json::{Value, json};

use super::super::{gradient_name, hex_text};
use super::{tool, type_in, with_fills};
use crate::board::design_panel::tests::{
    Scratch, click, drawn, field_text, focus_canvas, open_in_window, prop, undo_steps,
};

/// Review Focus 1 and 2 for a paint: a typed hex or opacity goes to the paint of every
/// selected layer, as one undo step, and what isn't one is refused, the field showing the
/// paint's again.
#[gpui_kit::test]
fn a_paints_hex_and_opacity_are_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("paint-fields");
    let path = tmp.0.join("design.sawit");
    let fills = json!(["#D9D9D9"]);
    let ids = with_fills(&path, &[0.0, 150.0], &[fills.clone(), fills]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    assert_eq!(field_text(&view, "fills.0.hex", &cx), "D9D9D9");
    assert_eq!(field_text(&view, "fills.0.opacity", &cx), "100%");
    let steps = undo_steps(&view, &cx);
    type_in(&mut cx, "design-field-fills.0.hex", "ff8800");
    for id in &ids {
        assert_eq!(prop(&view, *id, "fills", &cx)[0]["color"], json!("#FF8800"));
    }
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(field_text(&view, "fills.0.hex", &cx), "FF8800");
    // No colour: nothing changes, and the field shows the paint's again.
    type_in(&mut cx, "design-field-fills.0.hex", "zz");
    assert_eq!(field_text(&view, "fills.0.hex", &cx), "FF8800");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    // 50 in the opacity field is half (ruling P5-C7), and keeps the colour.
    type_in(&mut cx, "design-field-fills.0.opacity", "50");
    for id in &ids {
        let fill = &prop(&view, *id, "fills", &cx)[0];
        assert_eq!(
            (&fill["color"], &fill["opacity"]),
            (&json!("#FF8800"), &json!(0.5))
        );
    }
    assert_eq!(field_text(&view, "fills.0.opacity", &cx), "50%");
}

/// Review Focus 4 through the panel: a gradient beside an edited solid stays as it is, and
/// becomes a solid only by its own button.
#[gpui_kit::test]
fn a_gradient_becomes_a_solid_only_when_asked(cx: &mut TestAppContext) {
    let tmp = Scratch::new("paint-gradient");
    let path = tmp.0.join("design.sawit");
    let gradient = json!({"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]});
    let ids = with_fills(&path, &[0.0], &[json!([gradient, "#112233"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let fills = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx);
    let before = fills(&cx)[0].clone();
    assert!(!drawn(&mut cx, "design-fills-0-swatch"));
    assert!(drawn(&mut cx, "design-fills-1-swatch"));
    type_in(&mut cx, "design-field-fills.1.hex", "ff8800");
    assert_eq!(fills(&cx)[0], before);
    assert_eq!(fills(&cx)[1]["color"], json!("#FF8800"));
    click(&mut cx, "design-fills-0-replace");
    assert_eq!(fills(&cx)[0]["type"], json!("solid"));
    assert_eq!(fills(&cx)[0]["color"], json!("#FF0000"));
    assert!(drawn(&mut cx, "design-fills-0-swatch"));
}

/// Paints that differ show "Mixed"; Replace all gives every layer one paint, as one undo step;
/// then the eye hides it and the remove button takes it off.
#[gpui_kit::test]
fn mixed_fills_are_replaced_all_at_once(cx: &mut TestAppContext) {
    let tmp = Scratch::new("paint-mixed");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(
        &path,
        &[0.0, 150.0],
        &[json!(["#112233"]), json!(["#445566"])],
    );
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let fills = |id: NodeId, cx: &VisualTestContext| prop(&view, id, "fills", cx);
    assert!(drawn(&mut cx, "design-fills-replace-all"));
    assert!(!drawn(&mut cx, "design-fills-0-swatch"));
    let steps = undo_steps(&view, &cx);
    click(&mut cx, "design-fills-replace-all");
    for id in &ids {
        let fills = fills(*id, &cx);
        assert_eq!(fills.as_array().map(Vec::len), Some(1), "{fills}");
        assert_eq!(fills[0]["color"], json!("#D9D9D9"));
    }
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    click(&mut cx, "design-fills-0-visible");
    for id in &ids {
        assert_eq!(fills(*id, &cx)[0]["visible"], json!(false));
    }
    click(&mut cx, "design-fills-0-remove");
    for id in &ids {
        assert_eq!(fills(*id, &cx), Value::Null, "no fill left");
    }
}

/// The Stroke section: a stroke added shows its weight, position and dashes. Neither a pick
/// from its menu nor a click on its switch keeps the keyboard from the canvas (delegate
/// ruling on P5.T4): the canvas's keys work after them with no click on it.
#[gpui_kit::test]
fn the_stroke_section_edits_the_strokes(cx: &mut TestAppContext) {
    let tmp = Scratch::new("strokes");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let value = |key: &str, cx: &VisualTestContext| prop(&view, ids[0], key, cx);
    assert!(!drawn(&mut cx, "design-field-strokeWeight"));
    click(&mut cx, "design-strokes-add");
    assert_eq!(value("strokes", &cx)[0]["color"], json!("#000000"));
    type_in(&mut cx, "design-field-strokeWeight", "4");
    assert_eq!(value("strokeWeight", &cx), json!(4.0));

    focus_canvas(&view, &mut cx);
    click(&mut cx, "design-strokeAlign");
    click(&mut cx, "design-strokeAlign-Outside");
    assert_eq!(value("strokeAlign", &cx), json!("outside"));
    cx.simulate_keystrokes("r");
    assert_eq!(tool(&view, &cx), "rectangle");

    click(&mut cx, "design-dashes");
    assert_eq!(value("dashes", &cx), json!([4.0, 4.0]));
    cx.simulate_keystrokes("o");
    assert_eq!(tool(&view, &cx), "ellipse");
    click(&mut cx, "design-dashes");
    assert_eq!(value("dashes", &cx), Value::Null, "solid again");
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

#[test]
fn a_hex_shows_without_its_hash() {
    assert_eq!(hex_text("#D9D9D9").as_ref(), "D9D9D9");
}

#[test]
fn a_gradient_is_named_by_its_kind() {
    assert_eq!(gradient_name("Linear"), "Linear gradient");
    assert_eq!(gradient_name("Gradient"), "Gradient");
}
