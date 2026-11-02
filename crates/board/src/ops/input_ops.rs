//! The input ops, `pointer`, `key` and `type`: what a mouse and a keyboard do on the canvas.

use figcraft_engine::{PointerKind, UiRequest, guard, textedit};
use serde_json::{Value, json};

use super::failure;
use crate::doc::BoardDoc;
use crate::input::{self, KeyOutcome, Mods};

/// The kinds of the `pointer` op, as an error lists them.
const POINTER_KINDS: &str = "down, move, up, double or hover";

pub(super) fn pointer(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(name) = op.get("kind").and_then(Value::as_str) else {
        return failure(format!("a pointer op needs a kind: {POINTER_KINDS}"));
    };
    let kind = match name {
        "down" => PointerKind::Down,
        "move" => PointerKind::Move,
        "up" => PointerKind::Up,
        "double" => PointerKind::DoubleClick,
        "hover" => PointerKind::Hover,
        other => return failure(format!("{other:?} is no pointer kind: use {POINTER_KINDS}")),
    };
    let number = |key: &str| op.get(key).and_then(Value::as_f64);
    let (Some(x), Some(y)) = (number("x"), number("y")) else {
        return failure("a pointer op needs x and y, a point on the page");
    };
    let held = |key: &str| op.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mods = Mods {
        shift: held("shift"),
        alt: held("alt"),
        cmd: held("cmd"),
        ctrl: false,
    };
    let event = match board.session.ds() {
        Ok(ds) => input::page_pointer_event(kind, (x, y), &ds.view, mods),
        Err(error) => return failure(error),
    };
    let requests = match input::pointer(board, &event) {
        Ok(requests) => requests,
        Err(error) => return failure(error),
    };
    leave_for_the_view(board, &requests);
    let selection: Vec<String> = (board.session.selection().iter())
        .map(ToString::to_string)
        .collect();
    json!({"ok": true, "selection": selection, "tool": board.session.tool.tool.id()})
}

pub(super) fn key(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(key) = op
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
    else {
        return failure(r#"a key op needs a key, like {"op": "key", "key": "Escape"}"#);
    };
    let held = |modifier: &str| op.get(modifier).and_then(Value::as_bool).unwrap_or(false);
    let mods = Mods {
        shift: held("shift"),
        alt: held("alt"),
        cmd: held("cmd"),
        ctrl: held("ctrl"),
    };
    let outcome = match input::route_default_key(board, key, mods) {
        Ok(outcome) => outcome,
        Err(error) => return failure(error),
    };
    let requests = std::mem::take(&mut board.session.ui_requests);
    leave_for_the_view(board, &requests);
    json!({"ok": true, "handled": outcome != KeyOutcome::Unhandled})
}

pub(super) fn type_text(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(text) = op.get("text").and_then(Value::as_str) else {
        return failure(r#"a type op needs the text to type, like {"op": "type", "text": "hi"}"#);
    };
    if !board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()) {
        return failure("not editing text");
    }
    let session = &mut board.session;
    match guard::run("ui.type", || textedit::insert(session, text)) {
        Ok(()) => json!({"ok": true}),
        Err(error) => failure(error),
    }
}

/// Carries out what an op's input asked of the view that needs none
/// ([`input::handle_requests`]), and leaves the rest in `session.ui_requests`, ahead of any made
/// since, as the same input from the window leaves them for the view.
fn leave_for_the_view(board: &mut BoardDoc, requests: &[UiRequest]) {
    let mut rest = input::handle_requests(board, requests);
    rest.append(&mut board.session.ui_requests);
    board.session.ui_requests = rest;
}
