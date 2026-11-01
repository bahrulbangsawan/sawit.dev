//! Colours: shapes, text, connectors, ink, stickies and sections, whatever the user last picked.

use figcraft_engine::board::BoardTool;
use figcraft_engine::params::id_of;

use super::*;

#[test]
fn shape_colours_follow_the_spec() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50, "backgroundColor": "#ffc9c9"},
            {"type": "rectangle", "x": 0, "y": 100, "width": 100, "height": 50, "backgroundColor": "blue"},
            {"type": "rectangle", "x": 0, "y": 200, "width": 100, "height": 50, "strokeColor": "#e03131"},
            {"type": "rectangle", "x": 0, "y": 300, "width": 100, "height": 50,
             "backgroundColor": "transparent", "strokeColor": "#1971c2"},
            {"type": "ellipse", "x": 0, "y": 400, "width": 100, "height": 50,
             "backgroundColor": "#ffc9c9", "strokeColor": "#e03131"},
        ]),
    );
    let drawn = |i| outline(&b, node(&done, i));
    assert_eq!(fill(&b, drawn(0)), css("#ffc9c9"));
    assert_eq!(
        fill(&b, drawn(1)),
        Some(palette("blue")),
        "board colour names work"
    );
    assert_ne!(
        stroke(&b, drawn(1)),
        Some(palette("blue")),
        "its line is the deeper shade"
    );
    assert_eq!(
        stroke(&b, drawn(2)),
        css("#e03131"),
        "the outline is stroked, not the frame"
    );
    assert!(
        b.doc()
            .unwrap()
            .node(node(&done, 2))
            .unwrap()
            .strokes
            .is_empty()
    );
    assert!(
        b.doc().unwrap().node(drawn(3)).unwrap().fills.is_empty(),
        "transparent is no fill"
    );
    assert_eq!(
        stroke(&b, drawn(3)),
        css("#1971c2"),
        "and the shape is still drawn"
    );
    assert_eq!(fill(&b, drawn(4)), css("#ffc9c9"));
    assert_eq!(
        stroke(&b, drawn(4)),
        css("#e03131"),
        "the stroke colour wins over the shade"
    );
}

#[test]
fn text_is_coloured_by_stroke_color_and_has_no_background() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "text", "x": 0, "y": 0, "text": "red", "strokeColor": "#e03131",
             "backgroundColor": "transparent"},
            {"type": "text", "x": 0, "y": 40, "text": "plain", "backgroundColor": "#ffc9c9"},
        ]),
    );
    let red = node(&done, 0);
    assert_eq!(
        fill(&b, red),
        css("#e03131"),
        "Excalidraw's text colour is its stroke"
    );
    assert!(
        b.doc().unwrap().node(red).unwrap().strokes.is_empty(),
        "the glyphs aren't outlined"
    );
    let plain = node(&done, 1);
    assert_ne!(
        fill(&b, plain),
        css("#ffc9c9"),
        "a background is never drawn behind text"
    );
    assert!(
        !b.doc().unwrap().node(plain).unwrap().fills.is_empty(),
        "and the text stays visible"
    );
}

#[test]
fn connector_and_ink_colours_are_their_stroke_and_never_the_background() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "arrow", "x": 0, "y": 0, "width": 100, "strokeColor": "#e03131", "backgroundColor": "#ffc9c9"},
            {"type": "arrow", "x": 0, "y": 50, "width": 100, "backgroundColor": "#ffc9c9"},
            {"type": "freedraw", "x": 0, "y": 100, "points": [[0, 0], [30, 30]],
             "strokeColor": "#1971c2", "backgroundColor": "#ffc9c9"},
            {"type": "line", "x": 0, "y": 150, "points": [[0, 0], [10, 0], [10, 10]],
             "backgroundColor": "#ffc9c9"},
        ]),
    );
    let doc = b.doc().unwrap();
    assert_eq!(stroke(&b, node(&done, 0)), css("#e03131"));
    assert!(doc.node(node(&done, 0)).unwrap().fills.is_empty());
    let untouched = stroke(&b, node(&done, 1));
    assert!(
        untouched.is_some() && untouched != css("#ffc9c9"),
        "the default line stays"
    );
    assert_eq!(stroke(&b, node(&done, 2)), css("#1971c2"));
    assert!(doc.node(node(&done, 2)).unwrap().fills.is_empty());
    assert_ne!(stroke(&b, node(&done, 3)), css("#ffc9c9"));
}

#[test]
fn sticky_and_frame_colours() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "sticky", "x": 0, "y": 0, "text": "a", "backgroundColor": "green"},
            {"type": "sticky", "x": 300, "y": 0, "text": "b", "backgroundColor": "transparent"},
            {"type": "sticky", "x": 600, "y": 0, "text": "c", "strokeColor": "#e03131"},
            {"type": "frame", "x": 0, "y": 400, "width": 200, "height": 100,
             "backgroundColor": "#e7f5ff", "strokeColor": "#1971c2"},
            {"type": "frame", "x": 300, "y": 400, "width": 200, "height": 100, "backgroundColor": "transparent"},
        ]),
    );
    assert_eq!(fill(&b, node(&done, 0)), Some(palette("green")));
    let doc = b.doc().unwrap();
    assert!(doc.node(node(&done, 1)).unwrap().fills.is_empty());
    assert_eq!(stroke(&b, node(&done, 2)), css("#e03131"));
    assert_eq!(fill(&b, node(&done, 3)), css("#e7f5ff"));
    assert_eq!(stroke(&b, node(&done, 3)), css("#1971c2"));
    assert!(
        doc.node(node(&done, 4)).unwrap().fills.is_empty(),
        "transparent is no fill"
    );
}

