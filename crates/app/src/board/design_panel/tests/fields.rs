//! What is typed in a field: it goes to the layers the field showed, over "Mixed" too, before a
//! press or a drag on the canvas, and never to the canvas's keys; and an edit the engine refuses.

use gpui_kit::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use sawit_board::design::{Edit, Mixed};
use serde_json::json;

use super::{
    Scratch, click, design_file, field_text, focus_canvas, on_canvas, open, open_in_window,
    panel_value, prop, rectangle, select, undo, undo_steps, zoom,
};

#[gpui_kit::test]
fn apply_edit_error_sets_toast(cx: &mut TestAppContext) {
    let tmp = Scratch::new("apply-edit-error");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let view = open(cx, &path, &ids);
    let edit = Edit {
        command: "no.such.command",
        params: json!({}),
    };
    view.update(cx, |view, cx| view.apply_edit(edit, cx));
    assert!(view.read_with(cx, |view, _| view.toast.is_some()));
}

/// Ruling P5-C3: a key typed in a field goes to the field, and the canvas's keys come back
/// with the keyboard.
#[gpui_kit::test]
fn typing_in_a_field_leaves_the_canvas_keys_alone(cx: &mut TestAppContext) {
    let tmp = Scratch::new("type-in-field");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let tool = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
    };
    let before = tool(&cx);
    assert_eq!(field_text(&view, "x", &cx), "0");
    // A click in X gives the field the keyboard, its text selected: what is typed replaces it.
    click(&mut cx, "design-field-x");
    cx.simulate_keystrokes("r");
    assert_eq!(tool(&cx), before);
    assert_eq!(field_text(&view, "x", &cx), "r");
    // "r" is no number: Enter changes nothing, and the field shows the value again.
    let steps = undo_steps(&view, &cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(field_text(&view, "x", &cx), "0");
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
    assert_eq!(undo_steps(&view, &cx), steps);
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    // A number is applied, as one undo step.
    cx.simulate_keystrokes("cmd-a 4 0 enter");
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(40.0));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(field_text(&view, "x", &cx), "40");
    // Back on the canvas, its keys are its own again.
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("r");
    assert_eq!(tool(&cx), "rectangle");
}

/// Review Focus 1 through the panel: a value typed over "Mixed" goes to every layer, as one
/// undo step. Ruling P5-C7: an opacity typed without % is a percentage.
#[gpui_kit::test]
fn a_value_typed_over_mixed_goes_to_every_layer(cx: &mut TestAppContext) {
    let tmp = Scratch::new("type-mixed");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0), rectangle(150.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    assert_eq!(field_text(&view, "x", &cx), "Mixed");
    assert_eq!(field_text(&view, "width", &cx), "100");
    let steps = undo_steps(&view, &cx);
    click(&mut cx, "design-field-x");
    cx.simulate_keystrokes("1 0 enter");
    for id in &ids {
        assert_eq!(prop(&view, *id, "x", &cx), json!(10.0));
    }
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(field_text(&view, "x", &cx), "10");
    // 50 in the opacity field is half.
    assert_eq!(field_text(&view, "opacity", &cx), "100%");
    click(&mut cx, "design-field-opacity");
    cx.simulate_keystrokes("5 0 enter");
    assert_eq!(
        panel_value(&view, &ids, "opacity", &cx),
        Mixed::Same(json!(0.5))
    );
    assert_eq!(field_text(&view, "opacity", &cx), "50%");
}

/// A press on the canvas after typing in a field: the typed value goes to the layers the
/// field was showing, not to the ones the press selects.
#[gpui_kit::test]
fn a_field_applies_to_the_layers_it_showed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("field-target");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0), rectangle(150.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids[..1]);
    click(&mut cx, "design-field-width");
    cx.simulate_keystrokes("6 0");
    // Another layer is selected, and the canvas takes the keyboard back: the field loses it.
    select(&view, &ids[1..], &mut cx);
    focus_canvas(&view, &mut cx);
    assert_eq!(prop(&view, ids[0], "width", &cx), json!(60.0));
    assert_eq!(prop(&view, ids[1], "width", &cx), json!(100.0));
    // The field shows the layer on show now.
    assert_eq!(field_text(&view, "width", &cx), "100");
}

/// Review I1: a value typed in a field, then a press on a layer on the canvas, with no Enter.
/// The value is applied before the press reaches the tools, as an undo step of its own: the
/// gesture the press opens (a move, on the Move tool) neither takes it in nor puts it back,
/// though the frame drawn before the release is where the field hears it lost the keyboard.
#[gpui_kit::test]
fn a_value_typed_then_a_press_on_the_canvas_stays(cx: &mut TestAppContext) {
    let tmp = Scratch::new("typed-then-press");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    click(&mut cx, "design-field-width");
    cx.simulate_input("60");
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let on_layer = on_canvas(&view, (20.0, 50.0), &cx);
    cx.simulate_mouse_down(on_layer, left, none);
    cx.run_until_parked();
    assert_eq!(prop(&view, ids[0], "width", &cx), json!(60.0));
    cx.simulate_mouse_up(on_layer, left, none);
    assert_eq!(prop(&view, ids[0], "width", &cx), json!(60.0));
    assert_eq!(field_text(&view, "width", &cx), "60");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    // A drag that follows moves the layer as a step of its own, keeping the width.
    let moved = on_layer + point(px(40.), px(0.));
    cx.simulate_mouse_down(on_layer, left, none);
    cx.simulate_mouse_move(moved, left, none);
    cx.simulate_mouse_up(moved, left, none);
    assert_eq!(
        prop(&view, ids[0], "x", &cx),
        json!(40.0 / zoom(&view, &cx))
    );
    assert_eq!(prop(&view, ids[0], "width", &cx), json!(60.0));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    undo(&view, &mut cx);
    assert_eq!(prop(&view, ids[0], "width", &cx), json!(60.0));
    undo(&view, &mut cx);
    assert_eq!(prop(&view, ids[0], "width", &cx), json!(100.0));
}

/// Review I1, the same with a drag: the press that starts it applies the typed value first,
/// and the drag's preview, rebuilt from the document as the press found it, keeps it.
#[gpui_kit::test]
fn a_value_typed_then_a_drag_on_the_canvas_stays(cx: &mut TestAppContext) {
    let tmp = Scratch::new("typed-then-drag");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    click(&mut cx, "design-field-height");
    cx.simulate_input("60");
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = on_canvas(&view, (20.0, 20.0), &cx);
    let end = start + point(px(40.), px(0.));
    cx.simulate_mouse_down(start, left, none);
    cx.simulate_mouse_move(end, left, none);
    cx.simulate_mouse_up(end, left, none);
    assert_eq!(prop(&view, ids[0], "height", &cx), json!(60.0));
    assert_eq!(
        prop(&view, ids[0], "x", &cx),
        json!(40.0 / zoom(&view, &cx))
    );
    assert_eq!(undo_steps(&view, &cx), steps + 2);
}
