//! An import as a whole: what isn't drawn and the notes on it, ids, undo, and the board it makes.

use super::*;
use crate::excalidraw::draw::{draw_apart, named};

#[test]
fn what_is_not_an_element_is_noted_by_kind() {
    let imported = read(json!([
        1, "two", null,
        {"id": "x"},
        {"id": "e1", "type": "embeddable"},
        {"id": "e2", "type": "iframe"},
        {"id": "e3", "type": "magicframe"},
        {"id": "l1", "type": "laser"},
        {"id": "l2", "type": "laser"},
        {"id": "s", "type": "selection"},
        {"id": "long", "type": "a-type-name-that-is-much-too-long-to-show-in-a-note-in-full"},
    ]));
    assert!(imported.skeletons.is_empty());
    assert_eq!(
        imported.notes,
        [
            "skipped 3 entries of `elements` that are not objects",
            "skipped 1 element without a type",
            "skipped 2 elements of the unknown type `laser`",
            "skipped 1 element of the unknown type `selection`",
            "skipped 1 element of the unknown type `a-type-name-that-is-much-too-long-to-sho…`",
            "skipped 3 embeds",
        ]
    );
    let one = read(json!([1]));
    assert_eq!(
        one.notes,
        ["skipped 1 entry of `elements` that is not an object"]
    );
}

#[test]
fn the_ids_of_a_scene_are_agent_ids_on_the_board() {
    let mut board = board();
    import(&mut board, &basic()).unwrap();
    let doc = board.doc().unwrap();
    let ink = skeleton::resolve(&doc, "scribble").unwrap();
    assert!(matches!(doc.node(ink).unwrap().board, Some(BoardItem::Ink)));
    let link = skeleton::resolve(&doc, "link").unwrap();
    assert!(matches!(
        doc.node(link).unwrap().board,
        Some(BoardItem::Connector(_))
    ));
    // And an agent can change an element by the id it had in Excalidraw.
    let reply = ops::run(
        &mut board,
        &json!({"op": "update", "elements": [{"id": "box", "text": "Renamed"}]}),
    );
    assert_eq!(reply["updated"], json!(1), "{reply}");
}

#[test]
fn a_board_of_a_scene_opens_as_a_file_does() {
    let (board, notes) = to_board("Sketch", &basic()).unwrap();
    assert_eq!(notes, ["skipped 1 embed"]);
    assert_eq!(board.kind(), DocKind::Board);
    assert_eq!(board.doc().unwrap().name, "Sketch");
    // Nothing to undo: the scene is what was opened, not something done to the board.
    assert_eq!(board.session.undo_depth(), (0, 0));
    assert!(board.session.selection().is_empty());
    let mut board = board;
    assert_eq!(elements(&mut board).len(), 9);
    // What a save writes of it opens as the same board.
    let again = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
    assert_eq!(
        again.doc().unwrap().nodes().count(),
        board.doc().unwrap().nodes().count()
    );

    // A scene that is none is the error of `import`.
    let error = to_board("Sketch", &json!({})).err().unwrap();
    assert!(matches!(error, BoardError::Format(_)), "{error:?}");
}

#[test]
fn an_import_is_one_undo_step_and_leaves_the_selection_alone() {
    let mut board = board();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0}))
        .unwrap();
    let (selection, steps) = (board.session.selection(), board.session.undo_depth());
    import(&mut board, &basic()).unwrap();
    assert_eq!(board.session.selection(), selection);
    assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
    assert_eq!(board.session.undo_labels(1), ["Add elements"]);
}

#[test]
fn an_element_the_engine_refuses_is_left_out_with_a_note() {
    // Skeletons made by hand, one of which the engine refuses: its colour is nonsense, which
    // `to_skeletons` would never give. The list as a whole can't be drawn.
    let rectangle = |id: &str, colour: Option<&str>| Skeleton {
        kind: "rectangle".into(),
        id: Some(id.into()),
        width: Some(40.0),
        height: Some(40.0),
        stroke_color: colour.map(str::to_string),
        ..Skeleton::default()
    };
    let arrow = Skeleton {
        kind: "arrow".into(),
        id: Some("link".into()),
        start: Some(End {
            id: Some("good".into()),
            ..End::default()
        }),
        end: Some(End {
            id: Some("also".into()),
            ..End::default()
        }),
        ..Skeleton::default()
    };
    let items = [
        // The arrow comes first on purpose: it still finds what it joins.
        arrow,
        rectangle("good", None),
        rectangle("bad", Some("nonsense")),
        Skeleton {
            x: 200.0,
            ..rectangle("also", None)
        },
    ];
    let mut all_at_once = board();
    assert!(skeleton::apply(&mut all_at_once, &items, (0.0, 0.0)).is_err());

    let mut board = board();
    let mut notes = Vec::new();
    let refused = BoardError::Engine("the list".into());
    draw_apart(&mut board, &items, refused, &mut notes).unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("skipped rectangle `bad`: "),
        "{notes:?}"
    );
    assert!(!notes[0].contains("elements["), "{notes:?}");
    // The others are drawn, and the arrow joins the two rectangles.
    let elements = elements(&mut board);
    assert_eq!(of_type(&elements, "rectangle").len(), 2, "{elements:?}");
    let link = of_type(&elements, "connector");
    assert_eq!(link.len(), 1);
    assert!(link[0].get("from").is_some() && link[0].get("to").is_some());
}