#[test]
fn set_colors_is_one_helper_for_every_kind_of_layer() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50},
            {"type": "arrow", "x": 0, "y": 100, "width": 100},
            {"type": "text", "x": 0, "y": 200, "text": "t"},
        ]),
    );
    let (shape, arrow, text) = (node(&done, 0), node(&done, 1), node(&done, 2));
    let mut paint = |id, background, stroke| {
        set_colors(&mut b.session, id, background, stroke).unwrap();
    };
    paint(shape, Some("pink"), Some("#000000"));
    paint(arrow, Some("pink"), Some("#e03131"));
    paint(text, Some("pink"), None);
    // An empty colour is none: nothing changes.
    paint(text, Some(""), Some("  "));
    assert_eq!(fill(&b, outline(&b, shape)), Some(palette("pink")));
    assert_eq!(stroke(&b, outline(&b, shape)), css("#000000"));
    assert_eq!(stroke(&b, arrow), css("#e03131"));
    assert_ne!(fill(&b, text), Some(palette("pink")));
    // A colour the engine doesn't know is its error.
    assert!(set_colors(&mut b.session, shape, Some("banana"), None).is_err());
    assert!(set_colors(&mut b.session, NodeId(99_999), Some("pink"), None).is_err());
}

#[test]
fn what_is_drawn_does_not_depend_on_the_users_tools_or_name() {
    let items = json!([
        // None of these says what colour, which line or whose note.
        {"type": "rectangle", "id": "r", "x": 0, "y": 0, "width": 160, "height": 80,
         "label": {"text": "API"}},
        {"type": "sticky", "id": "s", "x": 400, "y": 0, "text": "note"},
        {"type": "rectangle", "x": 0, "y": 200, "width": 160, "height": 80,
         "backgroundColor": "transparent"},
        {"type": "ellipse", "x": 200, "y": 200, "width": 120, "height": 80},
        {"type": "arrow", "x": 160, "y": 40, "start": {"id": "r"}, "end": {"id": "s"},
         "label": {"text": "calls"}},
        {"type": "line", "x": 0, "y": 400, "points": [[0, 0], [200, 120]]},
        {"type": "arrow", "x": 300, "y": 400, "width": 100, "height": 60},
        {"type": "line", "x": 0, "y": 600, "points": [[0, 0], [50, 0], [50, 50]]},
        {"type": "freedraw", "x": 300, "y": 600, "points": [[0, 0], [30, 30], [60, 0]]},
        {"type": "text", "x": 0, "y": 800, "text": "plain"},
        {"type": "frame", "x": 600, "y": 0, "width": 300, "height": 200, "name": "Group"},
        // What a skeleton does name is drawn as named, whatever the user picked.
        {"type": "rectangle", "x": 600, "y": 300, "width": 160, "height": 80,
         "backgroundColor": "blue", "strokeColor": "#e03131"},
        {"type": "sticky", "x": 800, "y": 300, "text": "green", "backgroundColor": "green"},
        {"type": "freedraw", "x": 600, "y": 500, "points": [[0, 0], [40, 40]],
         "strokeColor": "#1971c2"},
        {"type": "arrow", "x": 800, "y": 500, "width": 100, "strokeColor": "#2f9e44"},
    ]);
    let mut fresh = board();
    let mut picked = board_with_other_choices();
    let done = add(&mut fresh, items.clone());
    assert_eq!(add(&mut picked, items), done, "the same ids and notes");
    assert!(done.notes.is_empty(), "{:?}", done.notes);
    assert_same_document(&fresh, &picked);

    // And the look is the engine's own default on a fresh board, not the user's.
    let tool = BoardTool::default();
    let mut engine = board();
    let made = engine
        .execute(
            "board.shape",
            &json!({"shape": "rectangle", "x": 0, "y": 0}),
        )
        .unwrap();
    let reference = made.get("id").and_then(id_of).unwrap();
    for b in [&fresh, &picked] {
        let at = |i| node(&done, i);
        assert_eq!(fill(b, outline(b, at(0))), Some(tool.sticky), "a rectangle");
        assert_eq!(fill(b, outline(b, at(3))), Some(tool.sticky), "an ellipse");
        assert_eq!(fill(b, at(1)), Some(tool.sticky), "a sticky");
        assert!(
            matches!(board_item(b, at(1)), BoardItem::Sticky { author } if author.is_empty()),
            "a sticky is not signed"
        );
        let sticky = b.doc().unwrap().node(at(1)).unwrap().children.len();
        assert_eq!(sticky, 1, "it holds the note and no author line");
        let transparent = outline(b, at(2));
        assert!(b.doc().unwrap().node(transparent).unwrap().fills.is_empty());
        assert_eq!(
            stroke(b, transparent),
            stroke(&engine, outline(&engine, reference)),
            "a shape with no fill is outlined as the engine does by default"
        );
        for ink in [7, 8] {
            assert_eq!(stroke(b, at(ink)), Some(tool.marker), "ink {ink}");
        }
        let lines = connectors(b);
        assert_eq!(
            lines.len(),
            4,
            "the joining arrow, the line and two free arrows"
        );
        assert!(
            lines.iter().all(|c| c.routing == Routing::Straight),
            "connectors are straight: {lines:?}"
        );
    }
    assert_eq!(
        fill(&picked, outline(&picked, node(&done, 11))),
        Some(palette("blue"))
    );
    assert_eq!(
        stroke(&picked, outline(&picked, node(&done, 11))),
        css("#e03131")
    );
    assert_eq!(fill(&picked, node(&done, 12)), Some(palette("green")));
    assert_eq!(stroke(&picked, node(&done, 13)), css("#1971c2"));
    assert_eq!(stroke(&picked, node(&done, 14)), css("#2f9e44"));
}
