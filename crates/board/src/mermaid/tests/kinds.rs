//! Every type Pele draws comes onto the board in its way: as shapes, on a picture, or as one.

use roxmltree::Document;

use super::*;

#[test]
fn flowchart_gives_nodes_and_bound_arrows() {
    let items = skeletons(FLOW);
    // Three nodes and two arrows, and nothing else: neither the page behind the diagram nor
    // what hides the line behind the edge label is a shape of it.
    assert_eq!(items.len(), 5, "{items:#?}");
    let nodes: Vec<&Skeleton> = items.iter().filter(|item| item.kind != "arrow").collect();
    let arrows = of_kind(&items, "arrow");
    let kinds: Vec<&str> = nodes.iter().map(|node| node.kind.as_str()).collect();
    assert_eq!(kinds, ["rectangle", "diamond", "rectangle"]);
    let labels: Vec<Option<&str>> = nodes.iter().map(|node| label(node)).collect();
    assert_eq!(labels, [Some("Start"), Some("Ok?"), Some("Done")]);

    // Each arrow joins two of the nodes, by the ids of their skeletons.
    let joined: Vec<(&str, &str)> = arrows
        .iter()
        .map(|arrow| (joins(&items, &arrow.start), joins(&items, &arrow.end)))
        .collect();
    assert_eq!(joined, [("Start", "Ok?"), ("Ok?", "Done")]);
    let arrow_labels: Vec<Option<&str>> = arrows.iter().map(|arrow| label(arrow)).collect();
    assert_eq!(arrow_labels, [None, Some("yes")]);
}

#[test]
fn sequence_is_editable() {
    let items = skeletons(SEQUENCE);
    assert!(of_kind(&items, "rectangle").len() >= 2, "{items:#?}");
    assert!(!of_kind(&items, "arrow").is_empty(), "{items:#?}");
    assert!(of_kind(&items, "image").is_empty(), "{items:#?}");
}

#[test]
fn pie_comes_in_as_svg_image() {
    let diagram = render(PIE, &Palette::default()).unwrap();
    assert_eq!(diagram.kind, "pie");
    let items = to_skeletons(&diagram).unwrap();
    assert_eq!(items.len(), 1, "{items:#?}");
    let picture = &items[0];
    assert_eq!(picture.kind, "image");
    let svg = picture.svg.as_deref().expect("the diagram's SVG");
    assert!(svg.starts_with("<svg"), "{svg}");
    assert_eq!(picture.name.as_deref(), Some("diagram.svg"));
    assert_eq!((picture.x, picture.y), (0.0, 0.0));
    assert_eq!(
        (picture.width, picture.height),
        (Some(diagram.width), Some(diagram.height))
    );
}

/// A diagram of every type that comes in as shapes.
const EVERY_KIND: &[(&str, &str)] = &[
    (
        "flowchart",
        "flowchart TD\n  A([Start]) --> B{Ok?}\n  B -->|yes| C[(Store)]\n  B -.-> D{{Retry}}\n  subgraph S [Group]\n    D --> E((End))\n  end\n",
    ),
    (
        "sequence",
        "sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi\n  Alice->>Alice: Think\n  Note right of Bob: A note\n  Alice-xBob: Lost\n",
    ),
    (
        "class",
        "classDiagram\n  class Animal {\n    +String name\n    +eat() void\n  }\n  Animal <|-- Dog\n  Dog *-- Tail\n  Dog o-- Toy\n",
    ),
    (
        "state",
        "stateDiagram-v2\n  [*] --> Still\n  Still --> Moving : push\n  Moving --> [*]\n",
    ),
    (
        "er",
        "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n",
    ),
    (
        "mindmap",
        "mindmap\n  root((Root))\n    Branch\n      Leaf\n    Other\n",
    ),
    (
        "requirement",
        "requirementDiagram\n  requirement need {\n    id: 1\n    text: the need.\n    risk: high\n    verifymethod: test\n  }\n  element thing {\n    type: simulation\n  }\n  thing - satisfies -> need\n",
    ),
    (
        "block",
        "block-beta\n  columns 2\n  a[\"A\"] b[\"B\"]\n  a --> b\n",
    ),
    (
        "kanban",
        "kanban\n  Todo\n    [Write]\n  Done\n    [Read]\n",
    ),
    (
        "c4",
        "C4Context\n  title Context\n  Person(customer, \"Customer\", \"A customer\")\n  System(bank, \"Bank\", \"The bank\")\n  Rel(customer, bank, \"Uses\")\n",
    ),
    (
        "gitGraph",
        "gitGraph\n  commit\n  branch dev\n  commit\n  checkout main\n  merge dev\n",
    ),
    (
        "timeline",
        "timeline\n  title History\n  2020 : One\n  2021 : Two : Three\n",
    ),
];

