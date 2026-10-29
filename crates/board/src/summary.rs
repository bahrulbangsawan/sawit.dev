//! What an agent reads of the canvas: one summary for each top-level layer of the page on show
//! (the `get` op of the agent wire contract), and the name of each layer's type.
//!
//! A summary is `{id, type, x, y, width, height, text?, color?, from?, to?, name?}`:
//! - `id` is the layer's FigCraft node id as a string, and the box is its bounds in page
//!   coordinates;
//! - `text` is what a sticky, a shape, a text layer or a connector's label says, when it says
//!   something;
//! - `color` is the colour the layer is drawn in, as `#RRGGBB` (`#RRGGBBAA` with some alpha): its
//!   first solid fill, else its first solid stroke (a connector or ink has only a stroke, and a
//!   shape with no fill is seen by its outline);
//! - `from` and `to` are the node ids a connector's two ends join; a free end has none;
//! - `name` is the layer's name, a section's title among them.
//!
//! A connector's label is a layer of the page of its own. An agent doesn't see it: its text is the
//! connector's `text`, and it is edited and deleted through the connector.

use figcraft_color::{Color, Paint, PaintKind};
use figcraft_doc::{BoardItem, Document, End, Node, NodeId, NodeKind};
use figcraft_engine::board::{label_of, text_of};
use serde_json::{Value, json};

/// The summaries of the top-level layers of `page`, back to front. A page that isn't there has
/// none.
pub fn summarize(doc: &Document, page: NodeId) -> Vec<Value> {
    let Some(page) = doc.node(page) else {
        return Vec::new();
    };
    page.children
        .iter()
        .filter_map(|&id| summary(doc, id))
        .collect()
}

/// The type name of layer `id` in a summary: one of `rectangle`, `roundedRectangle`, `ellipse`,
/// `diamond`, `triangle`, `speechBubble`, `chevron`, `sticky`, `connector`, `ink`, `stamp`, `text`,
/// `section`, `frame`, `image`, `vector`, `group`, `table` and `other`.
///
/// A board object is its own type, a shape by its outline. Of the layers of a design file, a
/// rectangle with an image fill is an `image` (there is no layer kind for one at this FigCraft),
/// a component or an instance is a `frame`, a line or a boolean group a `vector`. What has no
/// type of its own (a polygon, a star, a label, a layer that isn't there) is `other`.
pub fn element_type(doc: &Document, id: NodeId) -> &'static str {
    let Some(node) = doc.node(id) else {
        return "other";
    };
    match &node.board {
        Some(BoardItem::Shape { shape }) => return shape.id(),
        Some(BoardItem::Sticky { .. }) => return "sticky",
        Some(BoardItem::Connector(_)) => return "connector",
        Some(BoardItem::Ink) => return "ink",
        Some(BoardItem::Stamp { .. }) => return "stamp",
        Some(BoardItem::Table) => return "table",
        Some(BoardItem::Label { .. }) => return "other",
        None => {}
    }
    match &node.kind {
        NodeKind::Text(_) => "text",
        NodeKind::Section { .. } => "section",
        NodeKind::Frame
        | NodeKind::Component { .. }
        | NodeKind::ComponentSet { .. }
        | NodeKind::Instance { .. } => "frame",
        NodeKind::Group => "group",
        NodeKind::Rectangle if node.fills.iter().any(is_image) => "image",
        NodeKind::Rectangle => "rectangle",
        NodeKind::Ellipse { .. } => "ellipse",
        NodeKind::Vector { .. } | NodeKind::BooleanOp { .. } | NodeKind::Line => "vector",
        NodeKind::Page { .. }
        | NodeKind::Polygon { .. }
        | NodeKind::Star { .. }
        | NodeKind::Slice => "other",
    }
}

/// The text layer that holds the text of layer `id`: the layer itself when it is text, the text of
/// a connector's label, else the note of a sticky or the text of a shape. `None` for what has no
/// text.
pub(crate) fn text_layer(doc: &Document, id: NodeId) -> Option<NodeId> {
    let node = doc.node(id)?;
    if matches!(node.kind, NodeKind::Text(_)) {
        return Some(id);
    }
    match node.board {
        Some(BoardItem::Connector(_)) => text_of(doc, label_of(doc, id)?),
        _ => text_of(doc, id),
    }
}

