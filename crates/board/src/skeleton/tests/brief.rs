//! The tests the brief asks for: shapes and labels, arrows, ids, sections, undo and ink.

use figcraft_doc::{End as DocEnd, NodeKind, StrokeCap};

use super::*;

/// Excalidraw's `fontSize` sets a text's size and a shape's label's; without one, each is the
/// engine's.
#[test]
fn a_font_size_sets_the_text_and_the_label() {
    let mut b = board();
    add(
        &mut b,
        json!([
            {"type": "text", "x": 0, "y": 0, "text": "big", "fontSize": 28},
            {"type": "rectangle", "x": 0, "y": 100, "width": 120, "height": 40,
             "label": {"text": "small", "fontSize": 10}},
            {"type": "text", "x": 0, "y": 200, "text": "plain"},
            {"type": "rectangle", "x": 0, "y": 300, "width": 120, "height": 40,
             "label": {"text": "plain"}},
        ]),
    );
    let made = layers(&b);
    let doc = b.doc().unwrap();
    let size = |id: &NodeId| {
        let shape = figcraft_engine::board::text_of(&doc, *id);
        let text = doc.node(shape.unwrap_or(*id)).unwrap().kind.text().unwrap();
        (
            shape.is_some(),
            text.characters.clone(),
            text.style.font_size,
        )
    };
    let mut sizes: Vec<(bool, String, f64)> = made.iter().map(size).collect();
    sizes.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let fresh = figcraft_doc::TextData::default().style.font_size;
    let expected = [
        (false, "big", 28.0),
        (false, "plain", fresh),
        (true, "plain", 16.0),
        (true, "small", 10.0),
    ];
    let expected: Vec<(bool, String, f64)> = expected
        .iter()
        .map(|(shape, text, size)| (*shape, text.to_string(), *size))
        .collect();
    assert_eq!(sizes, expected);
}

#[test]
fn rectangle_with_label_becomes_board_shape() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([{"type": "rectangle", "id": "a", "x": 10, "y": 20, "width": 160, "height": 60,
                    "label": {"text": "API"}}]),
    );
    let made = layers(&b);
    assert_eq!(made.len(), 1, "one layer: the label is inside it");
    assert_eq!(done.ids, vec![made[0].to_string()]);
    assert!(done.notes.is_empty(), "{:?}", done.notes);
    assert_eq!(shape(&b, made[0]), BoardShape::Rectangle);
    assert_eq!(text_in(&b, made[0]), "API");
    assert_eq!(bounds_of(&b, made[0]), [10.0, 20.0, 170.0, 80.0]);
}

#[test]
fn roundness_makes_rounded_rectangle() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50, "roundness": {"type": 3}},
            {"type": "rectangle", "x": 0, "y": 100, "width": 100, "height": 50, "roundness": null},
            {"type": "rectangle", "x": 0, "y": 200, "width": 100, "height": 50},
            {"type": "rectangle", "x": 0, "y": 300, "width": 100, "height": 50, "roundness": false},
            {"type": "ellipse", "x": 0, "y": 400, "width": 100, "height": 50, "roundness": {"type": 2}},
            {"type": "diamond", "x": 0, "y": 500, "width": 100, "height": 50},
        ]),
    );
    let got: Vec<BoardShape> = (0..6).map(|i| shape(&b, node(&done, i))).collect();
    assert_eq!(
        got,
        [
            BoardShape::RoundedRectangle,
            BoardShape::Rectangle,
            BoardShape::Rectangle,
            BoardShape::Rectangle,
            BoardShape::Ellipse,
            BoardShape::Diamond,
        ]
    );
}

