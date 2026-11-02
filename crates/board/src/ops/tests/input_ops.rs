//! The `pointer`, `key` and `type` ops.

use figcraft_engine::textedit;
use serde_json::json;

use super::*;
use crate::input::{self, KeyOutcome, Mods};

#[test]
fn pointer_op_drives_tools() {
    let mut board = board();
    let reply = send(&mut board, "tool.sticky", json!({}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    pointer_at(&mut board, "down", 0.0, 0.0);
    let reply = pointer_at(&mut board, "up", 0.0, 0.0);
    assert_eq!(elements(&mut board).len(), 1);
    // The answer says which tool is on, back to Move once the sticky is placed, and what is
    // selected, by id.
    assert_eq!(reply["tool"], json!("move"));
    let selection = reply["selection"].as_array().expect("a selection");
    assert_eq!(selection.len(), 1, "{reply}");
    assert!(
        selection[0]
            .as_str()
            .is_some_and(|id| id.parse::<u64>().is_ok()),
        "{reply}"
    );
}

#[test]
fn pointer_op_takes_page_points_whatever_the_view() {
    let mut board = design();
    // Zoomed in and panned: the op's points are page points all the same.
    board.session.ds_mut().unwrap().view = figcraft_engine::ViewState {
        cx: 37.0,
        cy: -12.0,
        zoom: 2.0,
        vw: 640.0,
        vh: 480.0,
        ..figcraft_engine::ViewState::default()
    };
    send(&mut board, "tool.rectangle", json!({}));
    for (kind, x, y) in [
        ("down", 10.0, 20.0),
        ("move", 60.0, 60.0),
        ("move", 110.0, 100.0),
        ("up", 110.0, 100.0),
    ] {
        pointer_at(&mut board, kind, x, y);
    }
    let elements = elements(&mut board);
    assert_eq!(elements.len(), 1, "{elements:?}");
    let drawn = &elements[0];
    let bounds = ["x", "y", "width", "height"].map(|key| drawn[key].as_f64().unwrap());
    assert_eq!(bounds, [10.0, 20.0, 100.0, 80.0], "{drawn}");
}

#[test]
fn pointer_op_leaves_the_views_requests_for_it() {
    let mut board = design();
    let made = board
        .execute(
            "node.create",
            &json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100}),
        )
        .unwrap();
    let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
    board.execute("select.none", &json!({})).unwrap();
    // The view hands the engine its frames' names as it paints them: this one just above the
    // frame's top-left corner.
    let corner = figcraft_geom::Point::new;
    board.session.tool.labels = vec![(
        frame,
        [
            corner(0.0, -20.0),
            corner(60.0, -20.0),
            corner(60.0, -4.0),
            corner(0.0, -4.0),
        ],
    )];

    let reply = pointer_at(&mut board, "double", 10.0, -12.0);

    // Renaming the frame there is the view's to do: the request waits for it.
    let kinds: Vec<&str> = (board.session.ui_requests.iter())
        .map(|request| request.kind.as_str())
        .collect();
    assert_eq!(kinds, ["canvas.renameLabel"]);
    assert_eq!(reply["selection"], json!([frame.to_string()]));
}

#[test]
fn malformed_pointer_ops_are_reported() {
    let mut board = board();
    send(&mut board, "tool.sticky", json!({}));
    for (op, says) in [
        (json!({"op": "pointer", "x": 0, "y": 0}), "kind"),
        (json!({"op": "pointer", "kind": 1, "x": 0, "y": 0}), "kind"),
        (
            json!({"op": "pointer", "kind": "tap", "x": 0, "y": 0}),
            "tap",
        ),
        (json!({"op": "pointer", "kind": "down"}), "x and y"),
        (
            json!({"op": "pointer", "kind": "down", "x": "0", "y": 0}),
            "x and y",
        ),
    ] {
        let reply = run(&mut board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains(says), "{op}: {error}");
    }
    // None of them reached the tools: no sticky was placed, and the tool is still on.
    assert!(elements(&mut board).is_empty());
    assert_eq!(board.session.tool.tool.id(), "sticky");
}

