//! The importer's tests, by what they test.

mod brief;
// What the brief doesn't name.
mod arrows;
mod bindings;
mod elements;
mod files;
mod import;

use figcraft_doc::{BoardItem, End as DocEnd, NodeId, StrokeCap};
use serde_json::{Value, json};

use super::*;
use crate::ops;
use crate::skeleton::End;

/// The scene of the `basic.excalidraw` fixture.
fn basic() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/excalidraw/basic.excalidraw"
    ))
    .unwrap()
}

fn board() -> BoardDoc {
    BoardDoc::new_board("Canvas").unwrap()
}

/// What `get` lists on the board.
fn elements(board: &mut BoardDoc) -> Vec<Value> {
    let reply = ops::run(board, &json!({"op": "get"}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    reply["elements"].as_array().unwrap().clone()
}

/// The elements of `kind` among `elements`.
fn of_type<'a>(elements: &'a [Value], kind: &str) -> Vec<&'a Value> {
    let is = |e: &&Value| e["type"] == json!(kind);
    elements.iter().filter(is).collect()
}

/// A scene of `elements`, with the other keys a saved one has.
fn scene(elements: Value) -> Value {
    json!({"type": "excalidraw", "version": 2, "elements": elements, "files": {}})
}

/// The skeletons of a scene of `elements`, and the notes it gave.
fn read(elements: Value) -> Imported {
    to_skeletons(&scene(elements)).unwrap()
}

/// The skeleton with agent id `id`.
fn with_id<'a>(imported: &'a Imported, id: &str) -> &'a Skeleton {
    let found = imported
        .skeletons
        .iter()
        .find(|s| s.id.as_deref() == Some(id));
    found.unwrap_or_else(|| panic!("no skeleton `{id}` in {:?}", imported.skeletons))
}

/// A rectangle, as little of it as the importer reads.
fn rectangle(id: &str, x: f64, y: f64) -> Value {
    json!({"id": id, "type": "rectangle", "x": x, "y": y, "width": 100, "height": 50})
}

/// A text bound to the element `container`.
fn label(id: &str, container: &str, text: &str) -> Value {
    json!({"id": id, "type": "text", "x": 0, "y": 0, "text": text, "originalText": text,
               "containerId": container})
}

/// An arrow from `at` along `points`, as Excalidraw writes it: `heads` are its
/// `startArrowhead` and `endArrowhead` (`null` is none), and `joins` are the elements it is
/// bound to at its start and at its end.
fn arrow(
    id: &str,
    at: (f64, f64),
    points: Value,
    heads: (Value, Value),
    joins: (Option<&str>, Option<&str>),
) -> Value {
    let bound = |to: Option<&str>| match to {
        Some(id) => json!({"elementId": id, "focus": 0, "gap": 1}),
        None => json!(null),
    };
    json!({"id": id, "type": "arrow", "x": at.0, "y": at.1, "points": points,
               "startArrowhead": heads.0, "endArrowhead": heads.1,
               "startBinding": bound(joins.0), "endBinding": bound(joins.1)})
}

/// The rectangles `a` and `b`, side by side, and `link` after them.
fn between(link: Value) -> Value {
    json!([rectangle("a", 0.0, 0.0), rectangle("b", 300.0, 0.0), link])
}

/// The elements of a scene drawn on a new board, with nothing left out.
fn drawn(elements: Value) -> BoardDoc {
    let mut board = board();
    let notes = import(&mut board, &scene(elements)).unwrap();
    assert_eq!(notes, Vec::<String>::new());
    board
}

/// The node on `board` that the element `id` of the scene is.
fn node_of(board: &BoardDoc, id: &str) -> Option<NodeId> {
    skeleton::resolve(&board.doc().unwrap(), id)
}

/// The connector that the element `id` of the scene is on `board`: its two ends, and the caps
/// of its line (its arrowheads).
fn connector(board: &BoardDoc, id: &str) -> (DocEnd, DocEnd, StrokeCap, StrokeCap) {
    let doc = board.doc().unwrap();
    let node = doc.node(skeleton::resolve(&doc, id).unwrap()).unwrap();
    let Some(BoardItem::Connector(connector)) = node.board.clone() else {
        panic!("`{id}` is {:?}, not a connector", node.board);
    };
    (
        connector.from,
        connector.to,
        node.stroke.cap_start,
        node.stroke.cap_end,
    )
}

/// An end of a skeleton that joins the element `id`.
fn joined(id: &str) -> End {
    End {
        id: Some(id.into()),
        x: None,
        y: None,
    }
}

/// A free end of a skeleton, at the point `(x, y)` of the page.
fn loose(x: f64, y: f64) -> End {
    End {
        id: None,
        x: Some(x),
        y: Some(y),
    }
}
