//! The overlay's tests: the fixtures they draw on, and helpers that pick out what was drawn.

use figcraft_doc::{Document, NodeId};
use figcraft_engine::{PointerEvent, PointerKind, Session, ToolId, textedit};
use serde_json::{Value, json};

use super::*;
use crate::doc::BoardDoc;

mod gestures;
mod handles;
mod names;
mod selection;
mod text_edit;
mod vector_edit;

/// Runs `command`, which makes a layer, and returns that layer.
fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
    let made = board.execute(command, &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

/// Shows `board` through a 1200 × 800 canvas at `zoom`, the page point (-100, -100) at the
/// canvas's top-left.
fn zoom(board: &mut BoardDoc, zoom: f64) {
    let (vw, vh) = (1200.0, 800.0);
    let view = json!({
        "zoom": zoom, "vw": vw, "vh": vh,
        "cx": -100.0 + vw / 2.0 / zoom, "cy": -100.0 + vh / 2.0 / zoom,
    });
    board.execute("view.set", &view).unwrap();
}

/// A design file with a 200 × 150 frame at (0, 0) and a 100 × 80 rectangle at (300, 0) on its
/// page, nothing selected, at 100 %: the frame's top-left is at (100, 100) on screen and the
/// rectangle's at (400, 100).
struct Fixture {
    board: BoardDoc,
    frame: NodeId,
    rect: NodeId,
}

fn fixture() -> Fixture {
    let mut board = BoardDoc {
        session: Session::with_document(Document::new("Design")),
    };
    let frame = make(
        &mut board,
        "node.create",
        json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 150}),
    );
    let rect = make(
        &mut board,
        "node.create",
        json!({
            "type": "rectangle", "name": "Box", "x": 300, "y": 0, "width": 100, "height": 80,
        }),
    );
    board.session.set_selection(Vec::new());
    zoom(&mut board, 1.0);
    Fixture { board, frame, rect }
}

fn select(board: &mut BoardDoc, ids: &[NodeId]) {
    board.session.set_selection(ids.to_vec());
}

fn hover(board: &mut BoardDoc, id: NodeId) {
    board.session.ds_mut().unwrap().hover = Some(id);
}

/// A pointer event at page point (`x`, `y`), as the canvas sends them.
fn pointer(board: &mut BoardDoc, kind: PointerKind, x: f64, y: f64) {
    let zoom = board.session.ds().unwrap().view.zoom;
    let event = PointerEvent::at(kind, x, y, zoom);
    board.session.pointer(&event).unwrap();
}

fn outlines(shapes: &[Shape]) -> Vec<&Shape> {
    let outline = |s: &&Shape| matches!(s, Shape::Outline { .. });
    shapes.iter().filter(outline).collect()
}

/// Where the handles are, and whether each is round.
fn handles(shapes: &[Shape]) -> Vec<((f64, f64), bool)> {
    let handle = |s: &Shape| match s {
        Shape::Handle { center, round, .. } => Some((*center, *round)),
        _ => None,
    };
    shapes.iter().filter_map(handle).collect()
}

/// The labels' text and the layer each names.
fn labels(shapes: &[Shape]) -> Vec<(&str, Option<NodeId>)> {
    let mut labels = Vec::new();
    for shape in shapes {
        if let Shape::Label { text, node, .. } = shape {
            labels.push((text.as_str(), *node));
        }
    }
    labels
}

/// The readouts (labels naming no layer).
fn readouts(shapes: &[Shape]) -> Vec<&Shape> {
    let readout = |s: &&Shape| matches!(s, Shape::Label { node: None, .. });
    shapes.iter().filter(readout).collect()
}

/// Where the square handles are: the selection box's corners.
fn box_handles(shapes: &[Shape]) -> Vec<(f64, f64)> {
    let square = |s: &Shape| match s {
        Shape::Handle {
            center,
            round: false,
            ..
        } => Some(*center),
        _ => None,
    };
    shapes.iter().filter_map(square).collect()
}

/// The round handles drawn white and outlined: where each is, and its size across.
fn round_handles(shapes: &[Shape]) -> Vec<((f64, f64), f64)> {
    let round = |s: &Shape| match s {
        Shape::Handle {
            center,
            size,
            round: true,
            fill: WHITE,
            ..
        } => Some((*center, *size)),
        _ => None,
    };
    shapes.iter().filter_map(round).collect()
}

/// Whether `shapes` hold the open 1 px line in the selection colour from `a` to `b`, but for
/// rounding.
fn has_line(shapes: &[Shape], a: (f64, f64), b: (f64, f64), width: f64) -> bool {
    shapes.iter().any(|s| {
            matches!(s, Shape::Outline { points, closed: false, color: SELECTION, width: w, dashed: false }
                if *w == width && points.len() == 2 && near(points[0], a) && near(points[1], b))
        })
}

/// Whether `a` and `b` are the same point on screen, but for rounding.
fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
}