#[test]
fn key_op_routes_like_the_keyboard() {
    let mut board = board();
    let reply = run(&mut board, &json!({"op": "key", "key": "r"}));
    assert_eq!(reply, json!({"ok": true, "handled": true}));
    assert_eq!(board.session.tool.tool.id(), "rectangle");
    // With the modifier keys held: ⌘Z takes back the sticky.
    send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    assert!(press(&mut board, "z", json!({"cmd": true})));
    assert!(elements(&mut board).is_empty());
    // A key that is no shortcut, and a hidden command's (⌘N is file.new), aren't the canvas's.
    assert!(!press(&mut board, "g", json!({})));
    assert!(!press(&mut board, "n", json!({"cmd": true})));
    assert_eq!(board.session.docs.len(), 1);
    // The shortcut of a command Sawit carries out itself is the canvas's, though the op runs
    // nothing for it (⌘S, the save).
    assert!(press(&mut board, "s", json!({"cmd": true})));
}

/// The op presses the canvas's default keys as Sawit binds them, not FigCraft's raw keymap:
/// the keys Sawit keeps (⌘J toggles the terminal, ⌥⌘O adds a project) and the commands the
/// canvas doesn't offer (`/`, a live session's cursor chat) aren't the canvas's, as on the
/// keyboard, while its own keys still are.
#[test]
fn key_op_presses_the_canvas_default_keys() {
    let mut board = board();
    let made = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
    board.session.set_selection(vec![note]);
    let before = board.doc().unwrap();
    for (key, held) in [
        ("j", json!({"cmd": true})),
        ("o", json!({"alt": true, "cmd": true})),
        ("/", json!({})),
    ] {
        let mut op = json!({"op": "key", "key": key});
        for (modifier, on) in held.as_object().unwrap() {
            op[modifier] = on.clone();
        }
        let reply = run(&mut board, &op);
        assert_eq!(reply, json!({"ok": true, "handled": false}), "{op}");
        assert!(board.session.ui_requests.is_empty(), "{op}");
    }
    assert_eq!(board.doc().unwrap(), before);
    assert!(press(&mut board, "r", json!({})));
    assert_eq!(board.session.tool.tool.id(), "rectangle");
}

#[test]
fn key_op_leaves_the_views_requests_for_it() {
    let mut board = board();
    // ⌘K asks the view for the Actions palette: the request waits for it.
    assert!(press(&mut board, "k", json!({"cmd": true})));
    let kinds: Vec<&str> = (board.session.ui_requests.iter())
        .map(|request| request.kind.as_str())
        .collect();
    assert_eq!(kinds, ["palette.actions"]);
    board.session.ui_requests.clear();

    // ⇧⌘↩ while typing in a sticky makes the next one, and typing goes on there, as from the
    // keyboard: the op does what the engine asked of the view that needs no view.
    let made = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    let first = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
    assert!(figcraft_engine::board::edit_text(&mut board.session, first).unwrap());
    assert!(press(
        &mut board,
        "Enter",
        json!({"shift": true, "cmd": true})
    ));
    assert!(board.session.ui_requests.is_empty());
    let ds = board.session.ds().unwrap();
    let layers = &ds.doc.node(ds.page).unwrap().children;
    let [one, next] = layers[..] else {
        panic!("layers {layers:?}")
    };
    assert_eq!(one, first);
    let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
    assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
}

#[test]
fn malformed_key_ops_and_refusals_are_reported() {
    let mut board = board();
    for op in [
        json!({"op": "key"}),
        json!({"op": "key", "key": 5}),
        json!({"op": "key", "key": ""}),
    ] {
        let reply = run(&mut board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("a key op needs a key"), "{op}: {error}");
    }
    // A shortcut the engine refuses: ⌘G groups the selection, and nothing is selected.
    let reply = run(&mut board, &json!({"op": "key", "key": "g", "cmd": true}));
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().expect("an error string");
    assert!(error.contains("nothing is selected"), "{error}");
}

/// What an input method composes in the text being edited: shown there, not in the document.
fn composing(board: &BoardDoc) -> Option<String> {
    let ds = board.session.ds().unwrap();
    let preedit = ds.text_edit.as_ref()?.preedit.as_ref()?;
    Some(preedit.text.clone())
}