#[test]
fn arrow_binds_to_shapes_by_agent_id() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 160, "height": 60},
            {"type": "rectangle", "id": "b", "x": 400, "y": 0, "width": 160, "height": 60},
            {"type": "arrow", "x": 160, "y": 30, "start": {"id": "a"}, "end": {"id": "b"},
             "label": {"text": "calls"}},
        ]),
    );
    assert_eq!(
        done.ids.len(),
        3,
        "the arrow's label isn't an element of its own"
    );
    assert!(done.notes.is_empty(), "{:?}", done.notes);
    let doc = b.doc().unwrap();
    let arrow = connector(&b, node(&done, 2));
    assert_eq!(arrow.from.node(), resolve(&doc, "a"));
    assert_eq!(arrow.to.node(), resolve(&doc, "b"));
    assert_eq!(arrow.from.node(), Some(node(&done, 0)));
    assert_eq!(arrow.to.node(), Some(node(&done, 1)));
    let line = doc.node(node(&done, 2)).unwrap();
    assert_eq!(line.stroke.cap_end, StrokeCap::TriangleArrow);
    assert_eq!(line.stroke.cap_start, StrokeCap::None);
    assert!(figcraft_engine::board::label_of(&doc, node(&done, 2)).is_some());
}

#[test]
fn arrow_to_missing_id_is_free() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 160, "height": 60},
            {"type": "arrow", "x": 160, "y": 30, "start": {"id": "a"},
             "end": {"id": "zzz", "x": 400, "y": 300}},
            {"type": "arrow", "x": 10, "y": 20, "points": [[0, 0], [70, 80]],
             "start": {"id": "nope"}, "end": {"id": "zzz"}},
        ]),
    );
    assert!(done.notes.is_empty(), "{:?}", done.notes);
    let first = connector(&b, node(&done, 1));
    assert_eq!(first.from.node(), Some(node(&done, 0)));
    assert_eq!(first.to, DocEnd::Point { x: 400.0, y: 300.0 });
    // Neither id exists and neither end has coordinates: both ends are where the arrow is drawn.
    let second = connector(&b, node(&done, 2));
    assert_eq!(second.from, DocEnd::Point { x: 10.0, y: 20.0 });
    assert_eq!(second.to, DocEnd::Point { x: 80.0, y: 100.0 });
}

#[test]
fn agent_ids_win_over_node_ids() {
    let mut b = board();
    let doc = b.doc().unwrap();
    let page = doc.first_page().unwrap();
    assert_eq!(
        page,
        NodeId(1),
        "a new board's page is node 1, so \"1\" is ambiguous"
    );
    let done = add(
        &mut b,
        json!([{"type": "rectangle", "id": "1", "x": 0, "y": 0, "width": 10, "height": 10}]),
    );
    let made = node(&done, 0);
    assert_ne!(made, page);
    let doc = b.doc().unwrap();
    assert_eq!(
        resolve(&doc, "1"),
        Some(made),
        "the agent's element, not the page"
    );
    // An id no agent gave is a node id, in either spelling.
    assert_eq!(resolve(&doc, &made.to_string()), Some(made));
    assert_eq!(resolve(&doc, &format!("0:{made}")), Some(made));
    assert_eq!(resolve(&doc, "99999"), None);
    assert_eq!(resolve(&doc, "nope"), None);
}

#[test]
fn frame_children_move_inside() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "frame", "id": "f", "x": 0, "y": 0, "width": 400, "height": 300, "name": "Group A"},
            {"type": "rectangle", "id": "r", "x": 50, "y": 60, "width": 100, "height": 50, "frameId": "f"},
        ]),
    );
    let (frame, child) = (node(&done, 0), node(&done, 1));
    let doc = b.doc().unwrap();
    assert!(matches!(
        doc.node(frame).unwrap().kind,
        NodeKind::Section { .. }
    ));
    assert_eq!(doc.node(frame).unwrap().name, "Group A");
    assert_eq!(doc.node(child).unwrap().parent, Some(frame));
    assert_eq!(layers(&b), vec![frame], "only the section is on the page");
    assert_eq!(
        bounds_of(&b, child),
        [50.0, 60.0, 150.0, 110.0],
        "it stays where it was"
    );
}

