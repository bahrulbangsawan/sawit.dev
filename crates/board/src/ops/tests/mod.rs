//! The tests of the ops: what every op answers, and the helpers the topics share.

use std::sync::Arc;

use figcraft_doc::{BoardItem, Document, NodeId};
use figcraft_engine::params::id_of;
use figcraft_engine::textedit;
use serde_json::json;

use super::*;

mod command;
mod diagram;
mod documents;
mod elements;
mod export;
mod gesture;
mod import;
mod input_ops;
mod reads;
mod replace;
mod scenes;
mod screen;
mod update;
mod work;
mod writes;

fn board() -> BoardDoc {
    BoardDoc::new_board("Canvas").unwrap()
}

/// The elements `get` lists, after checking that it answered.
fn elements(board: &mut BoardDoc) -> Vec<Value> {
    let reply = run(board, &json!({"op": "get"}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    reply["elements"].as_array().unwrap().clone()
}

/// Sends command op `id` with `params`.
fn send(board: &mut BoardDoc, id: &str, params: Value) -> Value {
    run(
        board,
        &json!({"op": "command", "command": id, "params": params}),
    )
}

/// A second board in `dir`, as a `.sawit` file: the kind of file `file.recover` opens, and
/// `replace` and `file.import` bring in. It has two stickies, so it can't be taken for a
/// canvas that has one.
fn other_board_file(dir: &std::path::Path) -> String {
    let mut other = BoardDoc::new_board("Other").unwrap();
    for x in [40, 400] {
        other
            .execute("board.sticky", &json!({"x": x, "y": 40, "text": "other"}))
            .unwrap();
    }
    let path = dir.join("other.sawit");
    std::fs::write(&path, other.to_bytes().unwrap()).unwrap();
    path.to_string_lossy().into_owned()
}

/// A design file, which takes no board objects.
fn design() -> BoardDoc {
    let bytes = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
    BoardDoc::from_bytes(&bytes).unwrap()
}

/// Sends `op`, which has to answer `{ok: true, ...}`.
fn ask(board: &mut BoardDoc, op: Value) -> Value {
    let reply = run(board, &op);
    assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
    reply
}

/// Sends `op`, which has to answer `{ok: false, error}`, and gives the error.
fn refusal(board: &mut BoardDoc, op: Value) -> String {
    let reply = run(board, &op);
    assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
    let error = reply["error"].as_str().expect("an error string");
    error.to_string()
}

/// Adds `elements` with the `add` op and gives the ids it answered.
fn add(board: &mut BoardDoc, elements: Value) -> Vec<String> {
    let reply = ask(board, json!({"op": "add", "elements": elements}));
    let ids = reply["ids"].as_array().expect("ids");
    ids.iter()
        .map(|id| id.as_str().unwrap().to_string())
        .collect()
}

/// The element `get` lists under `id`.
fn listed(board: &mut BoardDoc, id: &str) -> Value {
    let found = elements(board).into_iter().find(|e| e["id"] == json!(id));
    found.unwrap_or_else(|| panic!("{id} is not listed"))
}

/// How many connector labels the document holds.
fn labels(board: &BoardDoc) -> usize {
    let doc = board.doc().unwrap();
    doc.nodes()
        .filter(|n| matches!(n.board, Some(BoardItem::Label { .. })))
        .count()
}

/// Runs `op` on a board with a sticky, on a thread of its own
/// ([`crate::store::within_seconds`]), and gives its reply, and whether the board's document is
/// still the one it had: `None` when it hadn't answered within 5 s.
fn on_a_board_of_its_own(op: Value) -> Option<(Value, bool)> {
    crate::store::within_seconds(move || {
        let mut board = board();
        send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let before = board.doc().unwrap();
        let reply = run(&mut board, &op);
        (reply, Arc::ptr_eq(&before, &board.doc().unwrap()))
    })
}

/// The scene of the Excalidraw fixture: nine layers on the page, and an embed it leaves out.
fn scene() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/excalidraw/basic.excalidraw"
    ))
    .unwrap()
}