#[test]
fn typing_into_a_new_sticky() {
    let mut board = board();
    let note = typing_in_a_new_sticky(&mut board);
    textedit::insert(&mut board.session, "hi").unwrap();
    textedit::end(&mut board.session).unwrap();
    assert!(!editing(&board));
    assert_eq!(sticky_text(&board, note), "hi");
}

#[test]
fn ime_commit_replaces_preedit() {
    let mut board = board();
    let note = typing_in_a_new_sticky(&mut board);
    textedit::preedit(&mut board.session, "ni", None).unwrap();
    // Composed, it shows; the document gets it only once it is committed.
    assert_eq!(composing(&board).as_deref(), Some("ni"));
    assert_eq!(sticky_text(&board, note), "");
    textedit::insert_ime_commit(&mut board.session, "你").unwrap();
    assert_eq!(sticky_text(&board, note), "你");
    assert_eq!(composing(&board), None);
    assert!(editing(&board));
}

/// Review Focus 3, at the engine: Escape while composing ends the edit, and the composition
/// goes with it.
#[test]
fn cancelled_composition_leaves_no_text() {
    let mut board = board();
    let note = typing_in_a_new_sticky(&mut board);
    textedit::preedit(&mut board.session, "ni", None).unwrap();
    let escape = input::route_key(&mut board, "Escape", Mods::default());
    assert_eq!(escape, Ok(KeyOutcome::Handled));
    assert!(!editing(&board));
    assert_eq!(sticky_text(&board, note), "");
    // The sticky stays, emptied, as an emptied sticky does.
    assert_eq!(elements(&mut board).len(), 1);
}

/// Ruling P2-C18.
#[test]
fn type_op_types_text() {
    let mut board = board();
    // With no text edited there is nothing to type into.
    let reply = run(&mut board, &json!({"op": "type", "text": "hi"}));
    assert_eq!(reply, json!({"ok": false, "error": "not editing text"}));
    assert!(elements(&mut board).is_empty());

    // While a new sticky is edited, the text goes into it, and the edit goes on.
    let note = typing_in_a_new_sticky(&mut board);
    let reply = run(&mut board, &json!({"op": "type", "text": "hi"}));
    assert_eq!(reply, json!({"ok": true}));
    assert_eq!(sticky_text(&board, note), "hi");
    assert!(editing(&board));
    // On at the caret, as the keyboard types.
    let reply = run(&mut board, &json!({"op": "type", "text": " there"}));
    assert_eq!(reply, json!({"ok": true}));
    assert_eq!(sticky_text(&board, note), "hi there");
}

#[test]
fn type_op_follows_the_pointer_and_key_ops() {
    // An agent's sticky, as a mouse and a keyboard make one: the sticky tool's click starts
    // typing in the new sticky, the text goes in, and Escape ends the typing.
    let mut board = board();
    send(&mut board, "tool.sticky", json!({}));
    pointer_at(&mut board, "down", 0.0, 0.0);
    pointer_at(&mut board, "up", 0.0, 0.0);
    let reply = run(
        &mut board,
        &json!({"op": "type", "text": "Hello from Sawit"}),
    );
    assert_eq!(reply, json!({"ok": true}));
    assert!(press(&mut board, "Escape", json!({})));
    let elements = elements(&mut board);
    assert_eq!(elements.len(), 1, "{elements:?}");
    assert_eq!(elements[0]["text"], json!("Hello from Sawit"));
    let reply = run(&mut board, &json!({"op": "type", "text": "more"}));
    assert_eq!(reply, json!({"ok": false, "error": "not editing text"}));
}

#[test]
fn malformed_type_ops_are_reported() {
    let mut board = board();
    let note = typing_in_a_new_sticky(&mut board);
    for op in [json!({"op": "type"}), json!({"op": "type", "text": 5})] {
        let reply = run(&mut board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("a type op needs the text"), "{op}: {error}");
    }
    // Nothing was typed, and the edit goes on.
    assert_eq!(sticky_text(&board, note), "");
    assert!(editing(&board));
}
