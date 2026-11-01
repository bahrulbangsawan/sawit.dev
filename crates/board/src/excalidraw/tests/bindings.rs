//! Bindings a connector can't keep as written, and ids that aren't the scene's.

use super::*;

// Bindings the old canvas wrote that a connector can't keep. Its Mermaid diagrams bound a
// sequence message to the note Pele put just under it, and a loop back to one node to that
// node at both ends. Excalidraw drew both as they were written; joined, the engine would bend
// the message onto the note and draw the loop as a stub.

/// `arrow` with the `focus` of each of its bindings, as Excalidraw writes it.
fn focused(mut arrow: Value, focus: (f64, f64)) -> Value {
    for (key, focus) in [("startBinding", focus.0), ("endBinding", focus.1)] {
        if arrow[key].is_object() {
            arrow[key]["focus"] = json!(focus);
        }
    }
    arrow
}

#[test]
fn an_end_bound_to_an_element_it_runs_beside_stays_where_it_is_drawn() {
    // Excalidraw's `focus` says where an arrow, carried on past its end, meets the element it
    // is bound to: from -1 to 1 across it. A message bound to the note under it runs beside
    // the note: Excalidraw says so (focus 1.6), and the drawing agrees (carried on, the
    // message misses the note). Excalidraw drew it level, from lifeline to lifeline.
    let note = json!({"id": "note", "type": "rectangle", "x": 75, "y": 10, "width": 61,
                          "height": 33});
    let heads = (json!(null), json!("triangle"));
    let message = |id: &str, focus: f64| {
        let to_note = (None, Some("note"));
        let message = arrow(
            id,
            (0.0, 0.0),
            json!([[0, 0], [105, 0]]),
            heads.clone(),
            to_note,
        );
        focused(message, (0.0, focus))
    };
    let elements = json!([
        note,
        message("beside", 1.606),
        message("under", -1.2),
        message("edge", -1.0),
        message("into", 0.4)
    ]);
    let imported = read(elements.clone());
    assert_eq!(with_id(&imported, "beside").end, Some(loose(105.0, 0.0)));
    assert_eq!(with_id(&imported, "under").end, Some(loose(105.0, 0.0)));
    // A focus up to ±1, the edges included, says the arrow meets the element, and it joins it
    // even where it is drawn beside it: a route the old canvas left behind when a script moved
    // its shape is still joined to it.
    assert_eq!(with_id(&imported, "edge").end, Some(joined("note")));
    assert_eq!(with_id(&imported, "into").end, Some(joined("note")));

    // On the board the message runs level, with its head where it was.
    let board = drawn(elements);
    let (from, to, _, cap_end) = connector(&board, "beside");
    assert_eq!(from, DocEnd::Point { x: 0.0, y: 0.0 });
    assert_eq!(to, DocEnd::Point { x: 105.0, y: 0.0 });
    assert_eq!(cap_end, StrokeCap::TriangleArrow);
    let (_, to, _, _) = connector(&board, "into");
    assert_eq!(to.node(), node_of(&board, "note"));
}

#[test]
fn an_arrow_into_a_diamond_joins_it_whatever_its_focus_says() {
    // Excalidraw 0.18 measures where a line meets a diamond against the wrong half of it, so
    // an arrow straight down into the top of a wide diamond, off its centre, is written with a
    // focus past 1, as if it ran beside it: 1.138 at 70 off the centre of a 289 × 123 one
    // (the size of the diamonds on a real board), 1.626 at 100. Each meets the diamond, and
    // joins it. One that runs over the diamond's top, which Excalidraw also puts past 1,
    // misses it, and is free.
    let choice = json!({"id": "choice", "type": "diamond", "x": 0, "y": 0, "width": 289,
                            "height": 123});
    // A diamond turned by 45° about its centre (200, 300), and an arrow drawn into it as it is
    // turned: straight into its top, 120 off its centre (focus -1.955). The diamond upright,
    // or turned the other way, the same arrow would miss it.
    let turned = json!({"id": "turned", "type": "diamond", "x": 55.5, "y": 238.5,
                            "width": 289, "height": 123, "angle": std::f64::consts::FRAC_PI_4});
    // Each arrow: its id, the diamond its end is bound to, where it starts and ends on the
    // page, and the focus that Excalidraw 0.18 gives that end.
    let arrows = [
        ("off-70", "choice", [214.5, -100.0], [214.5, 25.0], 1.138),
        ("off-100", "choice", [244.5, -100.0], [244.5, 37.0], 1.626),
        ("over", "choice", [0.0, -8.0], [144.5, -8.0], 1.130),
        ("angled", "turned", [390.0, 280.0], [295.0, 375.0], -1.955),
    ];
    let heads = (json!(null), json!("arrow"));
    let mut diamonds = vec![choice, turned];
    for (id, diamond, start, end, focus) in arrows {
        let points = json!([[0, 0], [end[0] - start[0], end[1] - start[1]]]);
        let to_diamond = (None, Some(diamond));
        let into = arrow(id, (start[0], start[1]), points, heads.clone(), to_diamond);
        diamonds.push(focused(into, (0.0, focus)));
    }
    let diamonds = Value::Array(diamonds);
    let imported = read(diamonds.clone());
    assert_eq!(with_id(&imported, "off-70").end, Some(joined("choice")));
    assert_eq!(with_id(&imported, "off-100").end, Some(joined("choice")));
    assert_eq!(with_id(&imported, "angled").end, Some(joined("turned")));
    assert_eq!(with_id(&imported, "over").end, Some(loose(144.5, -8.0)));

    // On the board they end at the diamonds, and `get` says so: the diamond is their `to`.
    let mut board = drawn(diamonds);
    let choice = node_of(&board, "choice").unwrap();
    for id in ["off-70", "off-100"] {
        let (_, to, _, cap_end) = connector(&board, id);
        assert_eq!(to.node(), Some(choice), "{id}");
        assert_eq!(cap_end, StrokeCap::TriangleArrow, "{id}");
    }
    let (_, to, _, _) = connector(&board, "angled");
    assert_eq!(to.node(), node_of(&board, "turned"));
    let listed = elements(&mut board);
    let to_choice = of_type(&listed, "connector")
        .into_iter()
        .filter(|connector| connector["to"] == json!(choice.to_string()))
        .count();
    assert_eq!(to_choice, 2, "{listed:?}");
}

