//! The tests the brief names: a scene with everything supported, a bad scene, unknown types.

use figcraft_doc::NodeKind;

use super::*;

#[test]
fn basic_scene_imports_everything_supported() {
    let mut board = board();
    let notes = import(&mut board, &basic()).unwrap();
    assert_eq!(notes, ["skipped 1 embed"]);

    let elements = elements(&mut board);
    // One of each kind the fixture holds, and nothing else: no layer for the label, the
    // embed or the deleted rectangle, and the frame's child is inside it.
    assert_eq!(elements.len(), 9, "{elements:?}");
    assert_eq!(of_type(&elements, "rectangle").len(), 1);
    assert_eq!(of_type(&elements, "ellipse").len(), 1);
    assert_eq!(of_type(&elements, "diamond").len(), 1);
    assert_eq!(of_type(&elements, "connector").len(), 2);
    assert_eq!(of_type(&elements, "ink").len(), 1);
    assert_eq!(of_type(&elements, "text").len(), 1);
    assert_eq!(of_type(&elements, "section").len(), 1);
    assert_eq!(of_type(&elements, "image").len(), 1);

    // The rectangle carries its label.
    let rectangle = of_type(&elements, "rectangle")[0];
    assert_eq!(rectangle["text"], json!("Service"));
    let ellipse = of_type(&elements, "ellipse")[0];

    // One connector is bound at both ends, to the rectangle and the ellipse; the other is a
    // free line, with no caps.
    let doc = board.doc().unwrap();
    let (bound, free): (Vec<&Value>, Vec<&Value>) = of_type(&elements, "connector")
        .into_iter()
        .partition(|c| c.get("from").is_some());
    assert_eq!((bound.len(), free.len()), (1, 1));
    assert_eq!(bound[0]["from"], rectangle["id"]);
    assert_eq!(bound[0]["to"], ellipse["id"]);
    assert!(free[0].get("to").is_none());
    let line = doc
        .node(free[0]["id"].as_str().unwrap().parse().unwrap())
        .unwrap();
    assert_eq!(line.stroke.cap_start, StrokeCap::None);
    assert_eq!(line.stroke.cap_end, StrokeCap::None);
    assert!(matches!(line.board, Some(BoardItem::Connector(_))));

    // The section is named for the frame, and holds the rectangle that was in it.
    let section = of_type(&elements, "section")[0];
    assert_eq!(section["name"], json!("Group A"));
    let section = doc
        .node(section["id"].as_str().unwrap().parse().unwrap())
        .unwrap();
    assert!(matches!(section.kind, NodeKind::Section { .. }));
    assert_eq!(section.children.len(), 1);
    let child = doc.node(section.children[0]).unwrap();
    assert!(matches!(child.board, Some(BoardItem::Shape { .. })));
}

#[test]
fn bad_scene_is_reported() {
    for scene in [
        json!({}),
        json!([]),
        json!(null),
        json!("scene"),
        json!({"elements": {}}),
        json!({"elements": "none"}),
        json!({"elements": null}),
    ] {
        let error = to_skeletons(&scene).err().unwrap_or_default();
        assert!(error.contains("`elements`"), "{scene}: {error:?}");
        // Importing it is an error too, and draws nothing.
        let mut board = board();
        let before = board.doc().unwrap();
        assert!(matches!(
            import(&mut board, &scene),
            Err(BoardError::Format(_))
        ));
        assert!(std::sync::Arc::ptr_eq(&before, &board.doc().unwrap()));
    }
}

#[test]
fn unknown_types_are_noted() {
    let scene = json!({"elements": [
        {"id": "a", "type": "laser", "x": 0, "y": 0},
        {"id": "b", "type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
    ]});
    let imported = to_skeletons(&scene).unwrap();
    assert_eq!(imported.skeletons.len(), 1);
    assert_eq!(imported.skeletons[0].kind, "rectangle");
    assert_eq!(imported.notes.len(), 1, "{:?}", imported.notes);
    assert!(imported.notes[0].contains("laser"), "{:?}", imported.notes);

    // It is a note when imported, never an error, and the rest is drawn.
    let mut board = board();
    let notes = import(&mut board, &scene).unwrap();
    assert_eq!(notes, imported.notes);
    assert_eq!(elements(&mut board).len(), 1);
}
