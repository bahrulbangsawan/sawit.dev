//! What the tests of the design model's modules share: a design file, layers made with
//! `node.create`, a selection read from the file, and an edit run the way the panel runs it.

use figcraft_doc::{Document, NodeId};
use figcraft_engine::Session;
use serde_json::{Value, json};

use super::{DesignSelection, Edit, Mixed};
use crate::doc::BoardDoc;

/// A design file with nothing on its page.
pub(super) fn design_file() -> BoardDoc {
    BoardDoc {
        session: Session::with_document(Document::new("Design")),
    }
}

/// Makes a layer with `node.create` and returns its id.
pub(super) fn make(board: &mut BoardDoc, params: Value) -> NodeId {
    let made = board.execute("node.create", &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

/// What layers `ids` of `board` hold, as the panel reads them.
pub(super) fn read(board: &BoardDoc, ids: &[NodeId]) -> DesignSelection {
    DesignSelection::read(&board.doc().unwrap(), ids)
}

/// Runs `edit` as the panel does, and panics if the engine refuses it.
pub(super) fn apply(board: &mut BoardDoc, edit: &Edit) {
    board.execute(edit.command, &edit.params).unwrap();
}

/// Reads the layers `ids`, builds the edit `edit` makes for them and runs it, as the panel does.
pub(super) fn run(
    board: &mut BoardDoc,
    ids: &[NodeId],
    edit: impl FnOnce(&DesignSelection) -> Edit,
) {
    let edit = edit(&read(board, ids));
    apply(board, &edit);
}

/// `node.set` on `ids`, without the panel's model.
pub(super) fn node_set(board: &mut BoardDoc, ids: &[NodeId], props: Value) {
    board
        .execute("node.set", &json!({"ids": ids, "props": props}))
        .unwrap();
}

/// A layer's JSON as the engine's inspector gives it.
pub(super) fn inspect(board: &BoardDoc, id: NodeId) -> Value {
    figcraft_engine::inspect::node_json(&board.doc().unwrap(), id, 0)
}

/// How many undo steps the file has.
pub(super) fn undo_steps(board: &BoardDoc) -> usize {
    board.session.undo_depth().0
}

pub(super) fn same(value: Value) -> Mixed {
    Mixed::Same(value)
}
