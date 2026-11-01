//! Arrows: what they join, and which way they point or whether they are lines, by their heads.

use super::*;

#[test]
fn arrows_join_what_the_scene_draws_and_are_free_otherwise() {
    let bound = |id: &str| json!({"elementId": id, "focus": 0, "gap": 1});
    let mut deleted = rectangle("gone", 400.0, 0.0);
    deleted["isDeleted"] = json!(true);
    let imported = read(json!([
        rectangle("a", 0.0, 0.0),
        deleted,
        {"id": "embed", "type": "embeddable", "x": 600, "y": 0, "width": 50, "height": 50},
        // Bound at the start to a drawn element, at the end to an embed, which isn't drawn.
        {"id": "one", "type": "arrow", "x": 100, "y": 25, "points": [[0, 0], [300, 40]],
         "startBinding": bound("a"), "endBinding": bound("embed")},
        // Both ends name what isn't there, or was deleted; one is a number, which is a node id
        // on the board too.
        {"id": "two", "type": "arrow", "x": 10, "y": 20, "points": [[0, 0], [70, 80]],
         "startBinding": bound("gone"), "endBinding": bound("3")},
        // Not bound at all, with the points relative to the arrow's place.
        {"id": "three", "type": "line", "x": 5, "y": 6, "points": [[0, 0], [20, 30]]},
    ]));
    assert_eq!(imported.notes, ["skipped 1 embed"]);
    let end = |id: &str, which: &str| {
        let item = with_id(&imported, id);
        let end = if which == "start" {
            &item.start
        } else {
            &item.end
        };
        end.clone().unwrap()
    };
    let free = |x: f64, y: f64| End {
        id: None,
        x: Some(x),
        y: Some(y),
    };
    // A joined end names the element and nothing else.
    assert_eq!(
        end("one", "start"),
        End {
            id: Some("a".into()),
            x: None,
            y: None
        }
    );
    assert_eq!(end("one", "end"), free(400.0, 65.0));
    assert_eq!(end("two", "start"), free(10.0, 20.0));
    assert_eq!(end("two", "end"), free(80.0, 100.0));
    assert_eq!(end("three", "start"), free(5.0, 6.0));
    assert_eq!(end("three", "end"), free(25.0, 36.0));
    // The points go through as they are, relative to `x`, `y`.
    assert_eq!(
        with_id(&imported, "one").points,
        Some(vec![[0.0, 0.0], [300.0, 40.0]])
    );
    assert_eq!(with_id(&imported, "one").x, 100.0);
}

// Arrowheads: where an arrow's heads are is which way it points, and an arrow with none is a
// line. The scenes here are written inline, so that the fixture's counts stay what they are.

#[test]
fn an_arrow_with_no_head_is_a_line() {
    // A link with no arrowhead (Mermaid's `A --- B`, say) is written as an arrow whose heads
    // are both `null`. It is drawn as a plain line, so it comes in as one that joins what it
    // joined, and not as an arrow with a head at its end.
    let heads = (json!(null), json!(null));
    let link = arrow(
        "link",
        (100.0, 25.0),
        json!([[0, 0], [200, 0]]),
        heads,
        (Some("a"), Some("b")),
    );
    let imported = read(between(link.clone()));
    assert_eq!(imported.notes, Vec::<String>::new());
    let item = with_id(&imported, "link");
    assert_eq!(item.kind, "line");
    assert_eq!(item.start, Some(joined("a")));
    assert_eq!(item.end, Some(joined("b")));
    assert_eq!(item.points, Some(vec![[0.0, 0.0], [200.0, 0.0]]));

    let board = drawn(between(link));
    let (from, to, cap_start, cap_end) = connector(&board, "link");
    assert_eq!(from.node(), node_of(&board, "a"));
    assert_eq!(to.node(), node_of(&board, "b"));
    assert_eq!((cap_start, cap_end), (StrokeCap::None, StrokeCap::None));
}

#[test]
fn an_arrow_with_a_head_at_its_start_only_is_turned_around() {
    // The head is where an arrow points, and a connector has one head, at its end. So the
    // arrow is drawn from its end to its start: the same line, with the head where it was.
    let heads = (json!("arrow"), json!(null));
    let link = arrow(
        "link",
        (100.0, 25.0),
        json!([[0, 0], [200, 0]]),
        heads,
        (Some("a"), Some("b")),
    );
    let imported = read(between(link.clone()));
    let item = with_id(&imported, "link");
    assert_eq!(item.kind, "arrow");
    // It starts where it ended, and what it joined at its start is what it points at now.
    assert_eq!(item.start, Some(joined("b")));
    assert_eq!(item.end, Some(joined("a")));
    assert_eq!((item.x, item.y), (300.0, 25.0));
    assert_eq!(item.points, Some(vec![[0.0, 0.0], [-200.0, 0.0]]));

    let board = drawn(between(link));
    let (from, to, cap_start, cap_end) = connector(&board, "link");
    assert_eq!(from.node(), node_of(&board, "b"));
    assert_eq!(to.node(), node_of(&board, "a"));
    assert_eq!(
        (cap_start, cap_end),
        (StrokeCap::None, StrokeCap::TriangleArrow)
    );
}

