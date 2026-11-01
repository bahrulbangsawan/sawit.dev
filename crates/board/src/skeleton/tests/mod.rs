//! The skeletons' tests: those the brief asks for, then by topic what the rulings and spec add.

// The tests the brief asks for.
mod brief;
// What the rulings and the spec add.
mod calls;
mod colours;
mod elements;
mod placement;

use figcraft_color::{Color, Paint};
use figcraft_doc::{BOARD_COLORS, BoardItem, BoardShape, Connector, Routing};
use serde_json::{Value, json};

use super::*;

// A 1x1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn board() -> BoardDoc {
    BoardDoc::new_board("Canvas").unwrap()
}

fn skeletons(items: Value) -> Vec<Skeleton> {
    serde_json::from_value(items).unwrap()
}

/// Apply `items` as they are, without an offset.
fn add(board: &mut BoardDoc, items: Value) -> Applied {
    apply(board, &skeletons(items), (0.0, 0.0)).unwrap()
}

/// The layers on the page, bottom first.
fn layers(board: &BoardDoc) -> Vec<NodeId> {
    let doc = board.doc().unwrap();
    let page = doc.first_page().unwrap();
    doc.node(page).unwrap().children.clone()
}

/// The node of the `index`th id in `applied`.
fn node(applied: &Applied, index: usize) -> NodeId {
    applied.ids[index].parse().unwrap()
}

fn board_item(board: &BoardDoc, id: NodeId) -> BoardItem {
    let doc = board.doc().unwrap();
    doc.node(id).unwrap().board.clone().unwrap()
}

fn connector(board: &BoardDoc, id: NodeId) -> Connector {
    match board_item(board, id) {
        BoardItem::Connector(c) => c,
        other => panic!("not a connector: {other:?}"),
    }
}

fn shape(board: &BoardDoc, id: NodeId) -> BoardShape {
    match board_item(board, id) {
        BoardItem::Shape { shape } => shape,
        other => panic!("not a shape: {other:?}"),
    }
}

/// The text of a shape or a sticky.
fn text_in(board: &BoardDoc, id: NodeId) -> String {
    let doc = board.doc().unwrap();
    let text = figcraft_engine::board::text_of(&doc, id).expect("an object with text");
    doc.node(text)
        .unwrap()
        .kind
        .text()
        .unwrap()
        .characters
        .clone()
}

/// The first solid fill of layer `id`.
fn fill(board: &BoardDoc, id: NodeId) -> Option<Color> {
    let doc = board.doc().unwrap();
    let n = doc.node(id).unwrap();
    n.fills.first().and_then(Paint::solid_color)
}

fn stroke(board: &BoardDoc, id: NodeId) -> Option<Color> {
    let doc = board.doc().unwrap();
    let n = doc.node(id).unwrap();
    n.strokes.first().and_then(Paint::solid_color)
}

fn css(colour: &str) -> Option<Color> {
    Color::parse_css(colour)
}

fn palette(name: &str) -> Color {
    BOARD_COLORS.iter().find(|(n, _)| *n == name).unwrap().1
}

/// The outline a shape with text is drawn by: its first child.
fn outline(board: &BoardDoc, id: NodeId) -> NodeId {
    board.doc().unwrap().node(id).unwrap().children[0]
}

fn bounds_of(board: &BoardDoc, id: NodeId) -> [f64; 4] {
    let r = board.doc().unwrap().world_bounds(id).unwrap();
    [r.x0, r.y0, r.x1, r.y1]
}

/// A board whose user has picked other tools than the defaults and has given a name, as in
/// the app: the toolbar's sticky colour, shape, connector style and marker, and Preferences.
fn board_with_other_choices() -> BoardDoc {
    let mut b = board();
    let tool = &mut b.session.tool.board;
    tool.sticky = palette("pink");
    tool.shape = BoardShape::Diamond;
    tool.routing = Routing::Curved;
    tool.marker = css("#e03131").unwrap();
    b.session.prefs.user_name = "Ada".into();
    b
}

/// Every connector on the page (not the labels, which are layers of their own).
fn connectors(board: &BoardDoc) -> Vec<Connector> {
    let doc = board.doc().unwrap();
    layers(board)
        .into_iter()
        .filter_map(|id| match doc.node(id).unwrap().board.clone() {
            Some(BoardItem::Connector(c)) => Some(c),
            _ => None,
        })
        .collect()
}

/// Both boards hold the same document: the same layers, each the same in every field.
fn assert_same_document(left: &BoardDoc, right: &BoardDoc) {
    let (left, right) = (left.doc().unwrap(), right.doc().unwrap());
    for (a, b) in left.nodes().zip(right.nodes()) {
        assert_eq!(a, b, "layer {} ({}) differs", a.id, a.name);
    }
    assert_eq!(left.nodes().count(), right.nodes().count(), "layers");
    assert!(
        *left == *right,
        "the layers are alike but the documents differ"
    );
}
