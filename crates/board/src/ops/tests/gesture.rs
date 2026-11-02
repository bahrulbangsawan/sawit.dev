//! Ops while a gesture is open on the canvas: text being typed in, a drag, the Pen's path.

use std::sync::Arc;

use figcraft_doc::NodeId;
use figcraft_engine::textedit;
use serde_json::json;

use super::*;

/// Whether the engine's gesture is open: typing holds one, "Edit text", until the edit ends.
fn gesture_open(board: &BoardDoc) -> bool {
    board.session.ds().unwrap().interaction.is_some()
}

/// A new board with a sticky typed in, as a person types: "hi", the edit still open. The board
/// and the sticky.
fn typing_hi() -> (BoardDoc, NodeId) {
    let mut board = board();
    let note = typing_in_a_new_sticky(&mut board);
    textedit::insert(&mut board.session, "hi").unwrap();
    (board, note)
}

/// Plan 7 final review I-2 (ruling R-TEXTEDIT): an op that changes the document ends the text
/// the user is typing in first, as a click elsewhere does. What was typed is an undo step of
/// its own, and the op is one after it, which undo takes back alone. Inside the open edit an
/// op joined the typing's step, so the user's ⌘Z took both; and an op that dropped the edit
/// (Select none, deleting the text being typed, a replace) left its gesture open, so the
/// user's next click on a layer cancelled it, and the typing and every op since were gone.
#[test]
fn an_op_that_changes_the_document_ends_the_typing_first() {
    let (mut typed, note) = typing_hi();
    let steps = typed.session.undo_depth().0;
    let rectangle = json!([{"type": "rectangle", "x": 400, "y": 0}]);
    ask(&mut typed, json!({"op": "add", "elements": rectangle}));
    assert!(!editing(&typed) && !gesture_open(&typed));
    assert_eq!(typed.session.undo_depth().0, steps + 2);
    send(&mut typed, "edit.undo", json!({}));
    assert_eq!(elements(&mut typed).len(), 1);
    assert_eq!(sticky_text(&typed, note), "hi");

    // Each of the others: then undo takes back the op, and what was typed is there.
    let ops: [fn(NodeId) -> Value; 6] = [
        |note| json!({"op": "update", "elements": [{"id": note, "x": 50}]}),
        |note| json!({"op": "delete", "ids": [note]}),
        |_| json!({"op": "clear"}),
        |_| json!({"op": "replace", "scene": scene()}),
        |_| json!({"op": "mermaid", "source": "graph TD; A-->B"}),
        |_| json!({"op": "command", "command": "page.add"}),
    ];
    for op in ops {
        let (mut typed, note) = typing_hi();
        let op = op(note);
        ask(&mut typed, op.clone());
        assert!(!editing(&typed) && !gesture_open(&typed), "{op}");
        send(&mut typed, "edit.undo", json!({}));
        assert_eq!(sticky_text(&typed, note), "hi", "{op}");
    }

    // A selection change too, which is no undo step.
    let (mut typed, note) = typing_hi();
    send(&mut typed, "select.none", json!({}));
    assert!(!editing(&typed) && !gesture_open(&typed));
    assert_eq!(sticky_text(&typed, note), "hi");
}

/// Ruling R-TEXTEDIT for the command op: a command that acts on the text being typed in (the
/// Text menu's) or changes nothing (a copy) runs inside the edit, which goes on, and so does a
/// `command.batch` of only such steps; a batch with any other step ends the edit first. A
/// command refused up front runs nothing and ends nothing, and the reads and the input ops
/// leave the edit as it is.
#[test]
fn a_command_ends_the_typing_unless_it_acts_on_the_text() {
    let (mut board, note) = typing_hi();
    let center = json!({"command": "text.alignCenter"});
    for op in [
        json!({"op": "command", "command": "text.alignLeft"}),
        json!({"op": "command", "command": "edit.copy"}),
        json!({"op": "command", "command": "command.batch", "params": {"commands": [center]}}),
        json!({"op": "command", "command": "file.save"}),
        json!({"op": "get"}),
        json!({"op": "commands"}),
        json!({"op": "type", "text": "!"}),
    ] {
        run(&mut board, &op);
        assert!(editing(&board) && gesture_open(&board), "{op}");
    }
    assert_eq!(sticky_text(&board, note), "hi!");

    let steps = [center, json!({"command": "select.none"})];
    let batch = json!({"op": "command", "command": "command.batch", "params": {"commands": steps}});
    ask(&mut board, batch);
    assert!(!editing(&board) && !gesture_open(&board));
    assert_eq!(sticky_text(&board, note), "hi!");
}

