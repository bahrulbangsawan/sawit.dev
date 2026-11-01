//! Stickies and text with their text, and pictures: SVG, sized images, ones that can't come in.

use figcraft_color::PaintKind;
use figcraft_doc::NodeKind;

use super::*;

#[test]
fn sticky_and_text_come_in_with_their_text() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "sticky", "x": 0, "y": 0, "text": "remember", "backgroundColor": "green"},
            {"type": "text", "x": 0, "y": -40, "text": "Title"},
            {"type": "text", "x": 0, "y": -80, "label": {"text": "from the label"}, "width": 200},
            {"type": "sticky", "x": 300, "y": 0, "label": {"text": "label wins here"}},
        ]),
    );
    assert!(matches!(
        board_item(&b, node(&done, 0)),
        BoardItem::Sticky { .. }
    ));
    assert_eq!(text_in(&b, node(&done, 0)), "remember");
    let doc = b.doc().unwrap();
    let text = |i| match &doc.node(node(&done, i)).unwrap().kind {
        NodeKind::Text(t) => t.characters.clone(),
        other => panic!("not text: {other:?}"),
    };
    assert_eq!(text(1), "Title");
    assert_eq!(text(2), "from the label");
    assert_eq!(text_in(&b, node(&done, 3)), "label wins here");
    assert_eq!(bounds_of(&b, node(&done, 1))[..2], [0.0, -40.0]);
}

#[test]
fn svg_comes_in_as_an_editable_frame() {
    let mut b = board();
    let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='30'>\
                   <rect width='40' height='30' fill='red'/></svg>";
    let done = add(
        &mut b,
        json!([{"type": "image", "x": 10, "y": 20, "svg": svg, "name": "diagram.svg", "width": 40, "height": 30},
                   {"type": "image", "x": 100, "y": 20, "svg": svg}]),
    );
    assert!(done.notes.is_empty(), "{:?}", done.notes);
    assert_eq!(layers(&b).len(), 2);
    let first = bounds_of(&b, node(&done, 0));
    assert_eq!(first, [10.0, 20.0, 50.0, 50.0]);
    assert_eq!(
        bounds_of(&b, node(&done, 1))[..2],
        [100.0, 20.0],
        "with or without a name"
    );
    let doc = b.doc().unwrap();
    assert!(doc.node(node(&done, 0)).unwrap().kind.is_frame_like());
}

#[test]
fn a_picture_takes_the_size_the_skeleton_gives_it() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "image", "x": 10, "y": 20, "data": PNG, "name": "dot.png", "width": 40, "height": 20},
            {"type": "image", "x": 10, "y": 100, "data": PNG, "width": 30},
            {"type": "image", "x": 10, "y": 200, "data": format!("data:image/png;base64,{PNG}")},
        ]),
    );
    assert!(done.notes.is_empty(), "{:?}", done.notes);
    let doc = b.doc().unwrap();
    let picture = doc.node(node(&done, 0)).unwrap();
    assert!(matches!(
        picture.fills.first().map(|p| &p.kind),
        Some(PaintKind::Image(_))
    ));
    assert_eq!(bounds_of(&b, node(&done, 0)), [10.0, 20.0, 50.0, 40.0]);
    assert_eq!(
        bounds_of(&b, node(&done, 1)),
        [10.0, 100.0, 40.0, 130.0],
        "a lone width keeps the proportion"
    );
    let natural = bounds_of(&b, node(&done, 2));
    assert_eq!(natural[..2], [10.0, 200.0], "a data URL works as it is");
    assert!(natural[2] > natural[0] && natural[3] > natural[1]);
}

#[test]
fn a_picture_that_cannot_be_imported_is_noted_and_the_rest_still_comes_in() {
    let mut b = board();
    let done = add(
        &mut b,
        json!([
            {"type": "image", "x": 0, "y": 0, "data": "!!not base64!!", "name": "broken.png"},
            {"type": "image", "x": 0, "y": 0, "data": "AAAA", "name": "garbage.png"},
            {"type": "image", "x": 0, "y": 0, "svg": "<not svg>"},
            {"type": "image", "x": 0, "y": 0},
            {"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10},
        ]),
    );
    assert_eq!(done.ids.len(), 1);
    assert_eq!(layers(&b).len(), 1);
    assert_eq!(done.notes.len(), 4, "{:?}", done.notes);
    for (note, element) in done.notes.iter().zip(0..4) {
        assert!(note.contains(&format!("elements[{element}]")), "{note}");
    }
}
