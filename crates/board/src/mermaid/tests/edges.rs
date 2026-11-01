//! Edges: which way a connector points, what it joins, and the marks, loops and labels it keeps.

use kurbo::{Point, Rect};

use super::*;
use crate::mermaid::geometry::distance_to_outline;

#[test]
fn an_arrowhead_at_the_start_turns_the_arrow_around() {
    // `Animal <|-- Dog`: Pele draws the line from Animal down to Dog, the triangle at Animal.
    let items = skeletons("classDiagram\n  Animal <|-- Dog\n");
    let arrows = of_kind(&items, "arrow");
    assert_eq!(arrows.len(), 1, "{items:#?}");
    let arrow = arrows[0];
    assert_eq!(
        (joins(&items, &arrow.start), joins(&items, &arrow.end)),
        ("Dog", "Animal")
    );
    // The triangle is the arrow's own head: the only lines left are the classes' dividers.
    let lines = of_kind(&items, "line");
    assert!(
        lines.iter().all(|line| point_count(line) == 2),
        "{lines:#?}"
    );
}

#[test]
fn marks_a_connector_has_no_cap_for_stay_drawn_and_the_edge_still_joins_its_nodes() {
    // A composition: a diamond at Dog's end of the line, which stops where the diamond begins.
    let items = skeletons("classDiagram\n  Dog *-- Tail\n");
    assert!(of_kind(&items, "arrow").is_empty(), "{items:#?}");
    let lines = of_kind(&items, "line");
    let joined: Vec<&&Skeleton> = lines
        .iter()
        .filter(|line| line.start.is_some() && line.end.is_some())
        .collect();
    assert_eq!(joined.len(), 1, "{items:#?}");
    assert_eq!(
        (
            joins(&items, &joined[0].start),
            joins(&items, &joined[0].end)
        ),
        ("Dog", "Tail")
    );
    // Ink has no fill, so the filled diamond is outlined in its colour.
    let diamond = lines.iter().find(|line| point_count(line) == 5);
    let diamond = diamond.expect("the diamond, closed: four corners and the first again");
    assert_eq!(diamond.stroke_color.as_deref(), Some(INK));

    // A double arrow: one head is the connector's, the other stays where Pele drew it.
    let items = skeletons("flowchart LR\n  A <--> B\n");
    let arrows = of_kind(&items, "arrow");
    assert_eq!(arrows.len(), 1, "{items:#?}");
    assert_eq!(
        (
            joins(&items, &arrows[0].start),
            joins(&items, &arrows[0].end)
        ),
        ("A", "B")
    );
    let heads = of_kind(&items, "line");
    assert_eq!(heads.len(), 1, "{items:#?}");
    assert_eq!(point_count(heads[0]), 4);
    assert_eq!(heads[0].stroke_color.as_deref(), Some(INK));

    // An entity relationship: a line between the entities, with its label; its bars, circle
    // and crow's foot stay drawn on it.
    let items = skeletons("erDiagram\n  CUSTOMER ||--o{ ORDER : places\n");
    let lines = of_kind(&items, "line");
    let joined: Vec<&&Skeleton> = lines.iter().filter(|line| line.end.is_some()).collect();
    assert_eq!(joined.len(), 1, "{items:#?}");
    assert_eq!(
        (
            joins(&items, &joined[0].start),
            joins(&items, &joined[0].end)
        ),
        ("CUSTOMER", "ORDER")
    );
    assert_eq!(label(joined[0]), Some("places"));
    // Marks that sit on the line leave its ends where they are: it runs straight down.
    let run = joined[0].points.as_ref().unwrap()[1];
    assert!(near(run[0], 0.0) && run[1] > 0.0, "{run:?}");
    assert_eq!(
        lines.len(),
        4,
        "the line, two bars and a crow's foot: {items:#?}"
    );
    assert_eq!(of_kind(&items, "ellipse").len(), 1, "{items:#?}");
}

#[test]
fn a_self_message_keeps_its_loop() {
    // A message to oneself loops out from the lifeline and back. Its ends join nothing, so it
    // is drawn as Pele drew it, its head where Pele put it.
    let items = skeletons("sequenceDiagram\n  Alice->>Alice: Think\n");
    let loops: Vec<&Skeleton> = of_kind(&items, "line")
        .into_iter()
        .filter(|line| point_count(line) > 4)
        .collect();
    assert_eq!(loops.len(), 1, "{items:#?}");
    let route = loops[0].points.as_ref().unwrap();
    let reach = route.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    assert!(reach > 20.0, "it loops out to the right: {route:?}");
    assert!(of_kind(&items, "arrow").is_empty(), "{items:#?}");
    // Its label is a text beside it.
    assert!(
        items
            .iter()
            .any(|item| item.text.as_deref() == Some("Think")),
        "{items:#?}"
    );
}

