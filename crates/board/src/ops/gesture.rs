//! What an op does first about a gesture open on the canvas: waits for a drag or the Pen's path
//! to end, and ends the text being typed in.

use serde_json::Value;

use super::refusal::refused;
use super::{BATCH, failure};
use crate::doc::{BoardDoc, BoardError};
use crate::{input, policy};

/// What every op named `name` does first: waits for a drag on the canvas to end, and ends the
/// text being typed in. The refusal when it can't, else `None`.
pub(super) fn before_op(board: &mut BoardDoc, name: &str, op: &Value) -> Option<Value> {
    if let Some(wait) = waits_for_the_drag(board, name) {
        return Some(failure(format!("{name}: {wait}")));
    }
    if let Err(error) = end_text_edit_before_op(board, name, op) {
        return Some(failure(format!("{name}: {error}")));
    }
    None
}

/// The ops that change the document, which wait for a drag on the canvas to end
/// ([`waits_for_the_drag`]).
const CHANGES_THE_DOCUMENT: [&str; 8] = [
    "add", "update", "delete", "clear", "replace", "mermaid", "command", "begin",
];

/// What op `name` has to wait for on the canvas, if anything, said for its refusal: it changes
/// the document, and the user holds the button down there with a tool (a move, a resize, a shape
/// being drawn, a marquee...), as the engine's own `tool.place` refuses to place mid-drag; or a
/// gesture is open with no button held, which only the user ends: the path the Pen draws, between
/// its clicks, until Enter or Escape.
///
/// A gesture keeps the document it began with, and each move of it builds the document again
/// from that one (`Session::preview`), as an Escape puts that one back. So what an op did in
/// between was gone at the next move, though the op had answered ok (Plan 8 final review I-1).
/// And while a gesture is open no command is an undo step of its own: an op between the Pen's
/// clicks joined the path's step, and the ⌘Z after Enter took both away (CANVAS-CORE-8).
/// Refused, the op changes nothing, and the agent can send it again once the gesture is over.
/// The reads (`get`, `commands`, the exports) change nothing either, and the input of `pointer`,
/// `key` and `type` meets the gesture as the mouse and the keyboard do, so they go through. The
/// text being typed in is a gesture too, which the op ends first instead
/// ([`end_text_edit_before_op`]).
fn waits_for_the_drag(board: &BoardDoc, name: &str) -> Option<&'static str> {
    if !CHANGES_THE_DOCUMENT.contains(&name) {
        return None;
    }
    let session = &board.session;
    if session.tool.busy() {
        return Some("finish the drag on the canvas first");
    }
    let open = (session.ds()).is_ok_and(|ds| ds.interaction.is_some() && ds.text_edit.is_none());
    match (open, session.tool.pen.is_some()) {
        (false, _) => None,
        (true, true) => Some("finish the path the Pen draws on the canvas first"),
        (true, false) => Some("finish what is being drawn on the canvas first"),
    }
}

/// Ends the text being typed in on the canvas before op `name` (`op`) changes the document, as the
/// canvas's chrome and menus end it before their commands (ruling R-TEXTEDIT): what was typed
/// becomes an undo step of its own ("Edit text"), and the op one after it.
///
/// The engine types inside one gesture, which only the end of the edit commits, and while it is
/// open no command is an undo step of its own. An agent's op inside it joined the typing's step,
/// so the user's ⌘Z took both. An op that drops the edit without ending it (`select.none`,
/// `page.select`, deleting the text being typed in, a `replace`, whose old pages go) left the
/// gesture open, and the user's next click on a layer cancelled it: the typing and every op since
/// were gone, though each answered ok, and the autosave wrote that (Plan 7 final review I-2).
///
/// - `add`, `update`, `delete`, `clear`, `replace`, `mermaid` and `begin` end the edit.
/// - `command` ends it unless the command acts on the text being typed in, or changes nothing
///   ([`command_keeps_text_edit`]: the Text menu's commands, and a copy), or is a
///   `command.batch` whose every step does. A command refused up front runs nothing, and ends
///   nothing either ([`refused`]).
/// - The reads (`get`, `commands`, the exports) leave the edit alone, and so does the input of
///   `pointer`, `key` and `type`, which meets the edit as the mouse and the keyboard do.
///
/// With no text being typed in, it does nothing.
fn end_text_edit_before_op(board: &mut BoardDoc, name: &str, op: &Value) -> Result<(), BoardError> {
    let ends = match name {
        "add" | "update" | "delete" | "clear" | "replace" | "mermaid" | "begin" => true,
        "command" => op.get("command").and_then(Value::as_str).is_some_and(|id| {
            let params = op.get("params").unwrap_or(&Value::Null);
            refused(id, params).is_none() && !command_keeps_text_edit(id, params)
        }),
        _ => false,
    };
    if ends {
        input::end_text_edit(board)
    } else {
        Ok(())
    }
}

/// The clipboard's commands ([`policy::CLIPBOARD`]) that change layers when the engine runs them:
/// the `command` op ends the text being typed in before them ([`command_keeps_text_edit`]).
const ENDS_TEXT_EDIT_IN_THE_ENGINE: [&str; 4] = [
    "edit.cut",
    "edit.paste",
    "edit.pasteOver",
    "edit.pasteReplace",
];

/// Whether command `id`, with `params`, runs with the text being typed in still open, as the
/// engine runs it: it acts on that text (the Text menu's commands, [`policy::keeps_text_edit`]),
/// or it is a copy, which changes nothing, or it is a `command.batch` whose every step is one of
/// these, however deeply batches are nested.
///
/// The canvas's keys and menus keep the edit open for a cut and the pastes too, because they send
/// them to the text's characters. The engine acts on layers instead (CANVAS-CORE-2): its cut, and
/// its paste to replace, delete the text being typed in without ending its edit, which left the
/// "Edit text" gesture open for the next click to cancel, and its pastes joined the typing's undo
/// step. They end the edit first.
fn command_keeps_text_edit(id: &str, params: &Value) -> bool {
    if id != BATCH {
        return policy::keeps_text_edit(id) && !ENDS_TEXT_EDIT_IN_THE_ENGINE.contains(&id);
    }
    let steps = params.get("commands").and_then(Value::as_array);
    steps.is_some_and(|steps| {
        steps.iter().all(|step| {
            let id = step
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            command_keeps_text_edit(id, step.get("params").unwrap_or(&Value::Null))
        })
    })
}