#[test]
fn a_turned_around_arrow_keeps_its_route_its_free_ends_and_its_label() {
    // Free at both ends, through a bend, coloured and labelled: all of it runs the other way
    // round. The first point is still the arrow's own place (`x`, `y`), so the points are
    // given relative to the place where the arrow now starts.
    let mut bent = arrow(
        "bent",
        (10.0, 20.0),
        json!([[0, 0], [60, -30], [200, 40]]),
        (json!("triangle"), json!(null)),
        (None, None),
    );
    bent["strokeColor"] = json!("#ff0000");
    let imported = read(json!([bent.clone(), label("bent-label", "bent", "back")]));
    assert_eq!(imported.notes, Vec::<String>::new());
    let item = with_id(&imported, "bent");
    assert_eq!(item.kind, "arrow");
    assert_eq!((item.x, item.y), (210.0, 60.0));
    assert_eq!(
        item.points,
        Some(vec![[0.0, 0.0], [-140.0, -70.0], [-200.0, -40.0]])
    );
    assert_eq!(item.start, Some(loose(210.0, 60.0)));
    assert_eq!(item.end, Some(loose(10.0, 20.0)));
    assert_eq!(item.stroke_color.as_deref(), Some("#ff0000"));
    assert_eq!(item.label.as_ref().map(|l| l.text.as_str()), Some("back"));

    // On the board it runs from where it ended to where it started, with its head there.
    let board = drawn(json!([bent, label("bent-label", "bent", "back")]));
    let (from, to, cap_start, cap_end) = connector(&board, "bent");
    assert_eq!(from, DocEnd::Point { x: 210.0, y: 60.0 });
    assert_eq!(to, DocEnd::Point { x: 10.0, y: 20.0 });
    assert_eq!(
        (cap_start, cap_end),
        (StrokeCap::None, StrokeCap::TriangleArrow)
    );
}

#[test]
fn an_arrow_with_a_head_at_its_end_or_at_both_ends_is_as_it_was() {
    // Whatever the kind of head is. A connector has one head, at its end, so a second one at
    // the start isn't carried, and the arrow keeps pointing the way it was drawn.
    for (id, start, end) in [
        ("arrow", json!(null), json!("arrow")),
        ("triangle", json!(null), json!("triangle_outline")),
        ("crow", json!(null), json!("crowfoot_many")),
        ("both", json!("arrow"), json!("arrow")),
        ("kinds", json!("dot"), json!("bar")),
    ] {
        let link = arrow(
            id,
            (100.0, 25.0),
            json!([[0, 0], [200, 0]]),
            (start, end),
            (Some("a"), Some("b")),
        );
        let imported = read(between(link.clone()));
        let item = with_id(&imported, id);
        assert_eq!(item.kind, "arrow", "{id}");
        assert_eq!(item.start, Some(joined("a")), "{id}");
        assert_eq!(item.end, Some(joined("b")), "{id}");
        assert_eq!((item.x, item.y), (100.0, 25.0), "{id}");
        assert_eq!(item.points, Some(vec![[0.0, 0.0], [200.0, 0.0]]), "{id}");

        let board = drawn(between(link));
        let (from, to, cap_start, cap_end) = connector(&board, id);
        assert_eq!(from.node(), node_of(&board, "a"), "{id}");
        assert_eq!(to.node(), node_of(&board, "b"), "{id}");
        assert_eq!(
            (cap_start, cap_end),
            (StrokeCap::None, StrokeCap::TriangleArrow),
            "{id}"
        );
    }
}

