//! A call as a whole: unknown types, ids, failure and rollback, the selection, the JSON, bounds.

use std::sync::Arc;

use figcraft_doc::NodeKind;

use super::*;

#[test]
fn unknown_types_are_skipped_with_notes() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "laser", "x": 0, "y": 0},
            {"type": "RECTANGLE", "x": 0, "y": 0, "width": 10, "height": 10},
            {"x": 1, "y": 2},
            {"type": "embeddable", "x": 0, "y": 0},
        ]),
    );
    assert_eq!(done.ids.len(), 1, "types match in any case");
    assert_eq!(shape(&b, node(&done, 0)), BoardShape::Rectangle);
    assert_eq!(done.notes.len(), 3, "{:?}", done.notes);
    assert!(done.notes[0].contains("elements[0]") && done.notes[0].contains("laser"));
    assert!(done.notes[1].contains("elements[2]") && done.notes[1].contains("no type"));
    assert!(done.notes[2].contains("elements[3]") && done.notes[2].contains("embeddable"));
}

#[test]
fn ids_come_back_in_input_order_whatever_the_order_they_are_made_in() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "arrow", "x": 0, "y": 0, "start": {"id": "a"}, "end": {"id": "b"}},
            {"type": "text", "x": 0, "y": -40, "text": "Title"},
            {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 100, "height": 50},
            {"type": "sticky", "id": "b", "x": 300, "y": 0, "text": "hi"},
            {"type": "frame", "x": -50, "y": -80, "width": 500, "height": 300},
        ]),
    );
    assert_eq!(done.ids.len(), 5);
    let doc = b.doc().unwrap();
    let kind = |i| doc.node(node(&done, i)).unwrap();
    let arrow = connector(&b, node(&done, 0));
    assert_eq!(arrow.from.node(), Some(node(&done, 2)));
    assert_eq!(arrow.to.node(), Some(node(&done, 3)));
    assert!(matches!(&kind(1).kind, NodeKind::Text(t) if t.characters == "Title"));
    assert_eq!(shape(&b, node(&done, 2)), BoardShape::Rectangle);
    assert!(matches!(kind(3).board, Some(BoardItem::Sticky { .. })));
    assert!(matches!(kind(4).kind, NodeKind::Section { .. }));
}

#[test]
fn ids_of_one_call_shadow_those_already_on_the_board() {
    let mut b = board();
    add(
        &mut b,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 10, "height": 10},
            {"type": "rectangle", "id": "old", "x": 0, "y": 500, "width": 10, "height": 10},
        ]),
    );
    let before = resolve(&b.doc().unwrap(), "a").unwrap();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 100, "width": 10, "height": 10},
            {"type": "rectangle", "id": "b", "x": 0, "y": 200, "width": 10, "height": 10},
            {"type": "arrow", "x": 0, "y": 0, "start": {"id": "a"}, "end": {"id": "b"}},
            {"type": "arrow", "x": 0, "y": 0, "start": {"id": "old"}, "end": {"id": "b"}},
        ]),
    );
    let (a, old) = (node(&done, 0), resolve(&b.doc().unwrap(), "old").unwrap());
    assert_ne!(a, before);
    assert_eq!(
        connector(&b, node(&done, 2)).from.node(),
        Some(a),
        "this call's `a`"
    );
    assert_eq!(
        connector(&b, node(&done, 3)).from.node(),
        Some(old),
        "an earlier call's `old`"
    );
    assert_eq!(
        resolve(&b.doc().unwrap(), "a"),
        Some(a),
        "the newest element holds an id"
    );
}

#[test]
fn the_agent_id_is_kept_in_plugin_data() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "text", "id": "t", "x": 0, "y": 0, "text": "hi"},
            {"type": "text", "id": "", "x": 0, "y": 40, "text": "no id"},
        ]),
    );
    let doc = b.doc().unwrap();
    let data = &doc.node(node(&done, 0)).unwrap().plugin_data;
    assert_eq!(data.get(SAWIT_ID).map(String::as_str), Some("t"));
    assert!(
        doc.node(node(&done, 1)).unwrap().plugin_data.is_empty(),
        "an empty id is none"
    );
}

#[test]
fn an_engine_error_fails_the_whole_call_and_changes_nothing() {
    let mut b = board();
    let items = skeletons(json!([
        {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50},
        {"type": "sticky", "x": 200, "y": 0, "text": "fine"},
        {"type": "rectangle", "x": 0, "y": 100, "width": -5, "height": 50},
    ]));
    let before = b.doc().unwrap();
    let err = apply(&mut b, &items, (0.0, 0.0)).unwrap_err();
    assert!(
        matches!(&err, BoardError::Engine(m) if m.contains("elements[2]") && m.contains("rectangle")),
        "{err}"
    );
    assert!(layers(&b).is_empty(), "the call is rolled back");
    assert!(
        Arc::ptr_eq(&before, &b.doc().unwrap()),
        "to the very document it started from"
    );
    let items = skeletons(
        json!([{"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
                                      "backgroundColor": "banana"}]),
    );
    assert!(
        apply(&mut b, &items, (0.0, 0.0)).is_err(),
        "a colour the engine doesn't know"
    );
    assert!(layers(&b).is_empty());
    assert!(Arc::ptr_eq(&before, &b.doc().unwrap()));
}

#[test]
fn applying_nothing_changes_nothing() {
    let mut b = board();
    let revision = b.revision();
    let done = apply(&mut b, &[], (10.0, 10.0)).unwrap();
    assert_eq!(done, Applied::default());
    assert_eq!(b.revision(), revision);
    b.execute("edit.undo", &json!({})).unwrap();
}

