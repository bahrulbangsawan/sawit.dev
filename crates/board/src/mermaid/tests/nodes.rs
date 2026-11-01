//! Nodes: their colours, and the room their labels get.

use figcraft_doc::BoardShape;
use kurbo::{Point, Rect};
use roxmltree::Document;

use super::*;
use crate::mermaid::geometry::shape_box;
use crate::mermaid::text::{set_widest_line, set_widest_word};

#[test]
fn colours_are_pele_s_and_its_neutral_ones_the_canvas_s() {
    let items =
        skeletons("flowchart LR\n  A[Plain] --> B[Red]\n  style B fill:#f9d0c4,stroke:#c00\n");
    let colours: Vec<(&str, Option<&str>, Option<&str>)> = items
        .iter()
        .map(|item| {
            (
                item.kind.as_str(),
                item.background_color.as_deref(),
                item.stroke_color.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        colours,
        [
            // Pele's surface and border: unfilled, outlined in ink.
            ("rectangle", Some("transparent"), Some(INK)),
            // A colour of the diagram's own.
            ("rectangle", Some("#f9d0c4"), Some("#cc0000")),
            // Pele's line colour.
            ("arrow", None, Some(INK)),
        ]
    );

    // A cluster in Pele's second surface, half see-through: as it looks on white. Its title is
    // a text, whose colour travels in strokeColor.
    let items = skeletons("flowchart LR\n  subgraph S [Group]\n    A\n  end\n");
    let cluster = &items[0];
    assert_eq!(cluster.kind, "rectangle");
    assert_eq!(cluster.background_color.as_deref(), Some("#f4f6f7"));
    assert_eq!(cluster.stroke_color.as_deref(), Some(INK));
    let title = items
        .iter()
        .find(|item| item.text.as_deref() == Some("Group"));
    let title = title.expect("the cluster's title");
    assert_eq!(title.kind, "text");
    assert_eq!(title.stroke_color.as_deref(), Some("#6e6e6e"));
    assert_eq!(title.background_color, None);
}

/// The width a board shape skeleton's label is set in: its width inside the shape's insets.
fn room(item: &Skeleton) -> f64 {
    let shape = match item.kind.as_str() {
        "ellipse" => BoardShape::Ellipse,
        "diamond" => BoardShape::Diamond,
        _ => BoardShape::Rectangle,
    };
    let [_, right, _, left] = shape.text_insets();
    item.width.expect("a size") * (1.0 - left - right)
}

/// The box Pele drew the first `tag` element of `diagram` in.
fn drawn(diagram: &Diagram, tag: &str) -> Rect {
    let svg = Document::parse(&diagram.svg).unwrap();
    let shape = svg.descendants().find(|n| n.has_tag_name(tag)).unwrap();
    shape_box(shape).unwrap()
}

#[test]
fn a_node_that_is_only_text_gets_an_invisible_box_that_its_edges_join() {
    let items = skeletons("mindmap\n  root((Root))\n    Leaf\n");
    let leaf = items.iter().find(|item| label(item) == Some("Leaf"));
    let leaf = leaf.expect("the leaf, holding its text as a label");
    assert_eq!(leaf.kind, "rectangle");
    assert_eq!(leaf.background_color.as_deref(), Some("transparent"));
    assert_eq!(leaf.stroke_color.as_deref(), Some("transparent"));
    // Room for its text on the one line Pele drew it on.
    assert!(room(leaf) >= set_widest_line("Leaf", PELE_SIZE));
    // The edge from the root joins it.
    let edges = of_kind(&items, "line");
    assert_eq!(edges.len(), 1, "{items:#?}");
    assert_eq!(
        (joins(&items, &edges[0].start), joins(&items, &edges[0].end)),
        ("Root", "Leaf")
    );
}

#[test]
fn a_long_word_gets_room_and_a_long_label_breaks_between_words() {
    // Pele's circle around one long word leaves the engine's label too little room, so the
    // word would be broken. The circle grows around its middle until the word fits.
    let source = "flowchart LR\n  A((Authentication)) --> B[Next]\n";
    let diagram = render(source, &Palette::default()).unwrap();
    let circle = drawn(&diagram, "circle");
    let word = set_widest_word("Authentication", PELE_SIZE);
    let [_, right, _, left] = BoardShape::Ellipse.text_insets();
    assert!(circle.width() * (1.0 - left - right) < word, "{circle:?}");
    let items = to_skeletons(&diagram).unwrap();
    let node = items
        .iter()
        .find(|item| label(item) == Some("Authentication"));
    let node = node.expect("the circle");
    assert!(room(node) >= word - 1e-9, "{node:?}");
    let middle = Point::new(
        node.x + node.width.unwrap() / 2.0,
        node.y + node.height.unwrap() / 2.0,
    );
    assert!(middle.distance(circle.center()) < 1e-6, "{node:?}");
    assert!(near(node.height.unwrap(), circle.height()), "{node:?}");
    // The arrow still joins it: Pele ended it on the circle as drawn.
    let arrows = of_kind(&items, "arrow");
    assert_eq!(arrows.len(), 1, "{items:#?}");
    assert_eq!(
        (
            joins(&items, &arrows[0].start),
            joins(&items, &arrows[0].end)
        ),
        ("Authentication", "Next")
    );

    // A label of several words that doesn't fit on one line is broken between them, as the
    // engine sets it, so the diamond keeps the size Pele drew it at and its neighbours keep
    // their room.
    let label_text = "Is the payload valid?";
    let diagram = render(
        &format!("flowchart LR\n  A{{{label_text}}}\n"),
        &Palette::default(),
    )
    .unwrap();
    let diamond = drawn(&diagram, "polygon");
    let items = to_skeletons(&diagram).unwrap();
    let node = &items[0];
    assert_eq!(
        (node.kind.as_str(), label(node)),
        ("diamond", Some(label_text))
    );
    assert!(
        room(node) < set_widest_line(label_text, PELE_SIZE),
        "{node:?}"
    );
    let (x, width) = (node.x, node.width.unwrap());
    assert!(
        near(x, diamond.x0) && near(width, diamond.width()),
        "{node:?}"
    );
}