#[test]
fn a_head_the_scene_does_not_mention_is_read_as_excalidraw_reads_it() {
    // Excalidraw draws an arrow with no `endArrowhead` with a head at its end (what
    // `restoreElements` does, which the canvas runs on every scene it loads), and one with no
    // `startArrowhead` without. A line has no heads, whatever it says of them.
    let plain =
        |id: &str| json!({"id": id, "type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [50, 0]]});
    let mut start_only = plain("start-only");
    start_only["startArrowhead"] = json!("arrow");
    let mut no_end = plain("no-end");
    no_end["endArrowhead"] = json!(null);
    let line = json!({"id": "line", "type": "line", "x": 0, "y": 0, "points": [[0, 0], [50, 0]],
                          "startArrowhead": "arrow", "endArrowhead": null});
    let imported = read(json!([plain("plain"), start_only, no_end, line]));
    let kind = |id: &str| with_id(&imported, id).kind.as_str();
    assert_eq!(kind("plain"), "arrow");
    // Both heads (the start's is written, the end's is the default): not turned around.
    assert_eq!(kind("start-only"), "arrow");
    assert_eq!(
        with_id(&imported, "start-only").points,
        with_id(&imported, "plain").points
    );
    assert_eq!(kind("no-end"), "line");
    // A line stays a line as it is, whatever heads it names, and is never turned around.
    let line = with_id(&imported, "line");
    assert_eq!(line.kind, "line");
    assert_eq!((line.x, line.y), (0.0, 0.0));
    assert_eq!(line.points, Some(vec![[0.0, 0.0], [50.0, 0.0]]));
}

#[test]
fn a_headless_arrow_that_bends_and_joins_something_is_a_connector() {
    // The Mermaid links on an old canvas are written with the 10 to 19 points that were
    // sampled along their route. A line through more than two points is ink, which joins
    // nothing, so a headless arrow keeps its first and its last point: it runs between what
    // it joined, like an arrow does.
    let route = json!([[0, 0], [60, -30], [140, -30], [200, 0]]);
    let heads = (json!(null), json!(null));
    for (id, joins) in [("both", (Some("a"), Some("b"))), ("end", (None, Some("b")))] {
        let link = arrow(id, (100.0, 25.0), route.clone(), heads.clone(), joins);
        let imported = read(between(link.clone()));
        let item = with_id(&imported, id);
        assert_eq!(item.kind, "line", "{id}");
        assert_eq!(item.points, Some(vec![[0.0, 0.0], [200.0, 0.0]]), "{id}");

        let mut board = drawn(between(link));
        let (from, to, cap_start, cap_end) = connector(&board, id);
        match joins.0 {
            Some(start) => assert_eq!(from.node(), node_of(&board, start), "{id}"),
            None => assert_eq!(from, DocEnd::Point { x: 100.0, y: 25.0 }, "{id}"),
        }
        assert_eq!(to.node(), node_of(&board, "b"), "{id}");
        assert_eq!((cap_start, cap_end), (StrokeCap::None, StrokeCap::None));
        // Not ink: the board has the two rectangles and the one connector.
        let listed = elements(&mut board);
        assert_eq!(listed.len(), 3, "{id}: {listed:?}");
        assert_eq!(of_type(&listed, "connector").len(), 1, "{id}");
    }
}

#[test]
fn a_headless_arrow_that_joins_nothing_keeps_its_bends_unless_it_has_a_label() {
    // Free at both ends, it is as an Excalidraw line is: a line through more than two points
    // is ink, and one through two is a connector with no heads.
    let heads = (json!(null), json!(null));
    let free =
        |id: &str, points: Value| arrow(id, (10.0, 10.0), points, heads.clone(), (None, None));
    let bent = free("bent", json!([[0, 0], [30, 5], [60, 0]]));
    let straight = free("straight", json!([[0, 0], [60, 0]]));
    let imported = read(json!([bent.clone(), straight.clone()]));
    assert_eq!(with_id(&imported, "bent").kind, "line");
    assert_eq!(
        with_id(&imported, "bent").points.as_ref().map(Vec::len),
        Some(3)
    );
    let mut board = drawn(json!([bent.clone(), straight]));
    let listed = elements(&mut board);
    assert_eq!(of_type(&listed, "ink").len(), 1, "{listed:?}");
    assert_eq!(of_type(&listed, "connector").len(), 1);
    let (_, _, cap_start, cap_end) = connector(&board, "straight");
    assert_eq!((cap_start, cap_end), (StrokeCap::None, StrokeCap::None));

    // Ink has no text, and a label is not to be lost: one that has a label is a connector
    // through its first and last point.
    let labelled = json!([bent, label("bent-label", "bent", "maybe")]);
    let imported = read(labelled.clone());
    let item = with_id(&imported, "bent");
    assert_eq!(item.kind, "line");
    assert_eq!(item.points, Some(vec![[0.0, 0.0], [60.0, 0.0]]));
    assert_eq!(item.label.as_ref().map(|l| l.text.as_str()), Some("maybe"));
    let mut board = drawn(labelled);
    let listed = elements(&mut board);
    assert_eq!(of_type(&listed, "ink").len(), 0, "{listed:?}");
    let connectors = of_type(&listed, "connector");
    assert_eq!(connectors.len(), 1);
    assert_eq!(connectors[0]["text"], json!("maybe"));
}