#[test]
fn an_arrow_bound_to_one_element_at_both_ends_joins_neither() {
    // A reply that runs beside the note under it, bound to that note at both ends. A connector
    // can't join one element at both ends, so the reply keeps the points it was drawn between
    // and runs level, as Excalidraw drew it. (Its focus here is 0: the ends alone free it.)
    let note = json!({"id": "note", "type": "rectangle", "x": -16, "y": 10, "width": 137,
                          "height": 33});
    let heads = (json!(null), json!("triangle"));
    let reply = arrow(
        "reply",
        (105.0, 0.0),
        json!([[0, 0], [-105, 0]]),
        heads,
        (Some("note"), Some("note")),
    );
    let imported = read(json!([note.clone(), reply.clone()]));
    let item = with_id(&imported, "reply");
    assert_eq!(item.kind, "arrow");
    assert_eq!(item.start, Some(loose(105.0, 0.0)));
    assert_eq!(item.end, Some(loose(0.0, 0.0)));

    let board = drawn(json!([note, reply]));
    let (from, to, _, cap_end) = connector(&board, "reply");
    assert_eq!(from, DocEnd::Point { x: 105.0, y: 0.0 });
    assert_eq!(to, DocEnd::Point { x: 0.0, y: 0.0 });
    assert_eq!(cap_end, StrokeCap::TriangleArrow);
}

/// A state of a state diagram, and its transition to itself as the old canvas wrote it: the
/// route Pele drew out from the state's right side and back, sampled, bound to the state at
/// both ends. `heads` are its `startArrowhead` and `endArrowhead`.
fn state_and_loop(heads: (Value, Value)) -> (Value, Value) {
    let state = json!({"id": "still", "type": "rectangle", "x": 13, "y": 1058, "width": 58,
                           "height": 44, "roundness": {"type": 3}});
    let joins = (Some("still"), Some("still"));
    let ring = arrow("ring", (70.5, 1072.0), json!(loop_route()), heads, joins);
    (state, focused(ring, (-0.14, 0.07)))
}

/// The route of [`state_and_loop`]'s loop, relative to where it starts.
fn loop_route() -> Vec<[f64; 2]> {
    vec![
        [-0.5, -0.5],
        [8.5, -1.3],
        [14.7, 0.3],
        [18.6, 3.8],
        [20.3, 8.3],
        [19.9, 12.8],
        [17.4, 16.6],
        [13.0, 18.7],
        [0.5, 16.5],
    ]
}

/// Checks that `head`, three points, outline the triangle head that Excalidraw draws at `tip`,
/// the end of a route whose last step comes from `before`: two sides as long as half that
/// step (at most 15), 25° off the way back along it, and the tip again, which closes it.
fn is_the_head_at(head: &[[f64; 2]], tip: [f64; 2], before: [f64; 2]) {
    assert_eq!(head.len(), 3, "{head:?}");
    assert_eq!(head[2], tip, "{head:?}");
    let back = [before[0] - tip[0], before[1] - tip[1]];
    let step = back[0].hypot(back[1]);
    let mut turns = Vec::new();
    for side in &head[..2] {
        let out = [side[0] - tip[0], side[1] - tip[1]];
        let length = out[0].hypot(out[1]);
        assert!((length - (step / 2.0).min(15.0)).abs() < 1e-9, "{head:?}");
        let cos = (back[0] * out[0] + back[1] * out[1]) / (step * length);
        assert!((cos - 25f64.to_radians().cos()).abs() < 1e-9, "{head:?}");
        turns.push(back[0] * out[1] - back[1] * out[0]);
    }
    assert!(turns[0] * turns[1] < 0.0, "one side to each side: {head:?}");
}