#[test]
fn every_editable_kind_comes_in_as_shapes() {
    for (kind, source) in EVERY_KIND {
        assert_eq!(way_of(kind), Way::Shapes, "{kind}");
        let diagram = render(source, &Palette::default()).unwrap();
        assert_eq!(diagram.kind, *kind);
        let items = to_skeletons(&diagram).unwrap();
        assert!(!items.is_empty(), "{kind}");
        // No picture of the diagram: only of a text Pele turned (a git graph's commit ids).
        let pictures = of_kind(&items, "image");
        assert!(
            pictures
                .iter()
                .all(|p| p.name.as_deref() == Some("text.svg")),
            "{kind}: {items:#?}"
        );
        // The engine takes every one of them: the op answers, and draws something.
        let mut board = board();
        draw(&mut board, json!({"op": "mermaid", "source": source}));
    }
}

/// The fixture for `keyword`, one of `sawit_core::mermaid::TYPES`.
fn fixture(keyword: &str) -> String {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mermaid/types")
        .join(format!("{keyword}.mmd"));
    std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("{} can't be read: {error}", file.display()))
}

/// The pictures cut from a diagram come in as groups of their layers, not frames: a frame's
/// title, its file's name, showed over every icon of an architecture and every turned title of
/// a swimlane. A chart, one picture of the whole diagram, stays a frame, as any SVG an agent
/// adds does. Each picture is where Pele drew it.
#[test]
fn the_pictures_cut_from_a_diagram_are_groups() {
    use figcraft_doc::NodeKind;
    for keyword in ["architecture-beta", "swimlane-beta", "venn-beta"] {
        let mut board = board();
        let ids = draw(
            &mut board,
            json!({"op": "mermaid", "source": fixture(keyword)}),
        );
        let doc = board.doc().unwrap();
        // A board's shape is a frame too, but one with its board item; a picture's has none.
        let picture_frame = |id: &NodeId| {
            let node = doc.node(*id).unwrap();
            matches!(node.kind, NodeKind::Frame) && node.board.is_none()
        };
        let group = |id: &NodeId| matches!(doc.node(*id).unwrap().kind, NodeKind::Group);
        assert!(!ids.iter().any(picture_frame), "{keyword}");
        assert!(ids.iter().any(group), "{keyword}");
    }
    let mut board = board();
    let ids = draw(&mut board, json!({"op": "mermaid", "source": PIE}));
    let doc = board.doc().unwrap();
    let chart = doc.node(ids[0]).unwrap();
    assert!(matches!(chart.kind, NodeKind::Frame) && chart.board.is_none());
}

/// How long a big flowchart takes to render, to become skeletons and to be drawn:
/// `cargo test -p sawit-board --release -- --ignored --nocapture a_big_diagram_takes`.
#[test]
#[ignore]
fn a_big_diagram_takes() {
    for edges in [500u32, 1000, 2000] {
        let nodes = edges.div_ceil(2);
        let mut source = String::from("flowchart LR\n");
        for i in 0..edges {
            source.push_str(&format!("  n{} --> n{}\n", i % nodes, (i * 7 + 3) % nodes));
        }
        let started = std::time::Instant::now();
        let diagram = render(&source, &Palette::default()).unwrap();
        let rendered = started.elapsed();
        let items = to_skeletons(&diagram).unwrap();
        let converted = started.elapsed() - rendered;
        let mut board = board();
        let applied = crate::skeleton::apply(&mut board, &items, (0.0, 0.0)).unwrap();
        let drawn = started.elapsed() - rendered - converted;
        println!(
            "{edges} edges, {} skeletons, {} layers: render {rendered:?}, convert \
                 {converted:?}, draw {drawn:?}",
            items.len(),
            applied.ids.len()
        );
    }
}