/// Sends pointer op `kind` at page point (`x`, `y`), checks that it answered, and returns the
/// answer.
fn pointer_at(board: &mut BoardDoc, kind: &str, x: f64, y: f64) -> Value {
    let reply = run(
        board,
        &json!({"op": "pointer", "kind": kind, "x": x, "y": y}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    reply
}

/// Sends key op `key` with the modifier keys in `held` (`{"cmd": true}`), checks that it
/// answered, and returns whether the canvas took the key.
fn press(board: &mut BoardDoc, key: &str, held: Value) -> bool {
    let mut op = json!({"op": "key", "key": key});
    for (modifier, on) in held.as_object().expect("modifier keys") {
        op[modifier] = on.clone();
    }
    let reply = run(board, &op);
    assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
    reply["handled"].as_bool().expect("handled")
}

/// Puts a new, empty sticky on the board and starts typing in it, and returns the sticky. The
/// edit is of the sticky's text, a layer of its own (ruling P2-C6).
fn typing_in_a_new_sticky(board: &mut BoardDoc) -> NodeId {
    let made = send(board, "board.sticky", json!({"x": 0, "y": 0}));
    let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
    let text = figcraft_engine::board::text_of(&board.doc().unwrap(), note).unwrap();
    textedit::begin(&mut board.session, text, None, false).unwrap();
    note
}

/// What sticky `id` reads in the document.
fn sticky_text(board: &BoardDoc, id: NodeId) -> String {
    let doc = board.doc().unwrap();
    let text = figcraft_engine::board::text_of(&doc, id).unwrap();
    let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
    text.characters.clone()
}

fn editing(board: &BoardDoc) -> bool {
    board.session.ds().unwrap().text_edit.is_some()
}

/// A 100 × 100 rectangle on the board with its top left corner at (`x`, 0).
fn square_at(board: &mut BoardDoc, x: f64) -> NodeId {
    let params = json!({"shape": "rectangle", "x": x, "y": 0, "width": 100, "height": 100});
    let made = board.execute("board.shape", &params).unwrap();
    figcraft_engine::params::id_of(&made["id"]).unwrap()
}

/// Runs engine command `id` with `params`, and returns the layer it made.
fn made(board: &mut BoardDoc, id: &str, params: Value) -> NodeId {
    let made = board.execute(id, &params).unwrap();
    id_of(&made["id"]).unwrap()
}

#[test]
fn without_a_document_ops_answer_with_an_error() {
    let mut board = BoardDoc {
        session: figcraft_engine::Session::empty(),
    };
    for op in [
        json!({"op": "get"}),
        json!({"op": "add", "elements": []}),
        json!({"op": "update", "elements": []}),
        json!({"op": "delete", "ids": []}),
        json!({"op": "clear"}),
        json!({"op": "replace", "path": "/tmp/canvas.sawit"}),
        json!({"op": "replace", "scene": {"elements": []}}),
        json!({"op": "export_svg"}),
        json!({"op": "export_png"}),
        json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}}),
        json!({"op": "pointer", "kind": "down", "x": 0, "y": 0}),
        json!({"op": "type", "text": "hi"}),
    ] {
        let reply = run(&mut board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        assert!(reply["error"].as_str().is_some_and(|e| !e.is_empty()));
    }
}

#[test]
fn unknown_op_is_reported() {
    let mut board = board();
    let reply = run(&mut board, &json!({"op": "teleport", "to": [0, 0]}));
    assert_eq!(
        reply,
        json!({"ok": false, "error": "teleport is not supported yet"})
    );
    // An op with no name, or that isn't an op at all.
    for op in [json!({}), json!("get"), json!({"op": 3}), json!(null)] {
        let reply = run(&mut board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        assert!(reply["error"].as_str().is_some_and(|e| e.contains("op")));
    }
}