fn is_image(paint: &Paint) -> bool {
    matches!(paint.kind, PaintKind::Image(_))
}

/// The summary of layer `id`, or `None` for a label, which is no element of its own.
fn summary(doc: &Document, id: NodeId) -> Option<Value> {
    let node = doc.node(id)?;
    if matches!(node.board, Some(BoardItem::Label { .. })) {
        return None;
    }
    let bounds = doc.world_bounds(id)?;
    let mut out = json!({
        "id": id.to_string(),
        "type": element_type(doc, id),
        "x": bounds.x0,
        "y": bounds.y0,
        "width": bounds.width(),
        "height": bounds.height(),
    });
    let text = text_layer(doc, id)
        .and_then(|layer| doc.node(layer)?.kind.text())
        .map(|text| text.characters.as_str())
        .filter(|text| !text.is_empty());
    if let Some(text) = text {
        out["text"] = json!(text);
    }
    if let Some(color) = color(doc, node) {
        out["color"] = json!(color.to_css_hex());
    }
    if let Some(BoardItem::Connector(connector)) = &node.board {
        for (key, end) in [("from", connector.from), ("to", connector.to)] {
            if let End::Node { node, .. } = end {
                out[key] = json!(node.to_string());
            }
        }
    }
    if !node.name.is_empty() {
        out["name"] = json!(node.name);
    }
    Some(out)
}

/// The colour `node` is drawn in: its first solid fill, else its first solid stroke. A shape with
/// text draws with its outline, a layer of its own.
fn color(doc: &Document, node: &Node) -> Option<Color> {
    let drawn = match node.board {
        Some(BoardItem::Shape { .. }) => node
            .children
            .first()
            .and_then(|outline| doc.node(*outline))
            .unwrap_or(node),
        _ => node,
    };
    solid(&drawn.fills).or_else(|| solid(&drawn.strokes))
}

/// The first solid paint of `paints` that shows, as the colour it makes.
fn solid(paints: &[Paint]) -> Option<Color> {
    paints
        .iter()
        .filter(|paint| paint.visible)
        .find_map(Paint::solid_color)
        .filter(|color| color.a > 0.0)
}

#[cfg(test)]
mod tests {
    use figcraft_doc::{BoardItem, Document};
    use serde_json::json;

    use super::*;
    use crate::doc::BoardDoc;
    use crate::skeleton::{self, Skeleton};

    // A 1x1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// An empty design file, which has frames and rectangles rather than board objects.
    fn design() -> BoardDoc {
        let bytes = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
        BoardDoc::from_bytes(&bytes).unwrap()
    }

