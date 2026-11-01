//! The tests of the Design panel's model, and the file of two rectangles and a text they share.

use figcraft_doc::NodeId;
use serde_json::json;

use super::testing::{apply, design_file, make, node_set, read, same};
use super::*;
use crate::doc::BoardDoc;

mod edits;
mod layout_keys;
mod parse;
mod read;
mod sections;

/// A 100 × 100 rectangle called `name` at (`x`, 0).
fn rectangle(board: &mut BoardDoc, name: &str, x: f64) -> NodeId {
    let params =
        json!({"type": "rectangle", "name": name, "x": x, "y": 0, "width": 100, "height": 100});
    make(board, params)
}

/// The file the panel tests share: rectangles R1 (x 0) and R2 (x 50), and a text.
struct Fixture {
    board: BoardDoc,
    r1: NodeId,
    r2: NodeId,
    text: NodeId,
}

fn fixture() -> Fixture {
    let mut board = design_file();
    let r1 = rectangle(&mut board, "R1", 0.0);
    let r2 = rectangle(&mut board, "R2", 50.0);
    let text = make(
        &mut board,
        json!({"type": "text", "name": "Title", "text": "Hello", "x": 0, "y": 200}),
    );
    Fixture {
        board,
        r1,
        r2,
        text,
    }
}