#[test]
fn a_loop_back_to_its_own_node_keeps_its_loop() {
    // Both ends of a loop are on its node, and a connector can't join one layer at both ends.
    // It joins neither and is drawn as Pele drew it, as a message to oneself is: out of the
    // node and back, its marks and its label where Pele put them.
    let diagrams = [
        (
            "stateDiagram-v2\n  Still --> Still : tick\n  Still --> Moving\n",
            "Still",
            Some("tick"),
            Some(("Still", "Moving")),
        ),
        (
            "flowchart LR\n  A[Retry] --> A\n  A --> B[Done]\n",
            "Retry",
            None,
            Some(("Retry", "Done")),
        ),
        (
            "classDiagram\n  Node --> Node : next\n",
            "Node",
            Some("next"),
            None,
        ),
        (
            "erDiagram\n  PERSON ||--o{ PERSON : parent\n",
            "PERSON",
            Some("parent"),
            None,
        ),
    ];
    for (source, name, said, other) in diagrams {
        let items = skeletons(source);
        // No connector names one node at both ends.
        let id = |end: &Option<End>| end.as_ref().and_then(|end| end.id.clone());
        for item in &items {
            if let (Some(start), Some(end)) = (id(&item.start), id(&item.end)) {
                assert_ne!(start, end, "{source}: {items:#?}");
            }
        }
        // The loop: one line through more than 4 points, joined to nothing, from the node's
        // outline out of it and back.
        let node = items.iter().find(|item| label(item) == Some(name));
        let node = node.expect("the node");
        let frame = Rect::from_origin_size(
            (node.x, node.y),
            (node.width.unwrap(), node.height.unwrap()),
        );
        let lines = of_kind(&items, "line");
        let loops: Vec<&&Skeleton> = lines.iter().filter(|line| point_count(line) > 4).collect();
        assert_eq!(loops.len(), 1, "{source}: {items:#?}");
        let ring = loops[0];
        assert!(
            ring.start.is_none() && ring.end.is_none(),
            "{source}: {ring:#?}"
        );
        let route: Vec<Point> = ring
            .points
            .as_ref()
            .unwrap()
            .iter()
            .map(|p| Point::new(ring.x + p[0], ring.y + p[1]))
            .collect();
        let (first, last) = (route[0], route[route.len() - 1]);
        assert!(
            distance_to_outline(first, frame) <= BIND_DISTANCE
                && distance_to_outline(last, frame) <= BIND_DISTANCE,
            "{source}: {route:?} starts and ends on {frame:?}"
        );
        let out = frame.inflate(10.0, 10.0);
        assert!(
            route.iter().any(|p| !out.contains(*p)),
            "{source}: {route:?} loops out of {frame:?}"
        );
        if let Some(said) = said {
            let text = items.iter().find(|item| item.text.as_deref() == Some(said));
            assert_eq!(
                text.map(|text| text.kind.as_str()),
                Some("text"),
                "{source}"
            );
        }
        // The diagram's other edge still joins its two nodes.
        let arrows = of_kind(&items, "arrow");
        let joined: Vec<(&str, &str)> = arrows
            .iter()
            .map(|arrow| (joins(&items, &arrow.start), joins(&items, &arrow.end)))
            .collect();
        assert_eq!(joined, other.into_iter().collect::<Vec<_>>(), "{source}");
        // The op draws it all, with nothing to say about it.
        let mut board = board();
        let reply = ops::run(&mut board, &json!({"op": "mermaid", "source": source}));
        assert_eq!(reply["ok"], json!(true), "{source}: {reply}");
        assert!(reply.get("notes").is_none(), "{source}: {reply}");
    }
}

