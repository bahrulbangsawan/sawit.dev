//! The tests of the Effects section's model, and the layers and rows they share.

use figcraft_doc::{Effect, NodeId};
use serde_json::json;

use super::*;
use crate::design::lists::testing::{
    apply, design_file, edit, node_set, read, rectangle, undo_steps,
};
use crate::doc::BoardDoc;

mod edits;
mod kinds;
mod remove;
mod rows;

/// The effects the engine holds for layer `id`.
fn held(board: &BoardDoc, id: NodeId) -> Vec<Effect> {
    board.doc().unwrap().node(id).unwrap().effects.clone()
}

fn rows_of(board: &BoardDoc, ids: &[NodeId]) -> Vec<EffectRow> {
    effects(&read(board, ids)).unwrap()
}

/// A row of `kind` with the fields a kind that doesn't use them reads as.
fn row(kind: EffectKind) -> EffectRow {
    EffectRow {
        kind,
        visible: true,
        x: 0.0,
        y: 0.0,
        blur: 0.0,
        spread: 0.0,
        hex: "#000000".into(),
        opacity: 1.0,
    }
}

fn other(tag: &str) -> EffectKind {
    EffectKind::Other(tag.into())
}

/// A layer with `effects`, given as `node.set` takes them.
fn with_effects(board: &mut BoardDoc, effects: serde_json::Value) -> NodeId {
    let id = rectangle(board, "R", 0.0);
    node_set(board, &[id], json!({ "effects": effects }));
    id
}
