//! An artboard's checks, from its layers.

use figcraft_doc::{Document, NodeId};
use figcraft_engine::Session;
use serde_json::{Value, json};

use super::super::layers::check;
use crate::doc::BoardDoc;

fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
    let made = board.execute(command, &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

/// A phone artboard with 10 px grey text, a 30 px button and a gap of 7 fails the text floor,
/// contrast, the target size and the spacing scale; its well-made text passes.
#[test]
fn an_artboards_layers_are_checked() {
    let mut board = BoardDoc {
        session: Session::with_document(Document::new("Design")),
    };
    let frame = make(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 393, "height": 852, "name": "Settings / Mobile",
               "props": {"fill": "#ffffff"}}),
    );
    let ok = json!({"x": 16, "y": 16, "text": "Settings", "parent": frame,
                    "props": {"fontSize": 24, "fill": "#111827"}});
    make(&mut board, "text.create", ok);
    let small = json!({"x": 16, "y": 64, "text": "Terms apply", "parent": frame,
                       "name": "Legal", "props": {"fontSize": 10, "fill": "#aaaaaa"}});
    let legal = make(&mut board, "text.create", small);
    let button = make(
        &mut board,
        "frame.create",
        json!({"x": 16, "y": 120, "width": 160, "height": 30, "parent": frame,
               "name": "Save button", "props": {"fill": "#1d4ed8"}}),
    );
    let row = make(
        &mut board,
        "frame.create",
        json!({"x": 16, "y": 200, "width": 200, "height": 40, "parent": frame, "name": "Row",
               "props": {"layout": "horizontal", "gap": 7, "padding": 8}}),
    );
    let doc = board.doc().unwrap();
    let found: Vec<(String, String)> = check(&doc, frame)
        .into_iter()
        .map(|f| (f.rule, f.id.unwrap()))
        .collect();
    assert_eq!(
        found,
        [
            ("typ.11".to_string(), legal.to_string()),
            ("col.10".to_string(), legal.to_string()),
            ("a11y.2".to_string(), button.to_string()),
            ("spc.1".to_string(), row.to_string()),
        ]
    );
}
