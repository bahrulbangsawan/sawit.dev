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
mod tests;
