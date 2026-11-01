//! Where elements go: points and the offset, free ends, what an end can join, what has no length.

use figcraft_doc::End as DocEnd;

use super::*;

#[test]
fn points_are_relative_to_x_y_and_the_offset_moves_everything() {
    let mut b = board();
    let items = skeletons(json!([
        {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
        {"type": "freedraw", "x": 100, "y": 200, "points": [[0, 0], [10, 0], [10, 10]]},
        {"type": "arrow", "x": 10, "y": 20, "points": [[0, 0], [100, 50]]},
        {"type": "arrow", "x": 0, "y": 0, "start": {"x": 300, "y": 40}, "end": {"x": 310, "y": 90}},
    ]));
    let done = apply(&mut b, &items, (5.0, 7.0)).unwrap();
    assert_eq!(bounds_of(&b, node(&done, 0)), [15.0, 27.0, 115.0, 77.0]);
    // Ink is drawn from its points: (105, 207) to (115, 217).
    assert_eq!(bounds_of(&b, node(&done, 1)), [105.0, 207.0, 115.0, 217.0]);
    let moved = connector(&b, node(&done, 2));
    assert_eq!(moved.from, DocEnd::Point { x: 15.0, y: 27.0 });
    assert_eq!(moved.to, DocEnd::Point { x: 115.0, y: 77.0 });
    let free = connector(&b, node(&done, 3));
    assert_eq!(
        free.from,
        DocEnd::Point { x: 305.0, y: 47.0 },
        "free ends are in page coordinates"
    );
    assert_eq!(free.to, DocEnd::Point { x: 315.0, y: 97.0 });
}

#[test]
fn the_offset_moves_every_kind_of_element() {
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='30'>\
                   <rect width='40' height='30'/></svg>";
    let items = skeletons(json!([
        {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
        {"type": "sticky", "x": 10, "y": 100, "text": "s"},
        {"type": "text", "x": 10, "y": 400, "text": "t"},
        {"type": "frame", "x": 10, "y": 500, "width": 200, "height": 100},
        {"type": "image", "x": 10, "y": 700, "svg": svg},
        {"type": "image", "x": 10, "y": 800, "data": PNG, "width": 20, "height": 10},
    ]));
    let (mut moved, mut still) = (board(), board());
    let moved_by = apply(&mut moved, &items, (1000.0, 2000.0)).unwrap();
    let still_at = apply(&mut still, &items, (0.0, 0.0)).unwrap();
    assert_eq!(moved_by.ids.len(), 6);
    for i in 0..6 {
        let (a, b) = (
            bounds_of(&moved, node(&moved_by, i)),
            bounds_of(&still, node(&still_at, i)),
        );
        assert_eq!([a[0] - 1000.0, a[1] - 2000.0], [b[0], b[1]], "element {i}");
        assert_eq!(
            [a[2] - a[0], a[3] - a[1]],
            [b[2] - b[0], b[3] - b[1]],
            "its size, element {i}"
        );
    }
}

#[test]
fn a_free_end_without_coordinates_takes_the_position_the_arrow_is_drawn_at() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "arrow", "x": 10, "y": 20, "width": 160, "height": 0},
            {"type": "arrow", "x": 10, "y": 50},
            {"type": "line", "x": 10, "y": 80, "points": [[0, 0], [100, 50]]},
            {"type": "arrow", "x": 10, "y": 120, "points": [[0, 0], [40, 0], [40, 30]]},
            {"type": "arrow", "x": 10, "y": 200, "height": 60},
            {"type": "arrow", "x": 10, "y": 300, "width": 30, "height": 40},
        ]),
    );
    let ends = |i| {
        let c = connector(&b, node(&done, i));
        (c.from, c.to)
    };
    let point = |x, y| DocEnd::Point { x, y };
    assert_eq!(ends(0), (point(10.0, 20.0), point(170.0, 20.0)));
    assert_eq!(
        ends(1),
        (point(10.0, 50.0), point(110.0, 50.0)),
        "100 to the right by default"
    );
    assert_eq!(ends(2), (point(10.0, 80.0), point(110.0, 130.0)));
    assert_eq!(
        ends(3),
        (point(10.0, 120.0), point(50.0, 150.0)),
        "the last point; the bend is lost"
    );
    assert_eq!(
        ends(4),
        (point(10.0, 200.0), point(10.0, 260.0)),
        "straight down"
    );
    assert_eq!(ends(5), (point(10.0, 300.0), point(40.0, 340.0)));
}