/// CANVAS-CORE-2 (ruling R-TEXTEDIT): the canvas's keys and menus send copy and paste to the
/// characters of the text being typed in, but the `command` op hands them to the engine, whose
/// cut and pastes act on layers. A cut, or a paste to replace, deleted the text being typed
/// without ending its edit: the "Edit text" gesture stayed open, and the user's next click on
/// a layer cancelled it, taking back the typing, the cut and every op since, though each had
/// answered ok. A paste joined the typing's undo step. Each now ends the typing first: it is
/// an undo step of its own, which the next click keeps, and the op is one after it.
#[test]
fn a_cut_or_a_paste_ends_the_typing_first() {
    let cut = json!({"command": "edit.cut"});
    let copy = json!({"command": "edit.copy"});
    for op in [
        cut.clone(),
        json!({"command": "edit.pasteReplace"}),
        json!({"command": "edit.paste"}),
        json!({"command": "edit.pasteOver"}),
        json!({"command": "command.batch", "params": {"commands": [copy, cut]}}),
    ] {
        let mut op = op;
        op["op"] = json!("command");
        // A rectangle on the page and on the clipboard, and a sticky being typed in.
        let mut board = board();
        let rectangle = json!([{"type": "rectangle", "x": 600, "y": 0, "width": 100,
                "height": 100}]);
        let added = ask(&mut board, json!({"op": "add", "elements": rectangle}));
        send(&mut board, "edit.copy", json!({"ids": added["ids"]}));
        let note = typing_in_a_new_sticky(&mut board);
        textedit::insert(&mut board.session, "hi").unwrap();

        ask(&mut board, op.clone());
        assert!(
            editing(&board) || !gesture_open(&board),
            "{op}: the typing's gesture is open with no text being typed in"
        );
        // The user's next click, on the rectangle, takes nothing back.
        let shown = board.doc().unwrap();
        pointer_at(&mut board, "down", 650.0, 50.0);
        pointer_at(&mut board, "up", 650.0, 50.0);
        assert!(
            *board.doc().unwrap() == *shown,
            "{op}: the click took edits back"
        );
        // Undo takes back the op alone: the sticky is there, with what was typed.
        send(&mut board, "edit.undo", json!({}));
        assert_eq!(sticky_text(&board, note), "hi", "{op}");
    }
}

/// The middle of the box `get` gives `element`, on the page.
fn middle(element: &Value) -> (f64, f64) {
    let [x, y, width, height] =
        ["x", "y", "width", "height"].map(|key| element[key].as_f64().unwrap());
    (x + width / 2.0, y + height / 2.0)
}

/// Plan 8 final review I-1 (the controller's FINAL.P8 ruling): while the user holds a drag on
/// the canvas, an op that changes the document is refused and changes nothing. Each move of
/// the drag builds the document again from the one the drag began with (`Session::preview`),
/// so an op run inside it was taken back by the next move, or by an Escape, though it had
/// answered ok. Reads still answer during the drag; once the button is up the op goes
/// through, and what it drew stays through the next drag.
/// CANVAS-CORE-9: a panic while an op runs is the op's error, as a panic in an engine command
/// is, and the canvas goes on: in the app it unwound into the window's callbacks, which
/// aborted the app with every tab.
#[test]
fn a_panic_in_an_op_is_its_error() {
    let mut board = board();
    let op = json!({"op": "add", "elements": []});
    let reply = run_guarded(&mut board, &op, |_, _| panic!("a bug in the op"));
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().unwrap();
    assert!(
        error.starts_with("internal error in `add`: a bug in the op"),
        "{error}"
    );
    let sticky = json!([{"type": "sticky", "x": 0, "y": 0}]);
    ask(&mut board, json!({"op": "add", "elements": sticky}));
    assert_eq!(elements(&mut board).len(), 1);
}