/// Every type Pele draws has its way onto the board, and every type there is one Pele draws:
/// a type a newer Pele adds has to be placed, and the module's table with it.
#[test]
fn every_type_has_its_way_onto_the_board() {
    let mut drawn: Vec<&str> = sawit_core::mermaid::TYPES.iter().map(|(_, k)| *k).collect();
    drawn.sort_unstable();
    drawn.dedup();
    let mut placed: Vec<&str> = WAYS.iter().map(|(kind, _)| *kind).collect();
    placed.sort_unstable();
    assert_eq!(drawn, placed);
    let doc = include_str!("../mod.rs");
    for (kind, way) in WAYS {
        let row = doc.lines().find(|line| {
            line.starts_with("//! | ")
                && line
                    .split('|')
                    .nth(1)
                    .is_some_and(|types| types.split(',').any(|t| t.trim() == *kind))
        });
        let row = row.unwrap_or_else(|| panic!("the module's table has no row for {kind}"));
        let said = match way {
            Way::Shapes => "| shapes |",
            Way::ShapesOnPicture => "| shapes on a picture |",
            Way::Picture => "| one picture |",
        };
        assert!(row.contains(said), "{kind} is {way:?}: {row}");
    }
    for (kind, _) in BACKDROPS {
        assert_eq!(way_of(kind), Way::ShapesOnPicture, "{kind}");
    }
}

/// Every diagram type Pele draws, in each of its spellings (each C4 and railroad variant, each
/// `-beta`), renders, comes onto the board and is sane: shapes when its type is [`editable`]
/// (on one picture for a type in [`BACKDROPS`]), else one picture; something drawn, nothing far
/// outside the box Pele drew it in, and the op answers with what it added and nothing it
/// couldn't draw.
///
/// `SAWIT_DIAGRAM_PNG=dir cargo test -p sawit-board every_type` also writes each board as PNG,
/// beside Pele's own drawing as SVG, for a look.
#[test]
fn every_type_comes_onto_the_board() {
    let png_dir = std::env::var_os("SAWIT_DIAGRAM_PNG").map(std::path::PathBuf::from);
    if let Some(dir) = &png_dir {
        std::fs::create_dir_all(dir).unwrap();
        // The PNG's text is set in Inter, as the app sets it.
        crate::fonts::register();
    }
    for (keyword, kind) in sawit_core::mermaid::TYPES {
        let source = fixture(keyword);
        let diagram = render(&source, &Palette::default())
            .unwrap_or_else(|error| panic!("{keyword}: {error}"));
        assert_eq!(diagram.kind, *kind, "{keyword}");
        let items = to_skeletons(&diagram).unwrap_or_else(|e| panic!("{keyword}: {e}"));
        assert!(!items.is_empty(), "{keyword}");
        let pictures: Vec<&Skeleton> = of_kind(&items, "image");
        match way_of(kind) {
            Way::Picture => {
                assert_eq!(items.len(), 1, "{keyword}");
                assert_eq!(items[0].kind, "image", "{keyword}");
            }
            way => {
                // Shapes, and pictures only for what no shape draws: the one at the back, the
                // icons, the turned texts.
                assert!(items.len() > 1, "{keyword}: {items:#?}");
                assert!(pictures.len() < items.len() / 2, "{keyword}: {items:#?}");
                let backdrop = pictures
                    .iter()
                    .filter(|p| p.name.as_deref() == Some("backdrop.svg"))
                    .count();
                let expected = usize::from(way == Way::ShapesOnPicture);
                assert_eq!(backdrop, expected, "{keyword}");
                if way == Way::ShapesOnPicture {
                    // At the back.
                    assert_eq!(items[0].name.as_deref(), Some("backdrop.svg"), "{keyword}");
                }
            }
        }
        // Each picture cut from the diagram is an SVG of its own that reads.
        for picture in &pictures {
            let svg = picture.svg.as_deref().expect("an SVG picture");
            Document::parse(svg).unwrap_or_else(|e| panic!("{keyword}: {e}\n{svg}"));
        }
        // Within the box Pele drew, give or take what a text's width is guessed at.
        let (x0, y0, x1, y1) = crate::skeleton::bounds(&items).expect("bounds");
        let slack = 120.0;
        assert!(
            x0 >= -slack
                && y0 >= -slack
                && x1 <= diagram.width + slack
                && y1 <= diagram.height + slack,
            "{keyword}: ({x0}, {y0}, {x1}, {y1}) is outside {}x{}",
            diagram.width,
            diagram.height
        );
        let mut board = board();
        let reply = ops::run(&mut board, &json!({"op": "mermaid", "source": source}));
        assert_eq!(reply["ok"], json!(true), "{keyword}: {reply}");
        assert!(reply.get("notes").is_none(), "{keyword}: {reply}");
        let added = reply["added"].as_u64().unwrap_or_default();
        assert!(added > 0, "{keyword}: {reply}");
        if let Some(dir) = &png_dir {
            let png = ops::export_page(&mut board, "png", 1.0).unwrap();
            std::fs::write(dir.join(format!("{keyword}.board.png")), png).unwrap();
            std::fs::write(dir.join(format!("{keyword}.pele.svg")), &diagram.svg).unwrap();
        }
    }
}
