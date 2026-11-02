//! A scrub of a field: one undo step however many values it goes through, put back by Escape; by a
//! label's drag, released anywhere, and kept apart from the canvas's typing, gestures and agents'
//! ops.

use gpui_kit::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use serde_json::json;

use super::{
    Scratch, design_file, draw_pen_path, edit_text, editing, gesture, open, open_in_window,
    panel_value, prop, rectangle, text_layer, undo, undo_steps,
};
use crate::board::layers_panel;
use sawit_board::design::Mixed;

#[gpui_kit::test]
fn scrub_is_one_undo_step(cx: &mut TestAppContext) {
    let tmp = Scratch::new("scrub-one-step");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let view = open(cx, &path, &ids);
    let steps = undo_steps(&view, cx);
    view.update(cx, |view, cx| {
        view.scrub_begin("x", cx);
        for step in 1..=20 {
            view.scrub_to("x", f64::from(step) * 5.0, cx);
        }
        view.scrub_end(true, cx);
    });
    assert_eq!(prop(&view, ids[0], "x", cx), json!(100.0));
    assert_eq!(undo_steps(&view, cx), steps + 1);
    // One undo takes the whole scrub back.
    view.update(cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    assert_eq!(prop(&view, ids[0], "x", cx), json!(0.0));
    assert_eq!(undo_steps(&view, cx), steps);
}

#[gpui_kit::test]
fn cancelled_scrub_restores(cx: &mut TestAppContext) {
    let tmp = Scratch::new("scrub-cancel");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let view = open(cx, &path, &ids);
    let steps = undo_steps(&view, cx);
    view.update(cx, |view, cx| {
        view.scrub_begin("x", cx);
        view.scrub_to("x", 30.0, cx);
        view.scrub_to("x", 60.0, cx);
    });
    // The canvas shows each value as it comes.
    assert_eq!(prop(&view, ids[0], "x", cx), json!(60.0));
    view.update(cx, |view, cx| view.scrub_end(false, cx));
    assert_eq!(prop(&view, ids[0], "x", cx), json!(0.0));
    assert_eq!(undo_steps(&view, cx), steps);
}

/// The scrub of a label's drag, from the press to a release anywhere (ruling P5-C14): 1 per
/// pixel, ten times as fast with ⇧, one undo step; an opacity moves 1% a pixel (P5-C7).
#[gpui_kit::test]
fn a_label_drag_scrubs_as_one_undo_step(cx: &mut TestAppContext) {
    let tmp = Scratch::new("label-drag");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    let start = cx.debug_bounds("design-handle-x").unwrap().center();
    cx.simulate_mouse_down(start, left, none);
    cx.simulate_mouse_move(start + point(px(10.), px(0.)), left, none);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(10.0));
    cx.simulate_mouse_move(start + point(px(12.), px(4.)), left, shift);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
    // Released over the canvas, far from the label: the scrub ends there, as one undo step.
    let canvas = point(px(layers_panel::WIDTH + 100.), px(300.));
    cx.simulate_mouse_up(canvas, left, none);
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    // The press selected nothing on the canvas, and drew nothing there.
    let selected = view.read_with(&cx, |view, _| view.board().unwrap().session.selection());
    assert_eq!(selected, ids);
    // An opacity moves 1% a pixel.
    let start = cx.debug_bounds("design-handle-opacity").unwrap().center();
    cx.simulate_mouse_down(start, left, none);
    cx.simulate_mouse_move(start - point(px(25.), px(0.)), left, none);
    cx.simulate_mouse_up(start - point(px(25.), px(0.)), left, none);
    assert_eq!(
        panel_value(&view, &ids, "opacity", &cx),
        Mixed::Same(json!(0.75))
    );
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    view.update(&mut cx, |view, cx| view.choose("edit.undo", json!({}), cx));
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
}

/// CANVAS-UI-8 for a label's scrub: the canvas's tab is hidden before the button comes up, so
/// nothing hears the release. Hiding ends the scrub, kept, as one undo step: the canvas's keys
/// work again (a scrub takes every key but Escape), and agents' edits are steps of their own.
#[gpui_kit::test]
fn hiding_the_canvas_ends_a_label_scrub_kept(cx: &mut TestAppContext) {
    let tmp = Scratch::new("label-hidden");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = cx.debug_bounds("design-handle-x").unwrap().center();
    cx.simulate_mouse_down(start, left, none);
    cx.simulate_mouse_move(start + point(px(10.), px(0.)), left, none);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(10.0));
    cx.update(|window, cx| {
        let home = cx.focus_handle();
        view.update(cx, |view, cx| view.set_shown(false, &home, window, cx));
    });
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(10.0));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
}