#[test]
fn a_message_joins_no_note_under_it() {
    // Pele puts a note 10 px under the message before it, near enough to join. A message runs
    // from lifeline to lifeline, so it stays free and level, whether the note is over one
    // lifeline or over both.
    for source in [
        "sequenceDiagram\n  Alice->>Bob: Hello\n  Note over Bob: Thinks\n",
        "sequenceDiagram\n  Alice->>Bob: Hello\n  Note right of Bob: A note\n  Bob-->>Alice: Hi\n  Note over Alice,Bob: Shared\n  Alice->>Bob: Again\n",
    ] {
        let items = skeletons(source);
        let messages = of_kind(&items, "arrow");
        assert!(!messages.is_empty(), "{source}: {items:#?}");
        for message in messages {
            assert!(
                message.start.is_none() && message.end.is_none(),
                "{source}: {message:#?}"
            );
            let run = message.points.as_ref().unwrap()[1];
            assert_eq!(run[1], 0.0, "{source}: {message:#?}");
        }
        // The notes are still there, each holding its text.
        let notes = ["Thinks", "A note", "Shared"];
        let held = items
            .iter()
            .filter(|item| label(item).is_some_and(|said| notes.contains(&said)));
        assert_eq!(held.count(), source.matches("Note ").count(), "{items:#?}");
        let mut board = board();
        let reply = ops::run(&mut board, &json!({"op": "mermaid", "source": source}));
        assert_eq!(reply["ok"], json!(true), "{source}: {reply}");
        assert!(reply.get("notes").is_none(), "{source}: {reply}");
    }

    // What a message does end at, the box of a participant it makes, it still joins.
    let items = skeletons(
        "sequenceDiagram\n  Alice->>Bob: Hi\n  create participant Carl\n  Alice->>Carl: Hi Carl\n",
    );
    let ends: Vec<Option<&str>> = of_kind(&items, "arrow")
        .iter()
        .map(|arrow| arrow.end.as_ref().map(|_| joins(&items, &arrow.end)))
        .collect();
    assert_eq!(ends, [None, Some("Carl")], "{items:#?}");
    // And a note's own line, which is no message, still joins its note.
    let items = skeletons("classDiagram\n  class Dog\n  note for Dog \"can bark\"\n");
    let joined: Vec<(&str, &str)> = of_kind(&items, "line")
        .iter()
        .filter(|line| line.start.is_some() || line.end.is_some())
        .map(|line| (joins(&items, &line.start), joins(&items, &line.end)))
        .collect();
    assert_eq!(joined, [("can bark", "Dog")], "{items:#?}");
}

#[test]
fn a_lost_message_runs_straight_to_its_cross() {
    // `-x`: the message ends in a cross, which a connector has no cap for. The cross stays
    // drawn, and the line runs on straight to its far side.
    let items = skeletons("sequenceDiagram\n  Alice-xBob: Lost\n");
    assert!(of_kind(&items, "arrow").is_empty(), "{items:#?}");
    let lines = of_kind(&items, "line");
    let run = |line: &&Skeleton| line.points.as_ref().unwrap()[1];
    let across: Vec<&&Skeleton> = lines
        .iter()
        .filter(|line| point_count(line) == 2 && run(line)[1].abs() < 1e-6)
        .collect();
    assert_eq!(across.len(), 1, "the message: {items:#?}");
    let strokes: Vec<&&Skeleton> = lines
        .iter()
        .filter(|line| run(line)[0].abs() > 1e-6 && run(line)[1].abs() > 1e-6)
        .collect();
    assert_eq!(strokes.len(), 2, "the cross: {items:#?}");
    let far_side = strokes
        .iter()
        .map(|stroke| stroke.x + run(stroke)[0].max(0.0))
        .fold(f64::NEG_INFINITY, f64::max);
    let message = across[0];
    assert!(near(message.x + run(message)[0], far_side), "{items:#?}");
}

#[test]
fn an_edge_label_on_its_route_is_the_connector_s_and_one_beside_it_a_text() {
    // Down the page, a label that starts left of the line and runs across it; across it, one
    // that sits above the line.
    let items = converted(
        r##"<svg xmlns="http://www.w3.org/2000/svg" font-size="16" fill="#1f1f1f">
                 <g class="pele-edges" fill="none" stroke="#1f1f1f">
                   <g class="pele-edge" data-id="down"><path d="M50,0V100"/></g>
                   <g class="pele-edge" data-id="across"><path d="M100,50H200"/></g>
                 </g>
                 <g class="pele-edge-labels">
                   <g class="pele-edge-label" data-id="down"><text x="33" y="55.6">abcd</text></g>
                   <g class="pele-edge-label" data-id="across">
                     <text x="150" y="30" text-anchor="middle">efgh</text>
                   </g>
                 </g>
               </svg>"##,
    );
    let kinds: Vec<&str> = items.iter().map(|item| item.kind.as_str()).collect();
    assert_eq!(kinds, ["line", "line", "text"]);
    assert_eq!(label(&items[0]), Some("abcd"));
    assert_eq!(label(&items[1]), None);
    assert_eq!(items[2].text.as_deref(), Some("efgh"));
}
