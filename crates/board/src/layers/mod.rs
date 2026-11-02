//! The layers panel's model: the rows of the layer tree and of the page list, where a dragged layer
//! lands, and which layers to open so that a selection made on the canvas shows. The view in
//! `crates/app/src/board/layers_panel/` draws what [`rows`] and [`pages`] return, runs
//! `node.reparent` with what [`drop_target`] returns, and keeps the set of open layers that
//! [`rows`] reads and [`reveal`] adds to. It ports the decisions of FigCraft's own panel,
//! `ui-egui/src/panels/layers.rs` (`collect`, `drop_zone`, `drop_layers`, `reveal_selection`),
//! with these differences:
//!
//! - A filter lists the layers that match under the layers that hold them, open, where FigCraft
//!   lists the matches as a flat list.
//! - A board item (a sticky, a shape, a table…) is one row: the text and outline inside it are
//!   never rows.
//! - A drop is checked here, against the document's rules (no cycles, containers only, sections
//!   only on pages and in sections) and the engine's rule that nothing goes into an instance, so
//!   the view doesn't offer a drop the engine would refuse for those reasons. A drop that would
//!   leave the layers where they are is no drop either, so it adds no empty undo step.
//! - [`rows`] walks the tree with a stack of its own, so a deep file can't overflow the thread's,
//!   and it cuts no depth off.
//!
//! Rows run from the top of the stacking order down, as Figma's panel shows them. A layer's
//! children are kept bottom to top, so they are listed in reverse.

use std::collections::HashSet;

use figcraft_color::PaintKind;
use figcraft_doc::{BoardItem, Document, Node, NodeId, NodeKind};

mod drop;
#[cfg(test)]
mod tests;

pub use drop::{DropPos, Reparent, drop_target};

/// The icon of a layer row: what the layer is, as the panel draws it. FigCraft's panel has an icon
/// for every kind; these are the ones Sawit's panel draws, and the kinds that have none share one:
/// polygons and stars show as [`LayerIcon::Vector`], boolean groups as [`LayerIcon::Group`],
/// component sets as [`LayerIcon::Component`], a rectangle with an image fill as
/// [`LayerIcon::Image`], and a connector's label as [`LayerIcon::Connector`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayerIcon {
    Frame,
    Section,
    Group,
    Component,
    Instance,
    Text,
    Rectangle,
    Ellipse,
    Vector,
    Line,
    Image,
    Sticky,
    Shape,
    Connector,
    Ink,
    Stamp,
    Table,
    /// Slices, and anything else the panel has no picture for.
    Other,
}

/// One row of the layer tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerRow {
    pub id: NodeId,
    /// How many layers hold this one, up to the page, which doesn't count: layers on the page
    /// itself are at depth 0.
    pub depth: usize,
    /// The layer's name, or its kind ("Frame") when it has none.
    pub name: String,
    pub icon: LayerIcon,
    /// The layer's own eye: false when it is hidden, whatever the layers holding it are.
    pub visible: bool,
    /// The layer's own lock, likewise.
    pub locked: bool,
    /// Whether this layer has layers to list under it, so it gets an expand arrow. Never for a board
    /// item: its text and outline are inside it, but it shows as one row. Under a filter, whether
    /// any of the layers found are inside it.
    pub has_children: bool,
    /// Whether the layers under this one are listed. Under a filter that is so for every layer
    /// that has any, whatever the set of open layers says.
    pub expanded: bool,
    pub selected: bool,
}

/// One row of the page list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRow {
    pub id: NodeId,
    pub name: String,
    /// The page that is open.
    pub current: bool,
    /// A line between pages: an empty page whose name starts with a hyphen. It opens nothing.
    pub divider: bool,
}

/// The rows of `page`'s layer tree: layers on the page at depth 0, then each open layer's own
/// below it, at one more.
///
/// `expanded` holds the layers that are open. `selection` marks rows. `filter` lists only the
/// layers with it in their name, ignoring case and the space around it, and the layers they are
/// in, which are shown open whether `expanded` has them or not; `expanded` is read and never
/// changed, so clearing the filter gives back the rows that were there. A filter with no match
/// lists nothing. Nothing is listed for a page that isn't there.
pub fn rows(
    doc: &Document,
    page: NodeId,
    expanded: &HashSet<NodeId>,
    selection: &[NodeId],
    filter: &str,
) -> Vec<LayerRow> {
    let Some(page_node) = doc.node(page) else {
        return Vec::new();
    };
    let query = filter.trim().to_lowercase();
    let found = (!query.is_empty()).then(|| found_with_holders(doc, page, &query));
    // Under a filter only what it found is listed.
    let listed = |id: &NodeId| found.as_ref().is_none_or(|found| found.contains(id));
    let selected: HashSet<NodeId> = selection.iter().copied().collect();

    let mut rows = Vec::new();
    // The layers still to list, with their depth. A layer's children go on bottom first, so the
    // top one comes off first, and each layer's rows follow it directly.
    let mut pending: Vec<(NodeId, usize)> = page_node
        .children
        .iter()
        .filter(|id| listed(id))
        .map(|id| (*id, 0))
        .collect();
    while let Some((id, depth)) = pending.pop() {
        let Some(node) = doc.node(id) else { continue };
        let inside: &[NodeId] = if node.board.is_some() {
            &[]
        } else {
            &node.children
        };
        let (has_children, open) = match &found {
            // Under a filter, a layer is open when something that was found is inside it.
            Some(found) => {
                let any = inside.iter().any(|child| found.contains(child));
                (any, any)
            }
            None => {
                let any = !inside.is_empty();
                (any, any && expanded.contains(&id))
            }
        };
        rows.push(LayerRow {
            id,
            depth,
            name: display_name(node).to_owned(),
            icon: icon_of(node),
            visible: node.visible,
            locked: node.locked,
            has_children,
            expanded: open,
            selected: selected.contains(&id),
        });
        if open {
            pending.extend(
                inside
                    .iter()
                    .filter(|child| listed(child))
                    .map(|child| (*child, depth + 1)),
            );
        }
    }
    rows
}