    /// Run a command that makes a layer, and name the layer.
    fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
        let made = board.execute(command, &params).unwrap();
        NodeId(made["id"].as_u64().expect("a command that makes a layer"))
    }

    /// Draw skeletons as `add` does, and name what came out, in input order.
    fn add(board: &mut BoardDoc, items: Value) -> Vec<NodeId> {
        let items: Vec<Skeleton> = serde_json::from_value(items).unwrap();
        let applied = skeleton::apply(board, &items, (0.0, 0.0)).unwrap();
        assert!(applied.notes.is_empty(), "{:?}", applied.notes);
        applied.ids.iter().map(|id| id.parse().unwrap()).collect()
    }

    /// The summaries of the page on show.
    fn summaries(board: &BoardDoc) -> Vec<Value> {
        let ds = board.session.ds().unwrap();
        summarize(&ds.doc, ds.page)
    }

    /// The summary of layer `id`.
    fn summary_of(board: &BoardDoc, id: NodeId) -> Value {
        let id = id.to_string();
        summaries(board)
            .into_iter()
            .find(|s| s["id"] == json!(id))
            .unwrap_or_else(|| panic!("{id} is not on the page"))
    }

    fn type_of(board: &BoardDoc, id: NodeId) -> &'static str {
        element_type(&board.doc().unwrap(), id)
    }

    #[test]
    fn every_kind_of_board_object_has_its_type() {
        let mut b = board();
        let mut expected = Vec::new();
        for (n, shape) in [
            "rectangle",
            "roundedRectangle",
            "ellipse",
            "diamond",
            "triangle",
            "speechBubble",
            "chevron",
        ]
        .into_iter()
        .enumerate()
        {
            let x = 200 * n;
            let id = make(
                &mut b,
                "board.shape",
                json!({"shape": shape, "x": x, "y": 0}),
            );
            expected.push((id, shape));
        }
        let sticky = make(&mut b, "board.sticky", json!({"x": 0, "y": 300}));
        let (first, last) = (expected[0].0, expected[6].0);
        let connector = make(
            &mut b,
            "board.connector",
            json!({"from": first, "to": last}),
        );
        let ink = make(&mut b, "board.ink", json!({"points": [[0, 0], [50, 50]]}));
        let stamp = make(
            &mut b,
            "board.stamp",
            json!({"x": 0, "y": 0, "stamp": "star"}),
        );
        let table = make(
            &mut b,
            "board.table",
            json!({"x": 0, "y": 600, "rows": 2, "columns": 2}),
        );
        let text = make(
            &mut b,
            "node.create",
            json!({"type": "text", "x": 0, "y": 0, "text": "t"}),
        );
        let section = make(
            &mut b,
            "node.create",
            json!({"type": "section", "x": 0, "y": 900}),
        );
        let image = make(&mut b, "image.place", json!({"data": PNG, "x": 0, "y": 0}));
        // A rectangle of a design file's kind, not a board shape with text, is a rectangle too.
        let plain = make(
            &mut b,
            "node.create",
            json!({"type": "rectangle", "x": 0, "y": 0}),
        );
        expected.extend([
            (sticky, "sticky"),
            (connector, "connector"),
            (ink, "ink"),
            (stamp, "stamp"),
            (table, "table"),
            (text, "text"),
            (section, "section"),
            (image, "image"),
            (plain, "rectangle"),
        ]);

        for (id, name) in expected {
            assert_eq!(type_of(&b, id), name, "{id}");
        }
    }

    #[test]
    fn a_design_files_layers_have_types_too() {
        let mut d = design();
        let frame = make(
            &mut d,
            "node.create",
            json!({"type": "frame", "x": 0, "y": 0}),
        );
        let rectangle = make(
            &mut d,
            "node.create",
            json!({"type": "rectangle", "x": 0, "y": 0}),
        );
        let ellipse = make(
            &mut d,
            "node.create",
            json!({"type": "ellipse", "x": 0, "y": 0}),
        );
        let vector = make(
            &mut d,
            "node.create",
            json!({"type": "vector", "points": [[0, 0], [10, 10]]}),
        );
        let line = make(
            &mut d,
            "node.create",
            json!({"type": "line", "x": 0, "y": 0}),
        );
        let star = make(
            &mut d,
            "node.create",
            json!({"type": "star", "x": 0, "y": 0}),
        );
        let image = make(&mut d, "image.place", json!({"data": PNG, "x": 0, "y": 0}));
        let group = make(
            &mut d,
            "node.create",
            json!({"type": "rectangle", "x": 0, "y": 0}),
        );
        let other = make(
            &mut d,
            "node.create",
            json!({"type": "rectangle", "x": 5, "y": 5}),
        );
        let made = d
            .execute("object.group", &json!({"ids": [group, other]}))
            .unwrap();
        let group = NodeId(made["id"].as_u64().unwrap());

        for (id, name) in [
            (frame, "frame"),
            (rectangle, "rectangle"),
            (ellipse, "ellipse"),
            (vector, "vector"),
            (line, "vector"),
            (star, "other"),
            (image, "image"),
            (group, "group"),
        ] {
            assert_eq!(type_of(&d, id), name, "{id}");
        }
    }

    #[test]
    fn a_layer_that_is_not_there_is_other() {
        let b = board();
        assert_eq!(element_type(&b.doc().unwrap(), NodeId(9999)), "other");
    }

    #[test]
    fn layers_are_summarised_back_to_front_and_a_label_is_no_layer_of_its_own() {
        let mut b = board();
        let ids = add(
            &mut b,
            json!([
                {"type": "sticky", "id": "s", "x": 0, "y": 0, "text": "hi"},
                {"type": "diamond", "id": "d", "x": 400, "y": 0},
                {"type": "arrow", "start": {"id": "s"}, "end": {"id": "d"}, "label": {"text": "yes"}},
            ]),
        );
        // The label is a layer of the page, next to the connector it belongs to.
        let doc = b.doc().unwrap();
        let page = doc.first_page().unwrap();
        let layers = &doc.node(page).unwrap().children;
        assert_eq!(layers.len(), 4);
        assert!(
            layers
                .iter()
                .any(|id| matches!(doc.node(*id).unwrap().board, Some(BoardItem::Label { .. })))
        );

        let summaries = summaries(&b);
        let listed: Vec<&str> = summaries
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        let expected: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert_eq!(listed, expected);
        let types: Vec<&str> = summaries
            .iter()
            .map(|s| s["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["sticky", "diamond", "connector"]);
    }

    #[test]
    fn a_summary_has_the_specs_fields() {
        let mut b = board();
        let ids = add(
            &mut b,
            json!([
                {"type": "sticky", "id": "s", "x": 10, "y": 20, "text": "hi", "backgroundColor": "blue"},
                {"type": "ellipse", "id": "e", "x": 400, "y": 0, "width": 100, "height": 60,
                 "label": {"text": "yes"}},
                {"type": "arrow", "start": {"id": "s"}, "end": {"id": "e"}},
                {"type": "arrow", "x": 0, "y": 500, "width": 100, "height": 0},
                {"type": "frame", "x": 0, "y": 600, "width": 300, "height": 200, "name": "Box"},
                {"type": "text", "x": 0, "y": 900, "text": "plain", "strokeColor": "#ff0000"},
                {"type": "line", "x": 0, "y": 1000, "points": [[0, 0], [50, 50], [100, 0]]},
                {"type": "rectangle", "x": 0, "y": 1200, "width": 80, "height": 40},
            ]),
        );
        let (sticky, ellipse, bound, free, frame, text, ink, empty) = (
            summary_of(&b, ids[0]),
            summary_of(&b, ids[1]),
            summary_of(&b, ids[2]),
            summary_of(&b, ids[3]),
            summary_of(&b, ids[4]),
            summary_of(&b, ids[5]),
            summary_of(&b, ids[6]),
            summary_of(&b, ids[7]),
        );

        // The box is the layer's bounds in page coordinates.
        assert_eq!(sticky["id"], json!(ids[0].to_string()));
        assert_eq!(sticky["type"], json!("sticky"));
        assert_eq!(
            (sticky["x"].as_f64(), sticky["y"].as_f64()),
            (Some(10.0), Some(20.0))
        );
        assert_eq!(sticky["width"], json!(200.0));
        assert!(sticky["height"].as_f64().unwrap() >= 200.0);
        assert_eq!(sticky["text"], json!("hi"));
        assert_eq!(sticky["color"], json!("#ACD3FA"));
        assert_eq!(sticky["name"], json!("Sticky"));
        let bounds = b.doc().unwrap().world_bounds(ids[0]).unwrap();
        assert_eq!(sticky["height"], json!(bounds.height()));

        assert_eq!(ellipse["type"], json!("ellipse"));
        assert_eq!(
            (ellipse["width"].as_f64(), ellipse["height"].as_f64()),
            (Some(100.0), Some(60.0))
        );
        assert_eq!(ellipse["text"], json!("yes"));

        // A connector says what it joins, by node id.
        assert_eq!(bound["type"], json!("connector"));
        assert_eq!(bound["from"], json!(ids[0].to_string()));
        assert_eq!(bound["to"], json!(ids[1].to_string()));
        assert!(bound.get("text").is_none(), "{bound}");
        assert!(bound["color"].is_string());
        // A free end joins nothing, so it has no id to give.
        assert_eq!(free["type"], json!("connector"));
        assert!(
            free.get("from").is_none() && free.get("to").is_none(),
            "{free}"
        );

        assert_eq!(frame["type"], json!("section"));
        assert_eq!(frame["name"], json!("Box"));
        assert!(frame.get("text").is_none());
        assert_eq!(frame["width"], json!(300.0));

        // The colour of text is its fill, the one `strokeColor` sets (P7-C24).
        assert_eq!(text["type"], json!("text"));
        assert_eq!(text["text"], json!("plain"));
        assert_eq!(text["color"], json!("#FF0000"));

        assert_eq!(ink["type"], json!("ink"));
        assert_eq!(ink["color"], json!("#1E1E1E"));
        assert!(ink.get("from").is_none() && ink.get("text").is_none());

        // Text that is empty is not given at all.
        assert_eq!(empty["type"], json!("rectangle"));
        assert!(empty.get("text").is_none(), "{empty}");
    }

    #[test]
    fn a_connector_has_the_text_of_its_label() {
        let mut b = board();
        let ids = add(
            &mut b,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0},
                {"type": "rectangle", "id": "b", "x": 400, "y": 0},
                {"type": "arrow", "start": {"id": "a"}, "end": {"id": "b"}, "label": {"text": "yes"}},
            ]),
        );
        assert_eq!(summary_of(&b, ids[2])["text"], json!("yes"));
        assert_eq!(summaries(&b).len(), 3);

        // The label is edited through its connector, and gone with its text.
        b.execute("board.label", &json!({"id": ids[2], "text": "no"}))
            .unwrap();
        assert_eq!(summary_of(&b, ids[2])["text"], json!("no"));
        b.execute("board.label", &json!({"id": ids[2], "text": ""}))
            .unwrap();
        assert!(summary_of(&b, ids[2]).get("text").is_none());
        assert_eq!(summaries(&b).len(), 3);
    }

    #[test]
    fn the_colour_is_what_is_drawn() {
        let mut b = board();
        let ids = add(
            &mut b,
            json!([
                {"type": "rectangle", "x": 0, "y": 0},
                {"type": "rectangle", "x": 200, "y": 0, "backgroundColor": "#2f9e44"},
                // No fill: the colour is the outline's.
                {"type": "rectangle", "x": 400, "y": 0, "backgroundColor": "transparent",
                 "strokeColor": "#e03131"},
                {"type": "arrow", "x": 0, "y": 300, "width": 100, "height": 0, "strokeColor": "#1971c2"},
            ]),
        );
        let colour = |id| summary_of(&b, id)["color"].clone();
        // The sticky tool's colour, which a shape takes when it is given none.
        assert_eq!(colour(ids[0]), json!("#FFE78A"));
        assert_eq!(colour(ids[1]), json!("#2F9E44"));
        assert_eq!(colour(ids[2]), json!("#E03131"));
        assert_eq!(colour(ids[3]), json!("#1971C2"));

        // A layer with no solid paint at all has no colour.
        let shape = make(
            &mut b,
            "board.shape",
            json!({"shape": "ellipse", "x": 0, "y": 600}),
        );
        let outline = b.doc().unwrap().node(shape).unwrap().children[0];
        b.execute(
            "node.set",
            &json!({"ids": [outline], "props": {"fill": "none", "stroke": "none"}}),
        )
        .unwrap();
        assert!(summary_of(&b, shape).get("color").is_none());
        // And a colour with some alpha is written with it.
        b.execute(
            "node.set",
            &json!({"ids": [outline], "props": {"fill": "#ff000080"}}),
        )
        .unwrap();
        assert_eq!(summary_of(&b, shape)["color"], json!("#FF000080"));
    }

    #[test]
    fn a_page_that_is_not_there_has_nothing_to_summarise() {
        let b = board();
        assert!(summarize(&b.doc().unwrap(), NodeId(9999)).is_empty());
        // An empty page too.
        assert!(summaries(&b).is_empty());
    }
}
