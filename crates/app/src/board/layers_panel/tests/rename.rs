//! A name typed in a row: started from the keys, a double-click on a row or on a frame's name on
//! the canvas, and kept when a click, a drag or a scrub comes before Enter.

use std::path::Path;

use figcraft_doc::{Document, NodeId, NodeKind};
use gpui_kit::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use serde_json::json;

use super::{
    add, change, command, design_file, double_click, drawn, middle, middle_of, name_of, open,
    open_in_window, pages, row, selection, selector, toast, undo_steps, write,
};
use crate::board::BoardView;
use crate::board::tests::{Scratch, click, run_op};

/// Ruling P4-C11: Rename (⌘R, F2) shows the panel, opens the layers holding the selection, and
/// starts typing in the row of its top layer, which it selects alone.
#[gpui_kit::test]
fn rename_from_the_keys_starts_in_the_row_of_the_top_layer(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-rename-request");
    let path = tmp.0.join("design.sawit");
    let t = design_file(&path);
    let view = open(cx, &path);
    view.update(cx, |view, _| {
        view.show_layers = false;
        view.board_mut().unwrap().session.set_selection(vec![t.c]);
    });
    // `object.rename` without a name asks the panel for it.
    let reply = run_op(&view, command("object.rename"), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    view.read_with(cx, |view, _| {
        assert!(view.show_layers);
        assert!(view.expanded.contains(&t.f) && view.expanded.contains(&t.g));
        assert_eq!(view.renaming, Some(t.c));
    });
    // Of several layers, the top one listed: B, above A.
    view.update(cx, |view, cx| {
        view.renaming = None;
        view.board_mut()
            .unwrap()
            .session
            .set_selection(vec![t.a, t.b]);
        cx.notify();
    });
    run_op(&view, command("object.rename"), cx);
    assert_eq!(view.read_with(cx, |view, _| view.renaming), Some(t.b));
    assert_eq!(selection(&view, cx), [t.b]);
}

/// A click on a row selects its layer, and the canvas keeps the keyboard; a double-click types
/// a new name in the row, from the name the layer has: Enter keeps it, Escape drops it, and a
/// layer with no name keeps none when nothing was typed (P4.T1's rename ruling).
#[gpui_kit::test]
fn a_double_click_renames_in_the_row(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-rename-row");
    let path = tmp.0.join("design.sawit");
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let nameless = add(&mut doc, page, NodeKind::Rectangle, "");
    let card = add(&mut doc, page, NodeKind::Frame, "Card");
    write(&path, doc);
    let (view, mut cx) = open_in_window(cx, &path);
    let field = |cx: &VisualTestContext| {
        view.read_with(cx, |view, cx| {
            let field = view.layers.field.as_ref()?;
            Some(field.input.read(cx).value().to_string())
        })
    };
    let canvas_has_keyboard =
        |cx: &mut VisualTestContext| cx.update(|window, cx| view.read(cx).focus.is_focused(window));
    let steps = undo_steps(&view, &cx);
    click(&mut cx, row(card));
    cx.run_until_parked();
    assert_eq!(selection(&view, &cx), [card]);
    assert!(canvas_has_keyboard(&mut cx));

    let at = middle(&mut cx, row(card));
    double_click(&mut cx, at);
    assert_eq!(view.read_with(&cx, |view, _| view.renaming), Some(card));
    assert!(drawn(&mut cx, "layers-rename"));
    assert_eq!(field(&cx).as_deref(), Some("Card"));
    // What is typed replaces the name, which starts selected.
    cx.simulate_keystrokes("H e r o enter");
    cx.run_until_parked();
    assert_eq!(name_of(&view, card, &cx), "Hero");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert!(!drawn(&mut cx, "layers-rename"));
    assert!(canvas_has_keyboard(&mut cx));

    // Escape drops what was typed.
    let at = middle(&mut cx, row(card));
    double_click(&mut cx, at);
    cx.simulate_keystrokes("X escape");
    cx.run_until_parked();
    assert_eq!(name_of(&view, card, &cx), "Hero");
    assert_eq!(view.read_with(&cx, |view, _| view.renaming), None);
    assert!(!drawn(&mut cx, "layers-rename"));

    // A layer with no name: the row shows its kind, and the field starts empty.
    let at = middle(&mut cx, row(nameless));
    double_click(&mut cx, at);
    assert_eq!(field(&cx).as_deref(), Some(""));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(name_of(&view, nameless, &cx), "");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}

/// Plan 2's final review, carried: with the Move tool, a double-click on a frame's name on the
/// canvas starts typing its name in the panel, and a blank name keeps the old one.
#[gpui_kit::test]
fn a_double_click_on_a_frames_name_renames_it(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-rename-label");
    let path = tmp.0.join("design.sawit");
    let mut session = figcraft_engine::Session::new();
    let frame = json!({
        "type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100,
    });
    let made = session.execute("node.create", &frame).unwrap();
    let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
    session.execute("select.none", &json!({})).unwrap();
    session
        .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
        .unwrap();
    let (view, mut cx) = open_in_window(cx, &path);
    change(&view, &mut cx, |view, _| view.show_layers = false);
    let steps = undo_steps(&view, &cx);
    // The middle of the frame's name, where the canvas painted it.
    let label = view.read_with(&cx, |view, _| {
        let board = view.board().unwrap();
        let (id, quad) = board.session.tool.labels[0];
        assert_eq!(id, frame);
        let x = quad.iter().map(|p| p.x).sum::<f64>() / 4.0;
        let y = quad.iter().map(|p| p.y).sum::<f64>() / 4.0;
        let (x, y) = board.session.ds().unwrap().view.page_to_screen(x, y);
        view.bounds.origin + point(px(x as f32), px(y as f32))
    });
    double_click(&mut cx, label);
    assert!(view.read_with(&cx, |view, _| view.show_layers));
    assert_eq!(view.read_with(&cx, |view, _| view.renaming), Some(frame));
    assert!(drawn(&mut cx, "layers-rename"));
    cx.simulate_keystrokes("backspace enter");
    cx.run_until_parked();
    assert_eq!(name_of(&view, frame, &cx), "Frame");
    assert_eq!(undo_steps(&view, &cx), steps);
    assert_eq!(view.read_with(&cx, |view, _| view.renaming), None);
}

/// A design file with two 50 × 50 rectangles: Card at (0, 0) and Other at (200, 0).
fn card_and_other(path: &Path) -> (NodeId, NodeId) {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let card = add(&mut doc, page, NodeKind::Rectangle, "Card");
    let other = doc
        .create(page, NodeKind::Rectangle, "Other", 200.0, 0.0, 50.0, 50.0)
        .unwrap();
    write(path, doc);
    (card, other)
}

/// Double-clicks layer `id`'s row and types "Hero" in it, with no Enter: typed, and not kept
/// yet.
fn type_hero_in_the_row(view: &Entity<BoardView>, id: NodeId, cx: &mut VisualTestContext) {
    let at = middle(cx, row(id));
    double_click(cx, at);
    assert_eq!(view.read_with(cx, |view, _| view.renaming), Some(id));
    cx.simulate_keystrokes("H e r o");
    assert_eq!(name_of(view, id, cx), "Card");
}

/// The labels of the canvas file's last undo steps, the next one first.
fn undo_labels(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.board().unwrap().session.undo_labels(4))
}