/// The rows of the page list, in the file's order. `current` is the open page.
pub fn pages(doc: &Document, current: NodeId) -> Vec<PageRow> {
    doc.pages
        .iter()
        .filter_map(|id| {
            let page = doc.node(*id)?;
            Some(PageRow {
                id: *id,
                name: page.name.clone(),
                current: *id == current,
                divider: doc.is_divider(*id),
            })
        })
        .collect()
}

/// The layers to open so that every one of `ids` has a row: all the layers holding any of them,
/// up to the page, which isn't a row. A board item isn't one of them: it can't be opened, so
/// whatever is inside it shows as the item. Layers that aren't there add nothing.
pub fn reveal(doc: &Document, ids: &[NodeId]) -> HashSet<NodeId> {
    let mut open = HashSet::new();
    for id in ids {
        for holder in doc.ancestors(*id) {
            if let Some(node) = doc.node(holder)
                && !node.is_page()
                && node.board.is_none()
            {
                open.insert(holder);
            }
        }
    }
    open
}

/// What a row calls its layer: the name, or the kind when it has no name worth showing.
fn display_name(node: &Node) -> &str {
    if node.name.trim().is_empty() {
        node.kind.label()
    } else {
        &node.name
    }
}

/// The icon of a layer: its board item's, else its kind's.
fn icon_of(node: &Node) -> LayerIcon {
    if let Some(item) = &node.board {
        return match item {
            BoardItem::Sticky { .. } => LayerIcon::Sticky,
            BoardItem::Shape { .. } => LayerIcon::Shape,
            BoardItem::Connector(_) | BoardItem::Label { .. } => LayerIcon::Connector,
            BoardItem::Stamp { .. } => LayerIcon::Stamp,
            BoardItem::Ink => LayerIcon::Ink,
            BoardItem::Table => LayerIcon::Table,
        };
    }
    match &node.kind {
        NodeKind::Frame => LayerIcon::Frame,
        NodeKind::Section { .. } => LayerIcon::Section,
        NodeKind::Group | NodeKind::BooleanOp { .. } => LayerIcon::Group,
        NodeKind::Component { .. } | NodeKind::ComponentSet { .. } => LayerIcon::Component,
        NodeKind::Instance { .. } => LayerIcon::Instance,
        NodeKind::Text(_) => LayerIcon::Text,
        // FigCraft has no image layer: a picture is a rectangle with an image fill.
        NodeKind::Rectangle
            if node
                .fills
                .iter()
                .any(|fill| matches!(fill.kind, PaintKind::Image(_))) =>
        {
            LayerIcon::Image
        }
        NodeKind::Rectangle => LayerIcon::Rectangle,
        NodeKind::Ellipse { .. } => LayerIcon::Ellipse,
        NodeKind::Polygon { .. } | NodeKind::Star { .. } | NodeKind::Vector { .. } => {
            LayerIcon::Vector
        }
        NodeKind::Line => LayerIcon::Line,
        NodeKind::Page { .. } | NodeKind::Slice => LayerIcon::Other,
    }
}

/// The layers of `page` a filter lists: those with `query` (lowercase) in their name, and every
/// layer holding one, up to the page. What is inside a board item is never looked at.
fn found_with_holders(doc: &Document, page: NodeId, query: &str) -> HashSet<NodeId> {
    let mut found = HashSet::new();
    let mut pending: Vec<NodeId> = doc
        .node(page)
        .map(|page| page.children.clone())
        .unwrap_or_default();
    while let Some(id) = pending.pop() {
        let Some(node) = doc.node(id) else { continue };
        if display_name(node).to_lowercase().contains(query) {
            // The layer and the ones holding it, up to the first that is in already: it was
            // added with everything above it.
            let mut up = Some(id);
            while let Some(at) = up.filter(|at| *at != page) {
                if !found.insert(at) {
                    break;
                }
                up = doc.node(at).and_then(|node| node.parent);
            }
        }
        if node.board.is_none() {
            pending.extend(node.children.iter().copied());
        }
    }
    found
}
