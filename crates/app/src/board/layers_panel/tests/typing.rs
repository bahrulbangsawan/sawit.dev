//! A page or a row picked in the panel while a sticky is typed in on the canvas: the typing ends
//! first, as one undo step (ruling R-TEXTEDIT).

use figcraft_doc::NodeId;
use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
use sawit_board::BoardDoc;
use serde_json::json;

use super::{
    change, double_click, middle_of, open_in_window, pages, row, selection, selector, toast,
    undo_steps,
};
use crate::board::BoardView;
use crate::board::tests::{Scratch, click};

/// Puts a 200 × 200 sticky reading `text` on the page on show of `board`, its top left corner
/// at page point (-100, -100), and returns it.
fn sticky(board: &mut BoardDoc, text: &str) -> NodeId {
    let params = json!({"x": -100, "y": -100, "text": text});
    let made = board.execute("board.sticky", &params).unwrap();
    figcraft_engine::params::id_of(&made["id"]).unwrap()
}

/// Whether a text is being typed in on the canvas.
fn typing(view: &Entity<BoardView>, cx: &TestAppContext) -> bool {
    view.read_with(cx, |view, _| view.editing_text())
}

/// What sticky `note` reads.
fn reads(view: &Entity<BoardView>, note: NodeId, cx: &TestAppContext) -> String {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, note).unwrap();
        let text = doc.node(text).and_then(|node| node.kind.text()).unwrap();
        text.characters.clone()
    })
}

/// The engine's gesture that is open, if one is: typing holds one, "Edit text", until the
/// typing ends.
fn gesture(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        let ds = view.board().unwrap().session.ds().unwrap();
        ds.interaction.as_ref().map(|gesture| gesture.label.clone())
    })
}

/// Shows the panel beside a board, whose canvas opens without it, and fits the view to the
/// canvas left beside it.
fn show_the_panel(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
    change(view, cx, |view, _| view.show_layers = true);
    change(view, cx, |view, cx| {
        view.choose("view.zoomFit", json!({}), cx)
    });
}

/// Typing in sticky `note` as a person does: a double-click on it, with the Move tool, and
/// "new" typed in place of its text.
fn type_new_in(view: &Entity<BoardView>, note: NodeId, cx: &mut VisualTestContext) {
    let at = middle_of(view, note, cx);
    double_click(cx, at);
    assert!(typing(view, cx), "the double-click started no typing");
    cx.simulate_input("new");
    assert_eq!(reads(view, note, cx), "new");
}

/// Ruling R-TEXTEDIT, and FINAL.P4's residual ruling that the controller carried to this plan:
/// a page picked in the panel while a sticky is typed in ends the typing first, as one undo
/// step. `page.select` alone dropped the text edit and left the engine's "Edit text" gesture
/// open: a Move-tool click on a layer of that page then cancelled the gesture, and the typing
/// was gone, with no undo step to bring it back.
#[gpui_kit::test]
fn a_page_picked_in_the_panel_while_typing_keeps_what_was_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-page-typing");
    let path = tmp.0.join("canvas.sawit");
    // A board of two pages, with a sticky on each: the first one reads "old".
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let note = sticky(&mut board, "old");
    board.execute("page.add", &json!({})).unwrap();
    let there = sticky(&mut board, "there");
    let second = board.session.page().unwrap();
    board.execute("page.select", &json!({"index": 0})).unwrap();
    board.execute("select.none", &json!({})).unwrap();
    std::fs::write(&path, board.to_bytes().unwrap()).unwrap();
    let (view, mut cx) = open_in_window(cx, &path);
    show_the_panel(&view, &mut cx);
    let steps = undo_steps(&view, &cx);
    type_new_in(&view, note, &mut cx);

    click(&mut cx, selector(format!("layers-page-{}", second.0)));
    cx.run_until_parked();
    assert_eq!(pages(&view, &cx).1, second);
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(undo_steps(&view, &cx), steps + 1);

    // A click on the sticky there, with the Move tool, selects it, and what was typed stays.
    cx.simulate_click(middle_of(&view, there, &cx), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(selection(&view, &cx), [there]);
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(reads(&view, note, &cx), "new");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}

/// Ruling R-TEXTEDIT: a layer's row clicked while a sticky is typed in ends the typing first,
/// through the canvas's one helper, as one undo step, and selects the row's layer. What was
/// typed stays after the next click on the canvas.
#[gpui_kit::test]
fn a_row_clicked_while_typing_keeps_what_was_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("layers-row-typing");
    let path = tmp.0.join("canvas.sawit");
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let note = sticky(&mut board, "old");
    let params = json!({"x": 300, "y": -100, "text": "other"});
    let made = board.execute("board.sticky", &params).unwrap();
    let other = figcraft_engine::params::id_of(&made["id"]).unwrap();
    board.execute("select.none", &json!({})).unwrap();
    std::fs::write(&path, board.to_bytes().unwrap()).unwrap();
    let (view, mut cx) = open_in_window(cx, &path);
    show_the_panel(&view, &mut cx);
    let steps = undo_steps(&view, &cx);
    type_new_in(&view, note, &mut cx);

    click(&mut cx, row(other));
    cx.run_until_parked();
    assert_eq!(selection(&view, &cx), [other]);
    assert!(!typing(&view, &cx));
    assert_eq!(gesture(&view, &cx), None);
    assert_eq!(undo_steps(&view, &cx), steps + 1);

    cx.simulate_click(middle_of(&view, other, &cx), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(selection(&view, &cx), [other]);
    assert_eq!(reads(&view, note, &cx), "new");
    assert_eq!(undo_steps(&view, &cx), steps + 1);
    assert_eq!(toast(&view, &cx), None);
}
