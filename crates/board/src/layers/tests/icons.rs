//! The icon of a row, for each kind of layer and each board item.

use figcraft_doc::{BoolOp, TextData, VectorNetwork};

use super::*;

#[test]
fn board_items_use_their_board_icon() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let sticky = make(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    let shape = make(
        &mut board,
        "board.shape",
        json!({"shape": "ellipse", "x": 300, "y": 0}),
    );
    let connector = make(
        &mut board,
        "board.connector",
        json!({"from": {"x": 0, "y": 400}, "to": {"x": 300, "y": 400}}),
    );
    let ink = make(
        &mut board,
        "board.ink",
        json!({"points": [[0, 600], [100, 650]]}),
    );
    let stamp = make(
        &mut board,
        "board.stamp",
        json!({"x": 500, "y": 0, "stamp": "heart"}),
    );
    let table = make(
        &mut board,
        "board.table",
        json!({"rows": 2, "columns": 2, "x": 0, "y": 800}),
    );
    let labelled = board
        .execute("board.label", &json!({"id": connector, "text": "yes"}))
        .unwrap();
    let label = id_of(&labelled["label"]).unwrap();
    let doc = board.doc().unwrap();
    let page = board.session.page().unwrap();
    let everything: HashSet<NodeId> = doc.ids().collect();
    let rows = rows(&doc, page, &everything, &[], "");

    let icon = |id| row(&rows, id).icon;
    assert_eq!(icon(sticky), LayerIcon::Sticky);
    assert_eq!(icon(shape), LayerIcon::Shape);
    assert_eq!(icon(connector), LayerIcon::Connector);
    assert_eq!(icon(ink), LayerIcon::Ink);
    assert_eq!(icon(stamp), LayerIcon::Stamp);
    assert_eq!(icon(table), LayerIcon::Table);
    // A connector's label is a layer of its own, drawn as the connector it belongs to.
    assert_eq!(icon(label), LayerIcon::Connector);
    // With everything open the rows are still the board's items and nothing from inside them.
    let top_level: Vec<NodeId> = doc
        .node(page)
        .unwrap()
        .children
        .iter()
        .rev()
        .copied()
        .collect();
    assert_eq!(ids(&rows), top_level);
    assert!(rows.iter().all(|row| row.depth == 0 && !row.has_children));
}

#[test]
fn icons_follow_the_kind() {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let main = add(&mut doc, page, component(), "Main");
    let image = {
        let id = add(&mut doc, page, NodeKind::Rectangle, "Photo");
        let node = doc.node_mut(id).unwrap();
        node.fills = vec![figcraft_color::Paint::image(
            "hash",
            figcraft_color::ImageScaleMode::Fill,
        )];
        id
    };
    let kinds = [
        (add(&mut doc, page, NodeKind::Frame, "x"), LayerIcon::Frame),
        (add(&mut doc, page, section(), "x"), LayerIcon::Section),
        (add(&mut doc, page, NodeKind::Group, "x"), LayerIcon::Group),
        (main, LayerIcon::Component),
        (
            add(
                &mut doc,
                page,
                NodeKind::Instance {
                    component: main,
                    props: BTreeMap::new(),
                },
                "x",
            ),
            LayerIcon::Instance,
        ),
        (
            add(&mut doc, page, NodeKind::Text(TextData::new("hi")), "x"),
            LayerIcon::Text,
        ),
        (
            add(&mut doc, page, NodeKind::Rectangle, "x"),
            LayerIcon::Rectangle,
        ),
        (
            add(
                &mut doc,
                page,
                NodeKind::Ellipse {
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                    inner: 0.0,
                },
                "x",
            ),
            LayerIcon::Ellipse,
        ),
        (
            add(
                &mut doc,
                page,
                NodeKind::Vector {
                    network: VectorNetwork::default(),
                },
                "x",
            ),
            LayerIcon::Vector,
        ),
        (add(&mut doc, page, NodeKind::Line, "x"), LayerIcon::Line),
        (image, LayerIcon::Image),
        // The kinds the panel has no icon of their own for.
        (
            add(&mut doc, page, NodeKind::Polygon { count: 5 }, "x"),
            LayerIcon::Vector,
        ),
        (
            add(
                &mut doc,
                page,
                NodeKind::Star {
                    count: 5,
                    inner_ratio: 0.4,
                },
                "x",
            ),
            LayerIcon::Vector,
        ),
        (
            add(
                &mut doc,
                page,
                NodeKind::BooleanOp { op: BoolOp::Union },
                "x",
            ),
            LayerIcon::Group,
        ),
        (
            add(
                &mut doc,
                page,
                NodeKind::ComponentSet {
                    description: String::new(),
                },
                "x",
            ),
            LayerIcon::Component,
        ),
        (add(&mut doc, page, NodeKind::Slice, "x"), LayerIcon::Other),
    ];
    let rows = rows(&doc, page, &HashSet::new(), &[], "");
    assert_eq!(rows.len(), kinds.len());
    for (id, icon) in kinds {
        let kind = doc.node(id).unwrap().kind.label();
        assert_eq!(row(&rows, id).icon, icon, "{kind}");
    }
}
