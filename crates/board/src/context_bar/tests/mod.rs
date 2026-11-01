//! The context bar's tests: the board they run on, the items they select, and the bar they read.

use figcraft_doc::NodeId;
use serde_json::json;

use super::*;
use crate::doc::BoardDoc;

mod anchor;
mod controls;
mod options;

/// A new board seen through an 800 × 600 canvas, centred on the page point (100, 100) at 100 %:
/// the page origin is at (300, 200) on screen.
fn board() -> BoardDoc {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let view = json!({"cx": 100, "cy": 100, "zoom": 1, "vw": 800, "vh": 600});
    board.execute("view.set", &view).unwrap();
    board
}

/// Runs `command` on `board`, which selects what it makes, and returns that layer.
fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
    let made = board.execute(command, &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

/// A sticky at (0, 0), 200 px wide, in the default yellow.
fn sticky(board: &mut BoardDoc) -> NodeId {
    make(board, "board.sticky", json!({"x": 0, "y": 0}))
}

/// A rectangle with text at (300, 0), 160 px square, in the default yellow and size M.
fn shape(board: &mut BoardDoc) -> NodeId {
    let params = json!({"shape": "rectangle", "x": 300, "y": 0});
    make(board, "board.shape", params)
}

/// An elbow connector with an arrow at its end, between two free points.
fn connector(board: &mut BoardDoc) -> NodeId {
    let params = json!({"from": {"x": 0, "y": 400}, "to": {"x": 300, "y": 400}});
    make(board, "board.connector", params)
}

/// A marker stroke.
fn ink(board: &mut BoardDoc) -> NodeId {
    make(
        board,
        "board.ink",
        json!({"points": [[0, 600], [100, 650]]}),
    )
}

fn select(board: &mut BoardDoc, ids: &[NodeId]) {
    board.session.set_selection(ids.to_vec());
}

/// The colours control showing `current`.
fn colors(current: Option<&'static str>) -> Control {
    Control::Colors {
        items: palette(),
        current,
    }
}

/// The bar for the selection now.
fn bar(board: &BoardDoc) -> ContextBar {
    context_bar(&board.session).unwrap()
}

/// The controls' names, in order.
fn kinds(bar: &ContextBar) -> Vec<&'static str> {
    bar.controls
        .iter()
        .map(|control| match control {
            Control::Colors { .. } => "colors",
            Control::Shape { .. } => "shape",
            Control::FontSize { .. } => "font size",
            Control::Routing { .. } => "routing",
            Control::StartCap { .. } => "start cap",
            Control::EndCap { .. } => "end cap",
        })
        .collect()
}
