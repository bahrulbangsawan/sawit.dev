//! The tests of the Auto layout section's edits, and the frames they share.

use figcraft_doc::NodeId;
use serde_json::json;

use super::*;
use crate::design::Mixed;
use crate::design::testing::{
    apply, design_file, inspect, make, node_set, read, run, same, undo_steps,
};
use crate::doc::{BoardDoc, BoardError};

mod add_remove;
mod align;

/// A 200 × 100 frame called `name` at (`x`, 0), with two 30 × 30 rectangles in it and no auto
/// layout.
fn frame_with_two_children(board: &mut BoardDoc, name: &str, x: f64) -> NodeId {
    let frame = make(
        board,
        json!({"type": "frame", "name": name, "x": x, "y": 0, "width": 200, "height": 100}),
    );
    for child_x in [10, 60] {
        make(
            board,
            json!({"type": "rectangle", "parent": frame, "x": child_x, "y": 10,
                       "width": 30, "height": 30}),
        );
    }
    frame
}

/// A 200 × 100 frame at (`x`, 0) with auto layout in `direction`, no padding and no gap, that
/// keeps its size. It holds one 20 × 20 rectangle, so where that rectangle ends up says where
/// the layout puts its content. Returns the frame and the rectangle.
fn frame_with_one_child(board: &mut BoardDoc, direction: &str, x: f64) -> (NodeId, NodeId) {
    let frame = make(
        board,
        json!({"type": "frame", "x": x, "y": 0, "width": 200, "height": 100}),
    );
    let child = make(
        board,
        json!({"type": "rectangle", "parent": frame, "width": 20, "height": 20}),
    );
    node_set(board, &[frame], json!({"layout": direction}));
    (frame, child)
}

/// The auto gaps a frame's `mainAlign` can be, as `node.set` spells them.
const AUTO_GAPS: [&str; 3] = ["spaceBetween", "spaceAround", "spaceEvenly"];

/// A 200 × 100 frame at (`x`, 0) with auto layout in `direction` and the main alignment `main`,
/// no padding and no gap, that keeps its size. It holds two 20 × 20 rectangles, so where they
/// end up says how the layout spreads them: an auto gap puts the free space between them.
/// Returns the frame and the rectangles.
fn spread_frame(
    board: &mut BoardDoc,
    direction: &str,
    main: &str,
    x: f64,
) -> (NodeId, [NodeId; 2]) {
    let frame = make(
        board,
        json!({"type": "frame", "x": x, "y": 0, "width": 200, "height": 100}),
    );
    let children = [0, 1].map(|_| {
        make(
            board,
            json!({"type": "rectangle", "parent": frame, "width": 20, "height": 20}),
        )
    });
    node_set(
        board,
        &[frame],
        json!({"layout": direction, "mainAlign": main}),
    );
    (frame, children)
}

/// Where layer `id` sits in its frame.
fn place(board: &BoardDoc, id: NodeId) -> (f64, f64) {
    let layer = inspect(board, id);
    (layer["x"].as_f64().unwrap(), layer["y"].as_f64().unwrap())
}

/// The layers on the page, to tell whether a command made one.
fn layer_count(board: &BoardDoc) -> usize {
    board.doc().unwrap().nodes().count()
}