#[test]
fn arrows_can_name_node_ids_and_pages_are_not_joined() {
    let mut b = board();
    let first = add(
        &mut b,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "old"}]),
    );
    let old = node(&first, 0);
    let done = add(
        &mut b,
        json!([
            {"type": "sticky", "id": "new", "x": 400, "y": 0, "text": "new"},
            {"type": "arrow", "x": 0, "y": 0, "start": {"id": old.to_string()}, "end": {"id": "new"}},
            {"type": "arrow", "x": 0, "y": 100, "start": {"id": "1", "x": 5, "y": 6}, "end": {"x": 90, "y": 100}},
        ]),
    );
    assert!(done.notes.is_empty(), "{:?}", done.notes);
    let joined = connector(&b, node(&done, 1));
    assert_eq!(joined.from.node(), Some(old));
    assert_eq!(joined.to.node(), Some(node(&done, 0)));
    let page = connector(&b, node(&done, 2));
    assert_eq!(
        page.from,
        DocEnd::Point { x: 5.0, y: 6.0 },
        "the page can't be joined: the end stays free"
    );
}

#[test]
fn an_arrow_cannot_join_a_connector_or_one_layer_twice() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 100, "height": 50},
            {"type": "rectangle", "id": "b", "x": 300, "y": 0, "width": 100, "height": 50},
            {"type": "arrow", "id": "c", "x": 0, "y": 0, "start": {"id": "a"}, "end": {"id": "b"}},
            {"type": "arrow", "x": 0, "y": 100, "points": [[0, 0], [60, 40]],
             "start": {"id": "a"}, "end": {"id": "a"}},
            {"type": "arrow", "x": 0, "y": 300, "start": {"id": "c"}, "end": {"id": "b"}},
        ]),
    );
    assert_eq!(done.ids.len(), 5);
    assert_eq!(done.notes.len(), 1, "{:?}", done.notes);
    assert!(done.notes[0].contains("elements[3]"), "{:?}", done.notes);
    let looped = connector(&b, node(&done, 3));
    assert_eq!(looped.from.node(), Some(node(&done, 0)), "the start stays");
    assert_eq!(
        looped.to,
        DocEnd::Point { x: 60.0, y: 140.0 },
        "the end is left where it was drawn"
    );
    let onto_arrow = connector(&b, node(&done, 4));
    assert_eq!(
        onto_arrow.from,
        DocEnd::Point { x: 0.0, y: 300.0 },
        "an arrow can't be an end"
    );
    assert_eq!(onto_arrow.to.node(), Some(node(&done, 1)));
}

#[test]
fn what_has_no_length_is_skipped_with_a_note() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "arrow", "x": 5, "y": 5, "width": 0, "height": 0},
            {"type": "line", "x": 0, "y": 0, "points": [[0, 0], [0, 0]]},
            {"type": "freedraw", "x": 0, "y": 0, "points": [[0, 0]]},
            {"type": "freedraw", "x": 0, "y": 0, "points": [[3, 3], [3, 3], [3, 3]]},
            {"type": "line", "x": 0, "y": 0, "points": [[0, 0], [0, 0], [0, 0]]},
            {"type": "freedraw", "x": 0, "y": 0},
        ]),
    );
    assert!(done.ids.is_empty());
    assert!(layers(&b).is_empty());
    assert_eq!(done.notes.len(), 6, "{:?}", done.notes);
    for (i, note) in done.notes.iter().enumerate() {
        assert!(note.contains(&format!("elements[{i}]")), "{note}");
    }
}
