//! The `mermaid` op: where it puts a diagram, what it draws, and what it says when it can't.

use std::sync::Arc;

use figcraft_doc::{BoardItem, BoardShape};
use kurbo::Rect;

use super::*;

/// The box around layers `ids` on the page, read from the document.
fn bounds_of(board: &BoardDoc, ids: &[NodeId]) -> Rect {
    let doc = board.doc().unwrap();
    doc.bounds_of(ids).expect("layers with bounds")
}

#[test]
fn a_pie_is_drawn_as_one_imported_picture() {
    let mut board = board();
    let reply = ops::run(&mut board, &json!({"op": "mermaid", "source": PIE}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(reply["added"], json!(1), "{reply}");
    assert!(reply.get("notes").is_none(), "{reply}");
    let id: NodeId = reply["ids"][0].as_str().unwrap().parse().unwrap();
    // At the top left of the empty page, as big as Pele drew it.
    let diagram = render(PIE, &Palette::default()).unwrap();
    let at = bounds_of(&board, &[id]);
    assert!(near(at.x0, 0.0) && near(at.y0, 0.0), "{at:?}");
    assert!(
        near(at.width(), diagram.width) && near(at.height(), diagram.height),
        "{at:?}"
    );
}

#[test]
fn mermaid_syntax_error_is_reported() {
    let mut board = board();
    let before = board.doc().unwrap();
    let reply = ops::run(
        &mut board,
        &json!({"op": "mermaid", "source": "flowchart LR; A-->"}),
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().expect("an error string");
    assert!(error.starts_with("mermaid: "), "{error}");
    // Pele's own words, which quote the source.
    assert!(error.contains("Parse error"), "{error}");
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
}

#[test]
fn mermaid_op_places_right_of_content() {
    let mut board = board();
    let sticky = board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "first"}))
        .unwrap();
    let sticky: NodeId = sticky["id"].to_string().parse().unwrap();
    let there = bounds_of(&board, &[sticky]);
    assert!(near(there.x0, 0.0) && near(there.x1, 200.0), "{there:?}");

    let ids = draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
    let placed = bounds_of(&board, &ids);
    assert!(placed.x0 >= 300.0 - 1e-6, "{placed:?}");
    // 100 px right of the sticky, level with its top.
    assert!(near(placed.x0, 300.0) && near(placed.y0, 0.0), "{placed:?}");
}

#[test]
fn the_op_puts_a_diagram_at_x_y_or_at_the_origin_of_an_empty_page() {
    let mut board = board();
    let first = draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
    let at = bounds_of(&board, &first);
    assert!(near(at.x0, 0.0) && near(at.y0, 0.0), "{at:?}");

    let ids = draw(
        &mut board,
        json!({"op": "mermaid", "source": FLOW, "x": -500, "y": 1000}),
    );
    let at = bounds_of(&board, &ids);
    assert!(near(at.x0, -500.0) && near(at.y0, 1000.0), "{at:?}");

    // Only x: the diagram is still level with the top of what the page holds.
    let ids = draw(
        &mut board,
        json!({"op": "mermaid", "source": FLOW, "x": 2000}),
    );
    let at = bounds_of(&board, &ids);
    assert!(near(at.x0, 2000.0) && near(at.y0, 0.0), "{at:?}");
}

#[test]
fn the_op_draws_shapes_with_connectors_bound_to_them() {
    let mut board = board();
    let ids = draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
    let doc = board.doc().unwrap();
    let text = |id: NodeId| {
        let text = figcraft_engine::board::text_of(&doc, id)?;
        Some(doc.node(text)?.kind.text()?.characters.clone())
    };
    let mut shapes = Vec::new();
    let mut connectors = Vec::new();
    for id in &ids {
        match doc.node(*id).and_then(|n| n.board.clone()) {
            Some(BoardItem::Shape { shape }) => shapes.push((*id, shape, text(*id))),
            Some(BoardItem::Connector(c)) => connectors.push((*id, c)),
            other => panic!("{id} is neither a shape nor a connector: {other:?}"),
        }
    }
    let drawn: Vec<(BoardShape, Option<String>)> = shapes
        .iter()
        .map(|(_, shape, text)| (*shape, text.clone()))
        .collect();
    assert_eq!(
        drawn,
        [
            (BoardShape::RoundedRectangle, Some("Start".into())),
            (BoardShape::Diamond, Some("Ok?".into())),
            (BoardShape::RoundedRectangle, Some("Done".into())),
        ]
    );
    // Both connectors are attached at both ends: Start to Ok?, Ok? to Done.
    let node = |at: usize| shapes[at].0;
    let ends: Vec<(Option<NodeId>, Option<NodeId>)> = connectors
        .iter()
        .map(|(_, c)| (c.from.node(), c.to.node()))
        .collect();
    assert_eq!(
        ends,
        [
            (Some(node(0)), Some(node(1))),
            (Some(node(1)), Some(node(2)))
        ]
    );
    // The second one carries the label "yes".
    let label_of = |at: usize| figcraft_engine::board::label_of(&doc, connectors[at].0);
    assert_eq!(label_of(0), None);
    assert_eq!(label_of(1).and_then(text).as_deref(), Some("yes"));
}

#[test]
fn the_op_is_one_undo_step() {
    let mut board = board();
    draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
    assert_eq!(board.session.undo_depth(), (1, 0));
    board.execute("edit.undo", &json!({})).unwrap();
    let doc = board.doc().unwrap();
    let page = doc.first_page().unwrap();
    assert!(doc.node(page).unwrap().children.is_empty());
}

#[test]
fn double_quotes_that_trip_up_a_diagram_are_tried_again_as_single_ones() {
    let source = "flowchart LR\n  A[\"a\"] --> B\n  style A fill:\"#f9f\"\n";
    assert!(render(source, &Palette::default()).is_err());
    let mut board = board();
    let ids = draw(&mut board, json!({"op": "mermaid", "source": source}));
    // Two nodes and the arrow between them.
    assert_eq!(ids.len(), 3);
}

#[test]
fn the_op_needs_a_source() {
    let mut board = board();
    for op in [
        json!({"op": "mermaid"}),
        json!({"op": "mermaid", "source": " \n"}),
        json!({"op": "mermaid", "source": 5}),
    ] {
        let reply = ops::run(&mut board, &op);
        assert_eq!(
            reply,
            json!({"ok": false, "error": "mermaid: `source` must be a non-empty string"}),
            "{op}"
        );
    }
}

#[test]
fn a_diagram_without_shapes_is_reported() {
    let mut board = board();
    let reply = ops::run(
        &mut board,
        &json!({"op": "mermaid", "source": "flowchart LR"}),
    );
    assert_eq!(
        reply,
        json!({"ok": false, "error": "mermaid: the diagram is empty"})
    );
}