/// CANVAS-CORE-8: between the Pen's clicks no button is held, but the path being drawn is an
/// open gesture, which only Enter (or Escape) ends. An agent's op ran inside it and joined
/// its undo step: the ⌘Z after Enter took the path and the agent's layers away together. Such
/// an op waits for the path, as one waits for a drag, and changes nothing; once the path is
/// ended it goes through, an undo step of its own.
#[test]
fn an_op_that_changes_the_document_waits_for_the_pens_path() {
    let mut board = board();
    send(&mut board, "tool.pen", json!({}));
    pointer_at(&mut board, "down", 0.0, 0.0);
    pointer_at(&mut board, "up", 0.0, 0.0);
    pointer_at(&mut board, "down", 100.0, 0.0);
    pointer_at(&mut board, "up", 100.0, 0.0);
    assert!(board.session.tool.pen.is_some() && !board.session.tool.busy());
    let held = board.doc().unwrap();
    let rectangle = json!([{"type": "rectangle", "x": 400, "y": 0}]);
    for op in [
        json!({"op": "add", "elements": rectangle}),
        json!({"op": "command", "command": "page.add"}),
    ] {
        let name = op["op"].as_str().unwrap();
        let reply = run(&mut board, &op);
        let error = format!("{name}: finish the path the Pen draws on the canvas first");
        assert_eq!(reply, json!({"ok": false, "error": error}), "{op}");
        assert!(Arc::ptr_eq(&board.doc().unwrap(), &held), "{op}");
    }
    assert!(press(&mut board, "Enter", json!({})));
    assert!(board.session.tool.pen.is_none() && !gesture_open(&board));
    let path = board.doc().unwrap();
    ask(&mut board, json!({"op": "add", "elements": rectangle}));
    send(&mut board, "edit.undo", json!({}));
    assert!(*board.doc().unwrap() == *path, "the undo took the path too");
}

#[test]
fn an_op_that_changes_the_document_waits_for_the_drag_to_end() {
    let mut board = board();
    let made = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
    let (x, y) = middle(&elements(&mut board)[0]);
    // The Move tool presses on the sticky and drags it past the slop, and the button stays
    // down.
    pointer_at(&mut board, "down", x, y);
    pointer_at(&mut board, "move", x + 40.0, y + 30.0);
    assert!(board.session.tool.busy());
    let held = board.doc().unwrap();
    let steps = board.session.undo_depth();

    let rectangle = json!([{"type": "rectangle", "x": 600, "y": 0}]);
    for op in [
        json!({"op": "add", "elements": rectangle}),
        json!({"op": "update", "elements": [{"id": note.to_string(), "x": 300}]}),
        json!({"op": "delete", "ids": [note.to_string()]}),
        json!({"op": "clear"}),
        json!({"op": "replace", "scene": scene()}),
        json!({"op": "mermaid", "source": "graph TD; A-->B"}),
        json!({"op": "command", "command": "board.sticky", "params": {"x": 900, "y": 0}}),
    ] {
        let name = op["op"].as_str().unwrap();
        let error = format!("{name}: finish the drag on the canvas first");
        assert_eq!(run(&mut board, &op), json!({"ok": false, "error": error}));
        // The page is as the drag shows it, and the drag goes on.
        assert!(Arc::ptr_eq(&board.doc().unwrap(), &held), "{op}");
        assert!(board.session.tool.busy() && gesture_open(&board), "{op}");
    }
    // The reads answer.
    assert_eq!(elements(&mut board).len(), 1);
    ask(&mut board, json!({"op": "commands"}));
    ask(&mut board, json!({"op": "export_svg"}));
    assert_eq!(board.session.undo_depth(), steps);

    // The drag ends as it would have: the sticky moved, in one undo step.
    pointer_at(&mut board, "move", x + 60.0, y + 50.0);
    pointer_at(&mut board, "up", x + 60.0, y + 50.0);
    assert!(!board.session.tool.busy());
    assert_eq!(board.session.undo_depth().0, steps.0 + 1);
    let moved = elements(&mut board);
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(
        (moved[0]["x"].clone(), moved[0]["y"].clone()),
        (json!(60.0), json!(50.0))
    );

    // Then the op goes through, and the next drag keeps what it drew.
    let added = ask(&mut board, json!({"op": "add", "elements": rectangle}));
    let drawn = added["ids"][0].clone();
    let (x, y) = middle(&moved[0]);
    pointer_at(&mut board, "down", x, y);
    pointer_at(&mut board, "move", x + 20.0, y + 20.0);
    pointer_at(&mut board, "up", x + 20.0, y + 20.0);
    let after = elements(&mut board);
    assert_eq!(after.len(), 2, "{after:?}");
    let kept = after.iter().find(|element| element["id"] == drawn);
    assert_eq!(kept.map(|element| element["x"].clone()), Some(json!(600.0)));
}
