//! The tests of the Fill and Stroke sections' model, and the layers and rows they share.

use figcraft_color::Paint;
use figcraft_doc::NodeId;
use serde_json::json;

use super::*;
use crate::design::lists::testing::{apply, design_file, node_set, read, rectangle, undo_steps};
use crate::doc::BoardDoc;

mod list;
mod replace;
mod rows;
mod solid;
mod strokes;

/// A 1 x 1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// The paints the engine holds for layer `id`, in list `key`.
fn held(board: &BoardDoc, id: NodeId, key: &str) -> Vec<Paint> {
    let doc = board.doc().unwrap();
    let node = doc.node(id).unwrap();
    if key == "fills" {
        node.fills.clone()
    } else {
        node.strokes.clone()
    }
}

fn solid(hex: &str, opacity: f64) -> PaintRow {
    PaintRow {
        kind: PaintKind::Solid {
            hex: hex.into(),
            opacity,
        },
        visible: true,
    }
}

fn gradient(label: &str) -> PaintRow {
    PaintRow {
        kind: PaintKind::Gradient(label.into()),
        visible: true,
    }
}

/// The rows of `key` for `ids`; the layers must agree.
fn rows_of(board: &BoardDoc, ids: &[NodeId], key: &str) -> Vec<PaintRow> {
    paints(&read(board, ids), key).unwrap()
}

/// A layer holding an image paint: the file's placed image.
fn image_layer(board: &mut BoardDoc) -> NodeId {
    let placed = board
        .execute("image.place", &json!({"data": PNG, "x": 200, "y": 0}))
        .unwrap();
    NodeId(placed["id"].as_u64().unwrap())
}
