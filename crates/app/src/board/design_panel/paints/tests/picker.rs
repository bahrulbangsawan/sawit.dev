//! A paint's or a shadow's colour picker: the colours it goes through are one undo step, Escape
//! puts them back, and the canvas's presses, typing, gestures and agents' ops are kept apart from
//! them.

use figcraft_doc::NodeId;
use gpui_kit::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use serde_json::{Value, json};

use super::super::picker::{colour_of, hex_and_opacity};
use super::{is_open, pick, picker, tool, with_fills};
use crate::board::design_panel::tests::{
    Scratch, click, design_file, draw_pen_path, drawn, edit_text, editing, field_text,
    focus_canvas, gesture, on_canvas, open_in_window, prop, select, text_layer, undo, undo_steps,
    zoom,
};

/// Ruling P5-C15: the colours a picker goes through while it is open are one undo step.
#[gpui_kit::test]
fn color_pick_is_one_undo_step(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-pick");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
    assert_eq!(colour(&cx), json!("#D9D9D9"));
    let steps = undo_steps(&view, &cx);
    // The swatch opens the picker, and with it the scrub.
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    assert!(is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_some()));
    for hex in ["#FF0000", "#00FF00", "#0000FF"] {
        pick(&picker, hex, &mut cx);
        // The canvas shows each colour as it comes, with no undo step of its own.
        assert_eq!(colour(&cx), json!(hex));
        assert_eq!(undo_steps(&view, &cx), steps);
    }
    // Closing the picker ends the scrub, as one undo step.
    click(&mut cx, "design-fills-0-swatch");
    assert!(!is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(colour(&cx), json!("#0000FF"));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    assert_eq!(colour(&cx), json!("#D9D9D9"));
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

/// Ruling P5-C15: Escape closes the picker and puts the colour back, with no undo step.
#[gpui_kit::test]
fn escape_puts_a_colour_pick_back(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-escape");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
    let steps = undo_steps(&view, &cx);
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    pick(&picker, "#FF0000", &mut cx);
    assert_eq!(colour(&cx), json!("#FF0000"));
    cx.simulate_keystrokes("escape");
    assert!(!is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(colour(&cx), json!("#D9D9D9"));
    assert_eq!(undo_steps(&view, &cx), steps);
    // The swatch shows the colour that stayed.
    let shown = picker.read_with(&cx, |picker, _| picker.value());
    assert_eq!(shown, colour_of("#D9D9D9", 1.0));
}

/// A press on the canvas while a picker is open ends the colour's scrub first, kept, so that
/// the drag the press starts is a gesture of its own: the selected rectangle moves, no toast
/// shows, and the colour and the move are two undo steps.
#[gpui_kit::test]
fn a_drag_on_the_canvas_after_a_colour_pick_moves_the_layer(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-then-drag");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
    let steps = undo_steps(&view, &cx);
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    pick(&picker, "#FF0000", &mut cx);
    assert_eq!(tool(&view, &cx), "move");
    // The press on the rectangle's middle closes the picker, and drags the rectangle 40 px.
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = on_canvas(&view, (50.0, 50.0), &cx);
    let end = start + point(px(40.), px(0.));
    cx.simulate_mouse_down(start, left, none);
    cx.simulate_mouse_move(end, left, none);
    cx.simulate_mouse_up(end, left, none);
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
    assert!(!is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(
        prop(&view, ids[0], "x", &cx),
        json!(40.0 / zoom(&view, &cx))
    );
    assert_eq!(colour(&cx), json!("#FF0000"));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    // The move is undone first, then the colour.
    view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
    assert_eq!(colour(&cx), json!("#FF0000"));
    view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    assert_eq!(colour(&cx), json!("#D9D9D9"));
}

/// An agent's op while a picker is open. A read leaves the picker and its scrub as they are.
/// An op that changes the canvas ends the colour's scrub first, kept, and closes the picker:
/// the op is an undo step of its own, which an Escape after it doesn't take back.
#[gpui_kit::test]
fn an_op_while_a_colour_is_picked_is_a_step_of_its_own(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-then-op");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
    let run_op = |op: Value, cx: &mut VisualTestContext| {
        let reply = view.update(cx, |view, cx| view.run_op(&op, cx));
        cx.run_until_parked();
        assert_eq!(reply["ok"], json!(true), "{reply}");
    };
    let steps = undo_steps(&view, &cx);
    // As after a click on the layer: the picker gives the keyboard back to the canvas.
    focus_canvas(&view, &mut cx);
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    pick(&picker, "#FF0000", &mut cx);
    run_op(json!({"op": "get"}), &mut cx);
    assert!(is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_some()));
    let x = json!({"ids": ids, "props": {"x": 40}});
    run_op(
        json!({"op": "command", "command": "node.set", "params": x}),
        &mut cx,
    );
    assert!(!is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    cx.simulate_keystrokes("escape");
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(40.0));
    assert_eq!(colour(&cx), json!("#FF0000"));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

/// A value typed in a field and left there, when a swatch is clicked, is applied before the
/// picker's scrub begins: it is an undo step of its own, which an Escape in the picker doesn't
/// take back.
#[gpui_kit::test]
fn a_value_typed_before_a_colour_pick_stays(cx: &mut TestAppContext) {
    let tmp = Scratch::new("typed-then-colour");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
    let steps = undo_steps(&view, &cx);
    // 40 typed in X, and no Enter: the click on the swatch takes the keyboard from it.
    click(&mut cx, "design-field-x");
    cx.simulate_input("40");
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    assert!(is_open(&picker, &cx));
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(40.0));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    pick(&picker, "#FF0000", &mut cx);
    cx.simulate_keystrokes("escape");
    assert!(!is_open(&picker, &cx));
    assert_eq!(colour(&cx), json!("#D9D9D9"));
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(40.0));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(field_text(&view, "x", &cx), "40");
}

/// Review C1: a colour picked while a text is edited on the canvas. The edit ends when the
/// picker opens, so what was typed is an undo step of its own: Escape in the picker puts back
/// the colour, and keeps the typing.
#[gpui_kit::test]
fn escape_in_a_colour_pick_keeps_the_text_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-escape-text");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[text_layer("Hello")]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let characters = |cx: &VisualTestContext| prop(&view, ids[0], "characters", cx);
    let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
    let before = colour(&cx);
    let steps = undo_steps(&view, &cx);
    edit_text(&view, ids[0], &mut cx);
    cx.simulate_input(" world");
    assert_eq!(characters(&cx), json!("Hello world"));
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    assert!(is_open(&picker, &cx));
    assert!(!editing(&view, &cx));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(gesture(&view, &cx).as_deref(), Some("fills"));
    pick(&picker, "#FF0000", &mut cx);
    assert_eq!(colour(&cx), json!("#FF0000"));
    cx.simulate_keystrokes("escape");
    assert!(!is_open(&picker, &cx));
    assert_eq!(colour(&cx), before);
    assert_eq!(characters(&cx), json!("Hello world"));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(gesture(&view, &cx), None);
    undo(&view, &mut cx);
    assert_eq!(characters(&cx), json!("Hello"));
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

/// Review C1, the commit: the typing before a colour pick, the pick, and typing in the text
/// again after it are three undo steps, undone one at a time.
#[gpui_kit::test]
fn a_colour_pick_while_text_is_edited_is_a_step_of_its_own(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-commit-text");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[text_layer("Hello")]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let characters = |cx: &VisualTestContext| prop(&view, ids[0], "characters", cx);
    let colour = |cx: &VisualTestContext| prop(&view, ids[0], "fills", cx)[0]["color"].clone();
    let before = colour(&cx);
    let steps = undo_steps(&view, &cx);
    edit_text(&view, ids[0], &mut cx);
    cx.simulate_input(" world");
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    pick(&picker, "#FF0000", &mut cx);
    click(&mut cx, "design-fills-0-swatch");
    assert!(!is_open(&picker, &cx));
    assert_eq!(colour(&cx), json!("#FF0000"));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    // Typing again, after a double-click into the text.
    edit_text(&view, ids[0], &mut cx);
    cx.simulate_input("!");
    cx.simulate_keystrokes("escape");
    assert!(!editing(&view, &cx));
    assert_eq!(characters(&cx), json!("Hello world!"));
    assert_eq!(undo_steps(&view, &cx), steps + 3);
    undo(&view, &mut cx);
    assert_eq!(characters(&cx), json!("Hello world"));
    assert_eq!(colour(&cx), json!("#FF0000"));
    undo(&view, &mut cx);
    assert_eq!(characters(&cx), json!("Hello world"));
    assert_eq!(colour(&cx), before);
    undo(&view, &mut cx);
    assert_eq!(characters(&cx), json!("Hello"));
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

/// Review C1: a text emptied while it is edited goes when the edit ends, as the picker opens.
/// With nothing left to colour, the picker closes, and nothing else happens: no scrub, no
/// toast.
#[gpui_kit::test]
fn a_colour_pick_on_a_text_emptied_while_edited_closes(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-emptied-text");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[text_layer("Hi")]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    edit_text(&view, ids[0], &mut cx);
    cx.simulate_keystrokes("backspace backspace");
    assert_eq!(prop(&view, ids[0], "characters", &cx), json!(""));
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    assert!(!is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(gesture(&view, &cx), None);
    assert!(drawn(&mut cx, "design-nothing-selected"));
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

/// Review C1: a pen path being drawn is a gesture the engine holds open, which no scrub may
/// take over. A colour picked for the path then joins the path's own undo step, and Escape in
/// the picker takes back neither the colour nor the path.
#[gpui_kit::test]
fn a_colour_pick_leaves_an_open_pen_path_alone(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-pen");
    let path = tmp.0.join("design.sawit");
    let ids = with_fills(&path, &[0.0], &[json!(["#D9D9D9"])]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    let path_id = draw_pen_path(&view, &mut cx);
    let stroke = |cx: &VisualTestContext| prop(&view, path_id, "strokes", cx)[0]["color"].clone();
    let width = prop(&view, path_id, "width", &cx);
    click(&mut cx, "design-strokes-0-swatch");
    let picker = picker(&view, "strokes-0", &cx);
    assert!(is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    pick(&picker, "#FF0000", &mut cx);
    assert_eq!(stroke(&cx), json!("#FF0000"));
    cx.simulate_keystrokes("escape");
    assert!(!is_open(&picker, &cx));
    assert_eq!(stroke(&cx), json!("#FF0000"));
    assert_eq!(prop(&view, path_id, "width", &cx), width);
    assert_eq!(gesture(&view, &cx).as_deref(), Some("Pen"));
    assert_eq!(undo_steps(&view, &cx), steps);
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

/// A picker's scrub lasts while its swatch is drawn for the layers the scrub began on. When
/// other layers are selected, by a path that doesn't end it itself, or the panel is hidden,
/// it ends, kept, and the picker closes: else no later edit could leave its undo step.
#[gpui_kit::test]
fn a_colour_pick_ends_once_its_swatch_is_gone(cx: &mut TestAppContext) {
    let tmp = Scratch::new("colour-out-of-sight");
    let path = tmp.0.join("design.sawit");
    let fills = json!(["#D9D9D9"]);
    let ids = with_fills(&path, &[0.0, 150.0], &[fills.clone(), fills]);
    let (view, mut cx) = open_in_window(cx, &path, &ids[..1]);
    let colour =
        |id: NodeId, cx: &VisualTestContext| prop(&view, id, "fills", cx)[0]["color"].clone();
    let show_design = |show: bool, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            view.show_design = show;
            cx.notify();
        });
        cx.run_until_parked();
    };
    let steps = undo_steps(&view, &cx);
    click(&mut cx, "design-fills-0-swatch");
    let picker = picker(&view, "fills-0", &cx);
    pick(&picker, "#FF0000", &mut cx);
    // The other rectangle: its fill's swatch has the same name, but its layer isn't the one
    // the pick began on.
    select(&view, &ids[1..], &mut cx);
    assert!(!is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(colour(ids[0], &cx), json!("#FF0000"));
    assert_eq!(colour(ids[1], &cx), json!("#D9D9D9"));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    // The panel hidden while the other rectangle's colour is picked.
    click(&mut cx, "design-fills-0-swatch");
    pick(&picker, "#00FF00", &mut cx);
    show_design(false, &mut cx);
    assert!(!is_open(&picker, &cx));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(colour(ids[1], &cx), json!("#00FF00"));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    // Shown again, the swatch is there and its picker stays closed.
    show_design(true, &mut cx);
    assert!(drawn(&mut cx, "design-fills-0-swatch"));
    assert!(!is_open(&picker, &cx));
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

#[test]
fn a_colour_goes_through_the_picker_and_back() {
    for (hex, opacity) in [
        ("#D9D9D9", 1.0),
        ("#000000", 0.25),
        ("#FF8800", 0.5),
        ("#0D99FF", 0.0),
    ] {
        let colour = colour_of(hex, opacity).unwrap();
        assert_eq!(hex_and_opacity(colour), (hex.to_string(), opacity), "{hex}");
    }
    // Any form of hex the field takes; anything else is no colour.
    assert!(colour_of("d9d9d9", 1.0).is_some());
    assert!(colour_of("#12", 1.0).is_none());
}