/// Ruling P5-C3: Escape during a label's scrub puts the value back, before the canvas can
/// take the key.
#[gpui_kit::test]
fn escape_puts_a_label_scrub_back(cx: &mut TestAppContext) {
    let tmp = Scratch::new("label-escape");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = cx.debug_bounds("design-handle-x").unwrap().center();
    let moved = start + point(px(30.), px(0.));
    cx.simulate_mouse_down(start, left, none);
    cx.simulate_mouse_move(moved, left, none);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
    cx.simulate_keystrokes("escape");
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    // The release that follows changes nothing more.
    cx.simulate_mouse_move(moved + point(px(20.), px(0.)), left, none);
    cx.simulate_mouse_up(moved, left, none);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
    assert_eq!(undo_steps(&view, &cx), steps);
    // The canvas didn't see Escape, which would have selected nothing.
    let selected = view.read_with(&cx, |view, _| view.board().unwrap().session.selection());
    assert_eq!(selected, ids);
}

/// Review C1: a label's drag while a text is edited on the canvas. The edit ends when the drag
/// begins, so what was typed is an undo step of its own: Escape puts back what the drag
/// changed, and keeps the typing.
#[gpui_kit::test]
fn escape_in_a_label_drag_keeps_the_text_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("label-drag-text");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[text_layer("Hello")]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let characters = |cx: &VisualTestContext| prop(&view, ids[0], "characters", cx);
    let steps = undo_steps(&view, &cx);
    edit_text(&view, ids[0], &mut cx);
    cx.simulate_input(" world");
    assert_eq!(characters(&cx), json!("Hello world"));
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = cx.debug_bounds("design-handle-x").unwrap().center();
    let moved = start + point(px(30.), px(0.));
    cx.simulate_mouse_down(start, left, none);
    // The typing ended there, as one undo step, and the drag has a gesture of its own.
    assert!(!editing(&view, &cx));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(gesture(&view, &cx).as_deref(), Some("x"));
    cx.simulate_mouse_move(moved, left, none);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
    cx.simulate_keystrokes("escape");
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
    assert_eq!(characters(&cx), json!("Hello world"));
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    cx.simulate_mouse_up(moved, left, none);
    assert_eq!(gesture(&view, &cx), None);
    // One undo takes back the typing.
    undo(&view, &mut cx);
    assert_eq!(characters(&cx), json!("Hello"));
    assert_eq!(view.read_with(&cx, |view, _| view.toast.clone()), None);
}

/// Review C1: a gesture of the canvas's own that the engine holds open, a pen path being
/// drawn, is not a scrub's to take over. A label's drag then scrubs nothing, and the path
/// keeps its points and its gesture, which its release doesn't end either.
#[gpui_kit::test]
fn a_label_drag_leaves_an_open_pen_path_alone(cx: &mut TestAppContext) {
    let tmp = Scratch::new("label-drag-pen");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let path_id = draw_pen_path(&view, &mut cx);
    let x = prop(&view, path_id, "x", &cx);
    let width = prop(&view, path_id, "width", &cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = cx.debug_bounds("design-handle-x").unwrap().center();
    let moved = start + point(px(30.), px(0.));
    cx.simulate_mouse_down(start, left, none);
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    cx.simulate_mouse_move(moved, left, none);
    cx.simulate_mouse_up(moved, left, none);
    assert_eq!(prop(&view, path_id, "x", &cx), x);
    assert_eq!(prop(&view, path_id, "width", &cx), width);
    assert_eq!(gesture(&view, &cx).as_deref(), Some("Pen"));
}

/// An agent's op during a label's drag ends the drag's scrub first, kept: the op is an undo
/// step of its own, which an Escape after it doesn't take back, and the rest of the drag
/// changes nothing.
#[gpui_kit::test]
fn an_op_during_a_label_drag_is_a_step_of_its_own(cx: &mut TestAppContext) {
    let tmp = Scratch::new("label-drag-op");
    let path = tmp.0.join("design.sawit");
    let ids = design_file(&path, &[rectangle(0.0)]);
    let (view, mut cx) = open_in_window(cx, &path, &ids);
    let steps = undo_steps(&view, &cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = cx.debug_bounds("design-handle-x").unwrap().center();
    cx.simulate_mouse_down(start, left, none);
    cx.simulate_mouse_move(start + point(px(30.), px(0.)), left, none);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
    // A read leaves the drag as it is.
    let read = json!({"op": "get"});
    let reply = view.update(&mut cx, |view, cx| view.run_op(&read, cx));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(view.read_with(&cx, |view, _| view.scrubbing_label()));
    let y = json!({"ids": ids, "props": {"y": 40}});
    let op = json!({"op": "command", "command": "node.set", "params": y});
    let reply = view.update(&mut cx, |view, cx| view.run_op(&op, cx));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    cx.run_until_parked();
    assert!(view.read_with(&cx, |view, _| view.scrub.is_none()));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    cx.simulate_keystrokes("escape");
    let moved = start + point(px(60.), px(0.));
    cx.simulate_mouse_move(moved, left, none);
    cx.simulate_mouse_up(moved, left, none);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
    assert_eq!(prop(&view, ids[0], "y", &cx), json!(40.0));
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    // The op is undone first, then the drag.
    undo(&view, &mut cx);
    assert_eq!(prop(&view, ids[0], "y", &cx), json!(0.0));
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(30.0));
    undo(&view, &mut cx);
    assert_eq!(prop(&view, ids[0], "x", &cx), json!(0.0));
}