/// CANVAS-UI-1, the Design panel's final review I1 for the layers panel: a name typed in a
/// row, then a press on a layer on the canvas, with no Enter. The name is kept before the
/// press reaches the tools, as an undo step of its own. The field hears that it lost the
/// keyboard only in the frame drawn after the press, inside the move the press opened, and a
/// click's release (a move that didn't move is cancelled) took the name back with it.
#[gpui_kit::test]
fn a_name_typed_then_a_click_on_a_layer_stays(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-typed-then-click");
    let path = tmp.0.join("design.sawit");
    let (card, other) = card_and_other(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    let steps = undo_steps(&view, &cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    // A click on the layer being renamed.
    type_hero_in_the_row(&view, card, &mut cx);
    let at = middle_of(&view, card, &cx);
    cx.simulate_mouse_down(at, left, none);
    cx.run_until_parked();
    cx.simulate_mouse_up(at, left, none);
    cx.run_until_parked();
    assert_eq!(name_of(&view, card, &cx), "Hero");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(undo_labels(&view, &cx)[0], "Rename");
    assert!(!drawn(&mut cx, "layers-rename"));
    // A click on another layer.
    change(&view, &mut cx, |view, cx| {
        view.rename_layer(card, "Card", cx)
    });
    let steps = undo_steps(&view, &cx);
    type_hero_in_the_row(&view, card, &mut cx);
    let at = middle_of(&view, other, &cx);
    cx.simulate_mouse_down(at, left, none);
    cx.run_until_parked();
    cx.simulate_mouse_up(at, left, none);
    cx.run_until_parked();
    assert_eq!(name_of(&view, card, &cx), "Hero");
    assert_eq!(selection(&view, &cx), [other]);
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}

/// CANVAS-UI-1, with a drag: the press that starts it keeps the typed name first, and the
/// drag's preview, rebuilt from the document as the press found it, keeps it too. The move and
/// the name are an undo step each.
#[gpui_kit::test]
fn a_name_typed_then_a_drag_on_a_layer_stays(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-typed-then-drag");
    let path = tmp.0.join("design.sawit");
    let (card, _) = card_and_other(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    let steps = undo_steps(&view, &cx);
    type_hero_in_the_row(&view, card, &mut cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let at = middle_of(&view, card, &cx);
    let x = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            doc.world_bounds(card).unwrap().x0
        })
    };
    let before = x(&cx);
    cx.simulate_mouse_down(at, left, none);
    cx.run_until_parked();
    for dx in [10., 20., 30., 40.] {
        cx.simulate_mouse_move(at + point(px(dx), px(0.)), left, none);
    }
    cx.simulate_mouse_up(at + point(px(40.), px(0.)), left, none);
    cx.run_until_parked();
    assert_eq!(name_of(&view, card, &cx), "Hero");
    assert!(x(&cx) > before, "the drag didn't move the layer");
    assert_eq!(undo_steps(&view, &cx), steps + 2);
    assert_eq!(undo_labels(&view, &cx)[..2], ["Move", "Rename"]);
}

/// CANVAS-UI-1 for a page: a page's name typed in its row, then a click on a layer.
#[gpui_kit::test]
fn a_page_name_typed_then_a_click_on_a_layer_stays(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-page-typed-then-click");
    let path = tmp.0.join("design.sawit");
    let (card, _) = card_and_other(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    let (_, page) = pages(&view, &cx);
    let steps = undo_steps(&view, &cx);
    let at = middle(&mut cx, selector(format!("layers-page-{}", page.0)));
    double_click(&mut cx, at);
    let renaming = view.read_with(&cx, |view, _| view.layers.renaming_page);
    assert_eq!(renaming, Some(page));
    cx.simulate_keystrokes("C o v e r");
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let at = middle_of(&view, card, &cx);
    cx.simulate_mouse_down(at, left, none);
    cx.run_until_parked();
    cx.simulate_mouse_up(at, left, none);
    cx.run_until_parked();
    assert_eq!(name_of(&view, page, &cx), "Cover");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
}

/// CANVAS-UI-1 for the Design panel's scrubs: a name typed in a row, then a scrub that is put
/// back with Escape, of a label's drag or of a colour picker. The name is kept before the
/// scrub begins, as an undo step of its own, which the Escape doesn't take back.
#[gpui_kit::test]
fn a_name_typed_then_a_scrub_put_back_stays(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-typed-then-scrub");
    let path = tmp.0.join("design.sawit");
    let (card, _) = card_and_other(&path);
    let (view, mut cx) = open_in_window(cx, &path);
    change(&view, &mut cx, |view, _| {
        let params = json!({"ids": [card], "props": {"fills": ["#D9D9D9"]}});
        view.board_mut()
            .unwrap()
            .execute("node.set", &params)
            .unwrap();
    });
    let steps = undo_steps(&view, &cx);
    // A label's drag, put back.
    type_hero_in_the_row(&view, card, &mut cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let start = cx.debug_bounds("design-handle-x").unwrap().center();
    cx.simulate_mouse_down(start, left, none);
    cx.run_until_parked();
    cx.simulate_mouse_move(start + point(px(10.), px(0.)), left, none);
    cx.simulate_keystrokes("escape");
    cx.simulate_mouse_up(start + point(px(10.), px(0.)), left, none);
    cx.run_until_parked();
    assert_eq!(name_of(&view, card, &cx), "Hero");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    // A colour picker, put back.
    change(&view, &mut cx, |view, cx| {
        view.rename_layer(card, "Card", cx)
    });
    let steps = undo_steps(&view, &cx);
    type_hero_in_the_row(&view, card, &mut cx);
    click(&mut cx, "design-fills-0-swatch");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(name_of(&view, card, &cx), "Hero");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}