#[test]
fn frame_children_may_come_before_their_frame() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "text", "x": 20, "y": 30, "text": "inside", "frameId": "f"},
            {"type": "sticky", "x": 40, "y": 50, "text": "too", "frameId": "f"},
            {"type": "frame", "id": "f", "x": 0, "y": 0, "width": 400, "height": 300,
             "label": {"text": "Titled by its label"}},
        ]),
    );
    let doc = b.doc().unwrap();
    let frame = node(&done, 2);
    assert_eq!(doc.node(frame).unwrap().name, "Titled by its label");
    assert_eq!(doc.node(node(&done, 0)).unwrap().parent, Some(frame));
    assert_eq!(doc.node(node(&done, 1)).unwrap().parent, Some(frame));
    assert_eq!(layers(&b), vec![frame]);
}

#[test]
fn frames_are_made_first_so_they_never_cover_what_is_drawn_on_them() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "x": 10, "y": 10, "width": 50, "height": 50},
            {"type": "frame", "x": 0, "y": 0, "width": 400, "height": 300},
        ]),
    );
    assert_eq!(
        layers(&b),
        vec![node(&done, 1), node(&done, 0)],
        "the frame is the lower layer"
    );
}

#[test]
fn a_frame_id_that_cannot_hold_the_element_is_ignored() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "id": "r", "x": 0, "y": 0, "width": 50, "height": 50, "frameId": "nope"},
            {"type": "rectangle", "id": "s", "x": 0, "y": 100, "width": 50, "height": 50, "frameId": "s"},
            {"type": "rectangle", "id": "t", "x": 0, "y": 200, "width": 50, "height": 50, "frameId": "r"},
            {"type": "frame", "id": "f1", "x": 300, "y": 0, "width": 50, "height": 50, "frameId": "f2"},
            {"type": "frame", "id": "f2", "x": 300, "y": 100, "width": 50, "height": 50, "frameId": "f1"},
            {"type": "arrow", "x": 0, "y": 400, "width": 50, "frameId": "f1"},
        ]),
    );
    let doc = b.doc().unwrap();
    let page = doc.first_page().unwrap();
    for i in [0, 1, 2, 5] {
        assert_eq!(
            doc.node(node(&done, i)).unwrap().parent,
            Some(page),
            "element {i}"
        );
    }
    // Two frames naming each other: the first goes into the second, and only that.
    assert_eq!(
        doc.node(node(&done, 3)).unwrap().parent,
        Some(node(&done, 4))
    );
    assert_eq!(doc.node(node(&done, 4)).unwrap().parent, Some(page));
}

#[test]
fn add_is_one_undo_step() {
    let mut b = board();
    add(
        &mut b,
        json!([
            {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50},
            {"type": "sticky", "x": 200, "y": 0, "text": "hi"},
            {"type": "text", "x": 0, "y": -40, "text": "Title"},
        ]),
    );
    assert_eq!(layers(&b).len(), 3);
    b.execute("edit.undo", &json!({})).unwrap();
    assert!(layers(&b).is_empty(), "one undo takes all three away");
    b.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(layers(&b).len(), 3, "and one redo brings them back");
}

#[test]
fn line_with_many_points_is_ink() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "line", "x": 0, "y": 0, "points": [[0, 0], [50, 0], [50, 50], [100, 50]]},
            {"type": "freedraw", "x": 200, "y": 0, "points": [[0, 0], [10, 10]]},
            {"type": "line", "x": 0, "y": 200, "points": [[0, 0], [100, 50]]},
            {"type": "line", "x": 0, "y": 300, "width": 80},
        ]),
    );
    assert_eq!(board_item(&b, node(&done, 0)), BoardItem::Ink);
    assert_eq!(
        board_item(&b, node(&done, 1)),
        BoardItem::Ink,
        "a freedraw is ink however short"
    );
    for i in [2, 3] {
        let doc = b.doc().unwrap();
        let line = doc.node(node(&done, i)).unwrap();
        assert!(
            matches!(line.board, Some(BoardItem::Connector(_))),
            "line {i}"
        );
        assert_eq!(line.stroke.cap_start, StrokeCap::None);
        assert_eq!(
            line.stroke.cap_end,
            StrokeCap::None,
            "a line has no arrowhead"
        );
    }
}