#[test]
fn when_nothing_can_be_drawn_it_is_the_lists_error() {
    // A design file has no board objects: that is the error, not a note for each element, and
    // not the sections and pictures that it would take.
    let bytes = figcraft_format::save_bytes(&figcraft_doc::Document::new("Design"), None).unwrap();
    let mut design = BoardDoc::from_bytes(&bytes).unwrap();
    let before = design.doc().unwrap();
    let error = import(&mut design, &basic()).unwrap_err();
    assert!(matches!(error, BoardError::Engine(_)), "{error:?}");
    assert!(error.to_string().contains("elements["), "{error}");
    assert!(std::sync::Arc::ptr_eq(&before, &design.doc().unwrap()));
    // An empty scene draws nothing, and that is no error.
    assert_eq!(
        import(&mut design, &scene(json!([]))).unwrap(),
        Vec::<String>::new()
    );

    // On a board, when the engine refuses every element, that is the list's error as well.
    let nonsense = |id: &str| Skeleton {
        kind: "rectangle".into(),
        id: Some(id.into()),
        stroke_color: Some("nonsense".into()),
        ..Skeleton::default()
    };
    let mut board = board();
    let before = board.doc().unwrap();
    let mut notes = vec!["kept".to_string()];
    let refused = BoardError::Engine("the list".into());
    let error = draw_apart(
        &mut board,
        &[nonsense("a"), nonsense("b")],
        refused,
        &mut notes,
    )
    .unwrap_err();
    assert_eq!(error, BoardError::Engine("the list".into()));
    assert_eq!(notes, ["kept"]);
    assert!(std::sync::Arc::ptr_eq(&before, &board.doc().unwrap()));
}

#[test]
fn notes_name_the_element_they_are_about() {
    // An arrow that starts and ends in one place can't be drawn, which `apply` says of
    // "elements[2]": here that is the arrow's name.
    let mut board = board();
    let notes = import(
        &mut board,
        &scene(json!([
            rectangle("a", 0.0, 0.0),
            rectangle("b", 300.0, 0.0),
            {"id": "dot", "type": "arrow", "x": 50, "y": 60, "points": [[0, 0], [0, 0]]},
        ])),
    )
    .unwrap();
    assert_eq!(
        notes,
        ["skipped arrow `dot`: it starts and ends at the same point"]
    );
    assert_eq!(elements(&mut board).len(), 2);

    // Only what looks like an index is replaced.
    let items = [Skeleton {
        kind: "text".into(),
        ..Skeleton::default()
    }];
    assert_eq!(named("elements[0]: x", &items), "text: x");
    assert_eq!(
        named("elements[9]: elements[x] elements[", &items),
        "elements[9]: elements[x] elements["
    );
}

#[test]
fn a_connector_the_engine_cannot_place_does_not_fail_the_import() {
    // Two shapes drawn on top of each other and an arrow between them: the engine can't
    // place its ends apart. The shapes are worth keeping, so that is a note, not an error.
    let bound = |id: &str| json!({"elementId": id, "focus": 0, "gap": 1});
    let mut board = board();
    let notes = import(
        &mut board,
        &scene(json!([
            rectangle("a", 0.0, 0.0),
            rectangle("b", 0.0, 0.0),
            {"id": "ab", "type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [10, 0]],
             "startBinding": bound("a"), "endBinding": bound("b")},
            {"id": "after", "type": "text", "x": 0, "y": 200, "text": "still here"},
        ])),
    )
    .unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("skipped arrow `ab`: "), "{notes:?}");
    assert!(
        notes[0].ends_with("the ends are in the same place"),
        "{notes:?}"
    );
    let elements = elements(&mut board);
    assert_eq!(of_type(&elements, "rectangle").len(), 2, "{elements:?}");
    assert_eq!(of_type(&elements, "text").len(), 1);
    assert_eq!(of_type(&elements, "connector").len(), 0);
}