#[test]
fn the_selection_is_left_as_it_was() {
    let mut b = board();
    let first = add(
        &mut b,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
    );
    let mine = node(&first, 0);
    b.session.set_selection(vec![mine]);
    add(
        &mut b,
        json!([
            {"type": "rectangle", "x": 300, "y": 0, "width": 50, "height": 50},
            {"type": "text", "x": 300, "y": 100, "text": "new"},
            {"type": "arrow", "x": 0, "y": 200, "width": 50},
        ]),
    );
    assert_eq!(b.session.selection(), vec![mine]);
    b.session.set_selection(Vec::new());
    add(
        &mut b,
        json!([{"type": "text", "x": 0, "y": 0, "text": "more"}]),
    );
    assert!(b.session.selection().is_empty());
}

#[test]
fn a_design_file_refuses_board_elements_and_stays_unchanged() {
    let design = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
    let mut b = BoardDoc::from_bytes(&design).unwrap();
    let items =
        skeletons(json!([{"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10}]));
    assert!(matches!(
        apply(&mut b, &items, (0.0, 0.0)),
        Err(BoardError::Engine(_))
    ));
    assert!(layers(&b).is_empty());
}

#[test]
fn skeleton_reads_the_mcp_json() {
    let s: Skeleton = serde_json::from_value(json!({
        "type": "arrow", "id": "e", "x": 160, "y": 30, "width": 200, "height": 10,
        "start": {"id": "a"}, "end": {"id": "b", "x": 1.5, "y": 2.5},
        "points": [[0, 0], [200, 10]],
        "strokeColor": "#e03131", "backgroundColor": "#ffc9c9", "roundness": {"type": 3},
        "frameId": "f", "label": {"text": "go", "fontSize": 16},
        "fontSize": 20, "roughness": 1, "someFutureField": [1, 2, 3]
    }))
    .unwrap();
    assert_eq!(s.kind, "arrow");
    assert_eq!((s.id.as_deref(), s.x, s.y), (Some("e"), 160.0, 30.0));
    assert_eq!((s.width, s.height), (Some(200.0), Some(10.0)));
    assert_eq!(
        s.start,
        Some(End {
            id: Some("a".into()),
            x: None,
            y: None
        })
    );
    assert_eq!(
        s.end,
        Some(End {
            id: Some("b".into()),
            x: Some(1.5),
            y: Some(2.5)
        })
    );
    assert_eq!(s.points, Some(vec![[0.0, 0.0], [200.0, 10.0]]));
    assert_eq!(s.stroke_color.as_deref(), Some("#e03131"));
    assert_eq!(s.background_color.as_deref(), Some("#ffc9c9"));
    assert_eq!(s.roundness, Some(json!({"type": 3})));
    assert_eq!(s.frame_id.as_deref(), Some("f"));
    assert_eq!(
        s.label,
        Some(Label {
            text: "go".into(),
            font_size: Some(16.0)
        })
    );
    // Excalidraw's `fontSize`, of a text and of a label, is read too.
    assert_eq!(s.font_size, Some(20.0));
    // Anything missing is its default, and `null` is missing.
    let bare: Skeleton = serde_json::from_value(json!({"roundness": null, "label": {}})).unwrap();
    assert_eq!(
        bare,
        Skeleton {
            label: Some(Label::default()),
            ..Skeleton::default()
        }
    );
    // It writes the same names back.
    let back = serde_json::to_value(&s).unwrap();
    assert_eq!(back["type"], "arrow");
    assert_eq!(back["strokeColor"], "#e03131");
    assert_eq!(back["frameId"], "f");
    assert_eq!(serde_json::from_value::<Skeleton>(back).unwrap(), s);
}

#[test]
fn bounds_cover_every_element_that_will_be_drawn() {
    assert_eq!(bounds(&[]), None);
    let items = skeletons(json!([
        {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
        {"type": "ellipse", "x": -30, "y": 40},
        {"type": "sticky", "x": 500, "y": 0, "text": "s"},
        {"type": "text", "x": 5, "y": -60, "text": "t"},
        {"type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [100, 400]]},
        {"type": "arrow", "x": 0, "y": 0, "end": {"x": -200, "y": 10}},
        {"type": "frame", "x": 0, "y": 0},
        {"type": "laser", "x": 9999, "y": 9999},
    ]));
    // Left: the free end at -200. Top: the text at -60. Right: the sticky's 200. Bottom: the arrow's 400.
    assert_eq!(bounds(&items), Some((-200.0, -60.0, 700.0, 400.0)));
    assert_eq!(
        bounds(&skeletons(json!([{"type": "laser", "x": 1, "y": 1}]))),
        None
    );
    assert_eq!(
        bounds(&skeletons(
            json!([{"type": "text", "x": 3, "y": 4, "text": "t"}])
        )),
        Some((3.0, 4.0, 3.0, 4.0))
    );
}

#[test]
fn a_hundred_elements_are_one_quick_call() {
    let mut b = board();
    let mut items =
        vec![json!({"type": "rectangle", "id": "n0", "x": 0, "y": 0, "width": 50, "height": 30})];
    for i in 1..100 {
        items.push(
            json!({"type": "rectangle", "id": format!("n{i}"), "x": f64::from(i) * 60.0, "y": 0,
                              "width": 50, "height": 30, "label": {"text": format!("n{i}")}}),
        );
        items.push(json!({"type": "arrow", "x": 0, "y": 0,
                              "start": {"id": format!("n{}", i - 1)}, "end": {"id": format!("n{i}")}}));
    }
    let done = add(&mut b, Value::Array(items));
    assert_eq!(done.ids.len(), 199);
    assert!(done.notes.is_empty(), "{:?}", done.notes);
}