#[test]
fn a_loop_back_to_one_element_is_drawn_along_its_route_with_its_head() {
    // A connector can't join one element at both ends (the engine would draw a stub from the
    // state to where the loop ends), so the loop joins neither and is drawn as Excalidraw drew
    // it: ink along its route, which has no cap, with its head drawn in it at its end.
    let (state, ring) = state_and_loop((json!(null), json!("triangle")));
    let imported = read(json!([state.clone(), ring.clone()]));
    let item = with_id(&imported, "ring");
    assert_eq!(item.kind, "line");
    assert_eq!(item.start, Some(loose(70.0, 1071.5)));
    assert_eq!(item.end, Some(loose(71.0, 1088.5)));
    let route = loop_route();
    let points = item.points.as_ref().unwrap();
    assert_eq!(points[..route.len()], route[..]);
    is_the_head_at(&points[route.len()..], route[8], route[7]);

    // On the board it is ink out from the state's right side, as wide as the loop, and there
    // is no connector.
    let mut board = drawn(json!([state, ring]));
    let listed = elements(&mut board);
    assert_eq!(of_type(&listed, "connector").len(), 0, "{listed:?}");
    let ink = of_type(&listed, "ink");
    assert_eq!(ink.len(), 1, "{listed:?}");
    assert!(ink[0]["width"].as_f64().unwrap() >= 20.0, "{:?}", ink[0]);
    assert!(ink[0]["x"].as_f64().unwrap() >= 68.0, "{:?}", ink[0]);

    // A loop whose head is at its start is turned around first: its head is drawn where it
    // points, at the start of the route as written.
    let (state, back) = state_and_loop((json!("triangle"), json!(null)));
    let imported = read(json!([state, back]));
    let item = with_id(&imported, "ring");
    assert_eq!(item.kind, "line");
    let points = item.points.as_ref().unwrap();
    assert_eq!(points.len(), route.len() + 3);
    // Relative to its new start, the old end; the head closes at the old start.
    let to_old = |p: [f64; 2]| [p[0] - route[8][0], p[1] - route[8][1]];
    is_the_head_at(&points[route.len()..], to_old(route[0]), to_old(route[1]));

    // With a label it stays a connector between the ends of its route, free at both, so that
    // the label isn't lost: ink has no text.
    let (state, ring) = state_and_loop((json!(null), json!("triangle")));
    let labelled = json!([state, ring, label("tick", "ring", "tick")]);
    let item = with_id(&read(labelled.clone()), "ring").clone();
    assert_eq!(item.kind, "arrow");
    assert_eq!(item.start, Some(loose(70.0, 1071.5)));
    assert_eq!(item.end, Some(loose(71.0, 1088.5)));
    let mut board = drawn(labelled);
    let listed = elements(&mut board);
    let connectors = of_type(&listed, "connector");
    assert_eq!(connectors.len(), 1, "{listed:?}");
    assert_eq!(connectors[0]["text"], json!("tick"));
}

#[test]
fn an_id_the_scene_does_not_have_is_not_taken_for_a_node_of_the_board() {
    // Excalidraw ids are strings, but a scene of numbers is as good, and a node id is a number
    // too: an element that names "5" when the scene has no "5" must not get node 5 of the board.
    let mut board = board();
    let made = |board: &mut BoardDoc, command: &str, params: Value| {
        board.execute(command, &params).unwrap()["id"].to_string()
    };
    let sticky = made(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    let section = made(
        &mut board,
        "node.create",
        json!({"type": "section", "x": 500, "y": 500}),
    );
    let mut outside = rectangle("box", 600.0, 600.0);
    outside["frameId"] = json!(section);
    let notes = import(
        &mut board,
        &scene(json!([
            outside,
            {"id": "arrow", "type": "arrow", "x": 100, "y": 300, "points": [[0, 0], [200, 50]],
             "startBinding": {"elementId": sticky}, "endBinding": {"elementId": sticky}},
        ])),
    )
    .unwrap();
    assert_eq!(notes, Vec::<String>::new());

    let doc = board.doc().unwrap();
    let arrow = skeleton::resolve(&doc, "arrow").unwrap();
    let Some(BoardItem::Connector(connector)) = doc.node(arrow).unwrap().board.clone() else {
        panic!("not a connector");
    };
    assert_eq!((connector.from.node(), connector.to.node()), (None, None));
    // Nor did the rectangle go into the section because its `frameId` said so.
    let rectangle = skeleton::resolve(&doc, "box").unwrap();
    assert_eq!(doc.node(rectangle).unwrap().parent, doc.first_page());
}
