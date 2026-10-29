//! The layers panel's model: the rows of the layer tree and of the page list, where a dragged layer
//! lands, and which layers to open so that a selection made on the canvas shows. The view in
//! `crates/app/src/board/layers_panel.rs` draws what [`rows`] and [`pages`] return, runs
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
//!   the view doesn't offer a drop the engine would refuse for those reasons.
//! - [`rows`] walks the tree with a stack of its own, so a deep file can't overflow the thread's,
//!   and it cuts no depth off.
//!
//! Rows run from the top of the stacking order down, as Figma's panel shows them. A layer's
//! children are kept bottom to top, so they are listed in reverse.

use std::collections::HashSet;

use figcraft_color::PaintKind;
use figcraft_doc::{BoardItem, Document, Node, NodeId, NodeKind};
use figcraft_engine::components::structure_locked;

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

/// Where on a row a dragged layer is dropped. Rows list the top of the stacking order first, so
/// `Above` is on top of the row's layer and `Below` under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPos {
    Above,
    Below,
    /// Inside the row's layer, on top of what it holds.
    Into,
}

/// A move to run as `node.reparent {ids, parent, index}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reparent {
    pub ids: Vec<NodeId>,
    pub parent: NodeId,
    /// Where the first moved layer ends up among `parent`'s layers, counted from the bottom, as
    /// `node.reparent` takes it: among the layers that stay, once the moved ones have left. It is
    /// not a row, and not a position in `parent`'s layers as they are now.
    pub index: usize,
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

/// Where dragging `dragged` onto row `over` of `rows`, at `pos`, puts it, or `None` when nothing
/// may be dropped there. The view shows a drop line only for a `Some` and runs `node.reparent`
/// with it. `Above` and `Below` land next to the row's layer, in its parent; `Into` lands inside
/// it, on top.
///
/// Nothing is dropped:
/// - on a dragged layer or inside one, so a layer never goes into itself or into its own layers;
/// - into a layer that takes no layers (see `takes_layers`): one that is not a container, an
///   instance or anything inside one, where the engine refuses to add, remove or reorder layers,
///   or a board item such as a sticky, whose insides are not rows, so a layer dropped there would
///   vanish from the panel. A drop next to a layer is a drop into its parent, so the same goes
///   for that parent;
/// - into a parent that takes no sections, when a section is among the dragged layers: sections
///   go on the page or in other sections.
///
/// Dragged layers that are no longer there, and pages, are left out, and one that is listed twice
/// moves once. Which layers are dragged is for the view to say: FigCraft drags the whole selection
/// when the dragged row is in it.
///
/// Two things are left to the engine. A dragged layer that is inside an instance is not checked:
/// the engine refuses to move it and says why. And a drop that leaves the layers where they are is
/// not refused, so it runs `node.reparent` and records an undo step that changes nothing.
pub fn drop_target(
    doc: &Document,
    rows: &[LayerRow],
    dragged: &[NodeId],
    over: usize,
    pos: DropPos,
) -> Option<Reparent> {
    let over = doc.node(rows.get(over)?.id)?;

    let mut moving = HashSet::new();
    let ids: Vec<NodeId> = dragged
        .iter()
        .copied()
        .filter(|id| doc.node(*id).is_some_and(|node| !node.is_page()) && moving.insert(*id))
        .collect();
    if ids.is_empty() {
        return None;
    }
    // On a dragged layer, or inside one: the layer and everything holding it must be free.
    let held_by_dragged = std::iter::once(over.id)
        .chain(doc.ancestors(over.id))
        .any(|id| moving.contains(&id));
    if held_by_dragged {
        return None;
    }

    let parent = match pos {
        DropPos::Into => over,
        DropPos::Above | DropPos::Below => doc.node(over.parent?)?,
    };
    if !takes_layers(doc, parent) {
        return None;
    }
    let drags_a_section = ids.iter().any(|id| {
        doc.node(*id)
            .is_some_and(|node| matches!(node.kind, NodeKind::Section { .. }))
    });
    if drags_a_section && !doc.takes_sections(parent.id) {
        return None;
    }

    // The layers are listed from the bottom, and rows from the top, so a layer above a row in the
    // panel comes after it in `children`. `index` counts once the dragged layers have left, the
    // way `node.reparent` counts it: the gap is the place in today's children, less the dragged
    // layers below it. Dropped inside, they go on top of the rest.
    let siblings = &parent.children;
    let index = match pos {
        DropPos::Into => siblings.iter().filter(|id| !moving.contains(id)).count(),
        DropPos::Above | DropPos::Below => {
            let at = siblings.iter().position(|id| *id == over.id)?;
            let gap = if pos == DropPos::Above { at + 1 } else { at };
            gap - siblings
                .iter()
                .take(gap)
                .filter(|id| moving.contains(id))
                .count()
        }
    };
    Some(Reparent {
        ids,
        parent: parent.id,
        index,
    })
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

/// Whether layers can be dropped into `node`, and stay where they are put and in the panel: it is
/// a container (a page, a frame, a group, a section, a boolean group, a component or a component
/// set), not an instance, not a board item that holds its own insides and not inside an instance.
fn takes_layers(doc: &Document, node: &Node) -> bool {
    node.kind.is_container()
        && !matches!(node.kind, NodeKind::Instance { .. })
        && !node.is_board_atom()
        && !structure_locked(doc, node.id)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use figcraft_doc::{BoolOp, Document, NodeId, NodeKind, TextData, VectorNetwork};
    use figcraft_engine::Session;
    use figcraft_engine::params::id_of;
    use serde_json::{Value, json};

    use super::*;
    use crate::doc::BoardDoc;

    /// A design file holding `doc`.
    fn file(doc: Document) -> BoardDoc {
        BoardDoc {
            session: Session::with_document(doc),
        }
    }

    /// A 50 × 50 layer of `kind` called `name`, on top of `parent`'s layers.
    fn add(doc: &mut Document, parent: NodeId, kind: NodeKind, name: &str) -> NodeId {
        doc.create(parent, kind, name, 0.0, 0.0, 50.0, 50.0)
            .unwrap()
    }

    fn section() -> NodeKind {
        NodeKind::Section {
            contents_hidden: false,
        }
    }

    fn component() -> NodeKind {
        NodeKind::Component {
            description: String::new(),
            props: BTreeMap::new(),
        }
    }

    /// The design doc's tree on a design file's first page: frame F holds A, B and the group G,
    /// which holds C. G goes in first, so F's layers run G (the bottom), A, B (the top) and the
    /// panel lists B, A, G.
    struct Tree {
        board: BoardDoc,
        page: NodeId,
        f: NodeId,
        a: NodeId,
        b: NodeId,
        g: NodeId,
        c: NodeId,
    }

    fn tree() -> Tree {
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let f = add(&mut doc, page, NodeKind::Frame, "F");
        let g = add(&mut doc, f, NodeKind::Group, "G");
        let c = add(&mut doc, g, NodeKind::Rectangle, "C");
        let a = add(&mut doc, f, NodeKind::Rectangle, "A");
        let b = add(&mut doc, f, NodeKind::Rectangle, "B");
        Tree {
            board: file(doc),
            page,
            f,
            a,
            b,
            g,
            c,
        }
    }

    impl Tree {
        fn doc(&self) -> Arc<Document> {
            self.board.doc().unwrap()
        }

        /// The rows with `expanded` open and `selection` selected.
        fn rows(&self, expanded: &[NodeId], selection: &[NodeId], filter: &str) -> Vec<LayerRow> {
            let expanded: HashSet<NodeId> = expanded.iter().copied().collect();
            rows(&self.doc(), self.page, &expanded, selection, filter)
        }

        /// Every branch open.
        fn all_rows(&self) -> Vec<LayerRow> {
            self.rows(&[self.f, self.g], &[], "")
        }

        fn children(&self, id: NodeId) -> Vec<NodeId> {
            self.doc().node(id).unwrap().children.clone()
        }

        /// `drop_target` for what `rows` lists, dragging `dragged` over the row of `over`.
        fn target(
            &self,
            rows: &[LayerRow],
            dragged: &[NodeId],
            over: NodeId,
            pos: DropPos,
        ) -> Option<Reparent> {
            drop_on(&self.doc(), rows, dragged, over, pos)
        }

        /// Runs the move as the panel will.
        fn apply(&mut self, reparent: &Reparent) {
            apply(&mut self.board, reparent);
        }
    }

    fn drop_on(
        doc: &Document,
        rows: &[LayerRow],
        dragged: &[NodeId],
        over: NodeId,
        pos: DropPos,
    ) -> Option<Reparent> {
        let at = rows.iter().position(|row| row.id == over).unwrap();
        drop_target(doc, rows, dragged, at, pos)
    }

    fn apply(board: &mut BoardDoc, reparent: &Reparent) {
        let params = json!({
            "ids": reparent.ids,
            "parent": reparent.parent,
            "index": reparent.index,
        });
        board.execute("node.reparent", &params).unwrap();
    }

    fn ids(rows: &[LayerRow]) -> Vec<NodeId> {
        rows.iter().map(|row| row.id).collect()
    }

    fn outline(rows: &[LayerRow]) -> Vec<(NodeId, usize)> {
        rows.iter().map(|row| (row.id, row.depth)).collect()
    }

    fn set(ids: &[NodeId]) -> HashSet<NodeId> {
        ids.iter().copied().collect()
    }

    fn row(rows: &[LayerRow], id: NodeId) -> &LayerRow {
        rows.iter().find(|row| row.id == id).unwrap()
    }

    /// Runs `command` on `board` and returns the id it made.
    fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
        let made = board.execute(command, &params).unwrap();
        id_of(&made["id"]).unwrap()
    }

    #[test]
    fn rows_list_top_first_with_depth() {
        let t = tree();
        // The fixture's own z order, bottom first.
        assert_eq!(t.children(t.f), [t.g, t.a, t.b]);

        let rows = t.all_rows();
        assert_eq!(
            outline(&rows),
            [(t.f, 0), (t.b, 1), (t.a, 1), (t.g, 1), (t.c, 2)]
        );
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["F", "B", "A", "G", "C"]);
        let icons: Vec<LayerIcon> = rows.iter().map(|row| row.icon).collect();
        assert_eq!(
            icons,
            [
                LayerIcon::Frame,
                LayerIcon::Rectangle,
                LayerIcon::Rectangle,
                LayerIcon::Group,
                LayerIcon::Rectangle
            ]
        );
        // F and G hold layers and show them; the rest are leaves. Nothing is hidden, locked or
        // selected.
        let holds: Vec<(bool, bool)> = rows
            .iter()
            .map(|row| (row.has_children, row.expanded))
            .collect();
        assert_eq!(
            holds,
            [
                (true, true),
                (false, false),
                (false, false),
                (true, true),
                (false, false)
            ]
        );
        assert!(rows.iter().all(|row| row.visible));
        assert!(rows.iter().all(|row| !row.locked));
        assert!(rows.iter().all(|row| !row.selected));
    }

    #[test]
    fn collapsed_hides_children() {
        let t = tree();
        let rows = t.rows(&[], &[], "");
        assert_eq!(ids(&rows), [t.f]);
        assert!(rows[0].has_children);
        assert!(!rows[0].expanded);

        // F open, G still closed: C stays out, and G says it holds something.
        let rows = t.rows(&[t.f], &[], "");
        assert_eq!(ids(&rows), [t.f, t.b, t.a, t.g]);
        assert!(row(&rows, t.g).has_children);
        assert!(!row(&rows, t.g).expanded);
    }

    #[test]
    fn expanding_a_leaf_or_an_unlisted_layer_changes_nothing() {
        let t = tree();
        // A has no layers; C is under a closed G.
        let rows = t.rows(&[t.f, t.a, t.c], &[], "");
        assert_eq!(ids(&rows), [t.f, t.b, t.a, t.g]);
        assert!(!row(&rows, t.a).expanded);
    }

    #[test]
    fn hidden_and_locked_layers_say_so() {
        let mut t = tree();
        t.board
            .execute("object.toggleVisible", &json!({"ids": [t.a]}))
            .unwrap();
        t.board
            .execute("object.toggleLock", &json!({"ids": [t.g]}))
            .unwrap();
        let rows = t.all_rows();
        // Each row says what its own layer is, not what an ancestor makes of it.
        assert!(!row(&rows, t.a).visible && !row(&rows, t.a).locked);
        assert!(row(&rows, t.g).locked && row(&rows, t.g).visible);
        assert!(row(&rows, t.c).visible && !row(&rows, t.c).locked);
        assert!(row(&rows, t.b).visible && !row(&rows, t.b).locked);
    }

    #[test]
    fn filter_keeps_ancestors() {
        let t = tree();
        let rows = t.rows(&[], &[], "c");
        assert_eq!(outline(&rows), [(t.f, 0), (t.g, 1), (t.c, 2)]);
        // F and G open although nothing was expanded: they hold what was found.
        assert!(row(&rows, t.f).has_children && row(&rows, t.f).expanded);
        assert!(row(&rows, t.g).has_children && row(&rows, t.g).expanded);
        assert!(!row(&rows, t.c).has_children && !row(&rows, t.c).expanded);
    }

    #[test]
    fn filter_ignores_case_and_surrounding_space() {
        let t = tree();
        let found = t.rows(&[], &[], "c");
        assert_eq!(t.rows(&[], &[], "C"), found);
        assert_eq!(t.rows(&[], &[], "  c "), found);
        // Matching is by a part of the name, and several layers can match.
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let card = add(&mut doc, page, NodeKind::Frame, "Card");
        let title = add(&mut doc, card, NodeKind::Rectangle, "Title");
        let other = add(&mut doc, page, NodeKind::Frame, "Other");
        let by_part = rows(&doc, page, &HashSet::new(), &[], "CAR");
        assert_eq!(ids(&by_part), [card]);
        // Card matches and its child doesn't: nothing is listed under it, so it has nothing to open.
        assert!(!by_part[0].has_children && !by_part[0].expanded);
        let several = rows(&doc, page, &HashSet::new(), &[], "t");
        assert_eq!(ids(&several), [other, card, title]);
        assert!(row(&several, card).expanded);
    }

    #[test]
    fn a_blank_filter_lists_everything() {
        let t = tree();
        let all = t.rows(&[t.f], &[], "");
        assert_eq!(ids(&all), [t.f, t.b, t.a, t.g]);
        assert_eq!(t.rows(&[t.f], &[], "   "), all);
    }

    #[test]
    fn filter_without_matches_is_empty() {
        let t = tree();
        // The same rows without the filter, so an empty answer is the filter's.
        assert_eq!(t.rows(&[t.f, t.g], &[], "").len(), 5);
        assert!(t.rows(&[t.f, t.g], &[], "zzz").is_empty());
        assert!(t.rows(&[], &[], "zzz").is_empty());
    }

    #[test]
    fn filter_restores_expansion() {
        let t = tree();
        let doc = t.doc();
        // F open, G closed.
        let expanded = set(&[t.f]);
        let before = rows(&doc, t.page, &expanded, &[], "");
        assert_eq!(ids(&before), [t.f, t.b, t.a, t.g]);
        assert!(!row(&before, t.g).expanded);

        // The filter lists G open to show C...
        let found = rows(&doc, t.page, &expanded, &[], "c");
        assert_eq!(ids(&found), [t.f, t.g, t.c]);
        assert!(row(&found, t.g).expanded);
        // ...without touching the set, so clearing it gives back the rows with G closed.
        assert_eq!(expanded, set(&[t.f]));
        assert_eq!(rows(&doc, t.page, &expanded, &[], ""), before);
    }

    #[test]
    fn selected_rows_are_marked() {
        let t = tree();
        let rows = t.rows(&[t.f, t.g], &[t.a, t.c], "");
        let selected: Vec<NodeId> = rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.id)
            .collect();
        assert_eq!(selected, [t.a, t.c]);
        // A selection under a closed branch marks nothing, since the row isn't listed.
        let rows = t.rows(&[], &[t.a], "");
        assert!(rows.iter().all(|row| !row.selected));
        // The filter marks what it lists.
        let rows = t.rows(&[], &[t.c, t.a], "c");
        assert_eq!(
            rows.iter()
                .filter(|row| row.selected)
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [t.c]
        );
    }

    #[test]
    fn rows_of_something_that_is_not_there_are_empty() {
        let t = tree();
        assert!(!rows(&t.doc(), t.page, &HashSet::new(), &[], "").is_empty());
        assert!(rows(&t.doc(), NodeId(9_999), &HashSet::new(), &[], "").is_empty());
        assert!(rows(&t.doc(), NodeId(9_999), &HashSet::new(), &[], "a").is_empty());
    }

    #[test]
    fn sticky_has_no_children_rows() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let sticky = make(
            &mut board,
            "board.sticky",
            json!({"x": 0, "y": 0, "text": "needle"}),
        );
        let doc = board.doc().unwrap();
        let page = board.session.page().unwrap();
        // The note holds its text, but the panel shows one row, open or not.
        assert!(!doc.node(sticky).unwrap().children.is_empty());
        for expanded in [HashSet::new(), set(&[sticky])] {
            let rows = rows(&doc, page, &expanded, &[], "");
            assert_eq!(ids(&rows), [sticky]);
            assert_eq!(rows[0].icon, LayerIcon::Sticky);
            assert!(!rows[0].has_children);
            assert!(!rows[0].expanded);
        }
        // Nor does the filter go inside it: its text is not a layer to find, and the note
        // itself is not listed as the ancestor of one.
        let inside = doc.node(sticky).unwrap().children[0];
        let name = doc.node(inside).unwrap().name.clone();
        assert!(!name.is_empty());
        assert!(rows(&doc, page, &HashSet::new(), &[], &name).is_empty());
        let by_name = rows(&doc, page, &HashSet::new(), &[], "sticky");
        assert_eq!(ids(&by_name), [sticky]);
        assert!(!by_name[0].has_children);
    }

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

    #[test]
    fn a_layer_without_a_name_is_listed_and_found_by_its_kind() {
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let blank = add(
            &mut doc,
            page,
            NodeKind::Ellipse {
                start: 0.0,
                sweep: std::f64::consts::TAU,
                inner: 0.0,
            },
            "",
        );
        let spaces = add(&mut doc, page, NodeKind::Frame, "  ");
        let rows_all = rows(&doc, page, &HashSet::new(), &[], "");
        assert_eq!(row(&rows_all, blank).name, "Ellipse");
        assert_eq!(row(&rows_all, spaces).name, "Frame");
        assert_eq!(
            ids(&rows(&doc, page, &HashSet::new(), &[], "ellip")),
            [blank]
        );
    }

    #[test]
    fn drop_between_siblings_gives_bottom_first_index() {
        let mut t = tree();
        let rows = t.all_rows();
        // Dragging A above B: B is the top layer of F, so A goes on top of it.
        let reparent = t.target(&rows, &[t.a], t.b, DropPos::Above).unwrap();
        assert_eq!(reparent.parent, t.f);
        assert_eq!(reparent.ids, [t.a]);
        assert_eq!(reparent.index, 2);
        t.apply(&reparent);
        assert_eq!(t.children(t.f), [t.g, t.b, t.a]);
    }

    #[test]
    fn drop_below_a_row_goes_under_it() {
        let mut t = tree();
        let rows = t.all_rows();
        // B below G: G is the bottom layer, so B goes under it.
        let reparent = t.target(&rows, &[t.b], t.g, DropPos::Below).unwrap();
        assert_eq!((reparent.parent, reparent.index), (t.f, 0));
        t.apply(&reparent);
        assert_eq!(t.children(t.f), [t.b, t.g, t.a]);
    }

    #[test]
    fn drop_between_siblings_counts_layers_that_leave_the_gap() {
        let mut t = tree();
        let rows = t.all_rows();
        // G and A, which sit under B, leave the gap above B: of the three layers below it only
        // B stays, so the index is 1, and they land on top of B, in their order.
        let reparent = t.target(&rows, &[t.g, t.a], t.b, DropPos::Above).unwrap();
        assert_eq!((reparent.parent, reparent.index), (t.f, 1));
        t.apply(&reparent);
        assert_eq!(t.children(t.f), [t.b, t.g, t.a]);

        // B and A, above G, drop under it: nothing of theirs is below the gap, so the index is 0,
        // and they keep their order, A under B.
        let mut t = tree();
        let rows = t.all_rows();
        let reparent = t.target(&rows, &[t.b, t.a], t.g, DropPos::Below).unwrap();
        assert_eq!((reparent.parent, reparent.index), (t.f, 0));
        t.apply(&reparent);
        assert_eq!(t.children(t.f), [t.a, t.b, t.g]);
    }

    #[test]
    fn drop_above_a_top_level_row_goes_to_the_page() {
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let first = add(&mut doc, page, NodeKind::Frame, "First");
        let second = add(&mut doc, page, NodeKind::Frame, "Second");
        let inner = add(&mut doc, first, NodeKind::Rectangle, "Inner");
        let mut board = file(doc);
        let doc = board.doc().unwrap();
        let rows = rows(&doc, page, &set(&[first]), &[], "");
        assert_eq!(ids(&rows), [second, first, inner]);

        // Inner above Second: out of First, onto the page above Second.
        let reparent = drop_on(&doc, &rows, &[inner], second, DropPos::Above).unwrap();
        assert_eq!((reparent.parent, reparent.index), (page, 2));
        apply(&mut board, &reparent);
        assert_eq!(
            board.doc().unwrap().node(page).unwrap().children,
            [first, second, inner]
        );
    }

    #[test]
    fn drop_into_frame() {
        let mut t = tree();
        let rows = t.all_rows();
        let reparent = t.target(&rows, &[t.c], t.f, DropPos::Into).unwrap();
        assert_eq!(reparent.parent, t.f);
        assert_eq!(reparent.ids, [t.c]);
        // On top of what F holds.
        assert_eq!(reparent.index, 3);
        t.apply(&reparent);
        // C is on top of A and B. G held only C, and the engine removes a group that is left empty.
        assert_eq!(t.children(t.f), [t.a, t.b, t.c]);
        assert_eq!(t.doc().node(t.c).unwrap().parent, Some(t.f));
        assert!(t.doc().node(t.g).is_none());
    }

    #[test]
    fn drop_into_a_parent_of_the_dragged_layer_puts_it_on_top() {
        let mut t = tree();
        let rows = t.all_rows();
        let reparent = t.target(&rows, &[t.g], t.f, DropPos::Into).unwrap();
        assert_eq!((reparent.parent, reparent.index), (t.f, 2));
        t.apply(&reparent);
        assert_eq!(t.children(t.f), [t.a, t.b, t.g]);
    }

    #[test]
    fn drop_into_a_group_a_section_and_a_component() {
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let group = add(&mut doc, page, NodeKind::Group, "Group");
        let kept = add(&mut doc, group, NodeKind::Rectangle, "Kept");
        let outer = add(&mut doc, page, section(), "Section");
        let main = add(&mut doc, page, component(), "Main");
        let loose = add(&mut doc, page, NodeKind::Rectangle, "Loose");
        let board = file(doc);
        let doc = board.doc().unwrap();
        let rows = rows(&doc, page, &HashSet::new(), &[], "");
        for into in [group, outer, main] {
            let reparent = drop_on(&doc, &rows, &[loose], into, DropPos::Into).unwrap();
            assert_eq!(reparent.parent, into);
            let held = doc.node(into).unwrap().children.len();
            assert_eq!(reparent.index, held);
        }
        assert_eq!(doc.node(group).unwrap().children, [kept]);
    }

    #[test]
    fn drop_into_own_subtree_is_refused() {
        let t = tree();
        let rows = t.all_rows();
        // F into its own group, and onto, above or below any row inside it.
        assert_eq!(t.target(&rows, &[t.f], t.g, DropPos::Into), None);
        for pos in [DropPos::Above, DropPos::Below, DropPos::Into] {
            assert_eq!(t.target(&rows, &[t.f], t.c, pos), None, "{pos:?}");
            assert_eq!(t.target(&rows, &[t.f], t.a, pos), None, "{pos:?}");
        }
        // G into C's layer, and next to C inside G.
        assert_eq!(t.target(&rows, &[t.g], t.c, DropPos::Above), None);
        assert_eq!(t.target(&rows, &[t.g], t.c, DropPos::Into), None);
        // Any of several dragged layers decides.
        assert_eq!(t.target(&rows, &[t.a, t.g], t.c, DropPos::Below), None);
        // A layer onto itself.
        for pos in [DropPos::Above, DropPos::Below, DropPos::Into] {
            assert_eq!(t.target(&rows, &[t.a], t.a, pos), None, "{pos:?}");
            assert_eq!(t.target(&rows, &[t.a, t.b], t.b, pos), None, "{pos:?}");
        }
        // Next to the dragged layer's parent is fine.
        assert!(t.target(&rows, &[t.g], t.f, DropPos::Above).is_some());
    }

    #[test]
    fn nothing_dragged_or_nowhere_to_drop_is_no_drop() {
        let t = tree();
        let rows = t.all_rows();
        let doc = t.doc();
        assert_eq!(t.target(&rows, &[], t.f, DropPos::Into), None);
        assert_eq!(drop_target(&doc, &rows, &[t.a], 99, DropPos::Into), None);
        assert_eq!(drop_target(&doc, &[], &[t.a], 0, DropPos::Above), None);
        // A layer that is gone is left out, and a drag of nothing else has no drop.
        assert_eq!(t.target(&rows, &[NodeId(9_999)], t.f, DropPos::Into), None);
        let reparent = t
            .target(&rows, &[NodeId(9_999), t.c], t.f, DropPos::Into)
            .unwrap();
        assert_eq!(reparent.ids, [t.c]);
        // A page doesn't move into a layer, and a layer listed twice moves once.
        assert_eq!(t.target(&rows, &[t.page], t.f, DropPos::Into), None);
        let reparent = t.target(&rows, &[t.c, t.c], t.f, DropPos::Into).unwrap();
        assert_eq!(reparent.ids, [t.c]);
    }

    #[test]
    fn drop_into_sticky_is_refused() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let first = make(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let second = make(&mut board, "board.sticky", json!({"x": 300, "y": 0}));
        let doc = board.doc().unwrap();
        let page = board.session.page().unwrap();
        let rows = rows(&doc, page, &HashSet::new(), &[], "");
        assert_eq!(ids(&rows), [second, first]);

        // Into a sticky the layer would vanish from the panel; next to it is fine.
        assert_eq!(drop_on(&doc, &rows, &[second], first, DropPos::Into), None);
        let reparent = drop_on(&doc, &rows, &[second], first, DropPos::Below).unwrap();
        assert_eq!((reparent.parent, reparent.index), (page, 0));
    }

    #[test]
    fn board_items_that_hold_layers_take_none_into_them() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let shape = make(
            &mut board,
            "board.shape",
            json!({"shape": "ellipse", "x": 0, "y": 0}),
        );
        let table = make(
            &mut board,
            "board.table",
            json!({"rows": 2, "columns": 2, "x": 0, "y": 400}),
        );
        let stamp = make(
            &mut board,
            "board.stamp",
            json!({"x": 500, "y": 0, "stamp": "heart"}),
        );
        let loose = make(&mut board, "board.sticky", json!({"x": 700, "y": 0}));
        let doc = board.doc().unwrap();
        let page = board.session.page().unwrap();
        let rows = rows(&doc, page, &HashSet::new(), &[], "");
        for atom in [shape, table, stamp] {
            assert_eq!(drop_on(&doc, &rows, &[loose], atom, DropPos::Into), None);
            assert!(drop_on(&doc, &rows, &[loose], atom, DropPos::Above).is_some());
        }
    }

    #[test]
    fn section_into_frame_is_refused() {
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let outer = add(&mut doc, page, section(), "Outer");
        let frame = add(&mut doc, page, NodeKind::Frame, "Frame");
        let inner = add(&mut doc, frame, NodeKind::Rectangle, "Inner");
        let group = add(&mut doc, page, NodeKind::Group, "Group");
        let moved = add(&mut doc, page, section(), "Moved");
        let board = file(doc);
        let doc = board.doc().unwrap();
        let rows = rows(&doc, page, &set(&[frame]), &[], "");
        assert_eq!(ids(&rows), [moved, group, frame, inner, outer]);

        // Sections go on the page or in sections, never in a frame or a group...
        assert_eq!(drop_on(&doc, &rows, &[moved], frame, DropPos::Into), None);
        assert_eq!(drop_on(&doc, &rows, &[moved], group, DropPos::Into), None);
        // ...also when the drop is next to one of the layers in the frame.
        assert_eq!(drop_on(&doc, &rows, &[moved], inner, DropPos::Above), None);
        assert_eq!(drop_on(&doc, &rows, &[moved], inner, DropPos::Below), None);
        // A section among other layers drags as one of them.
        assert_eq!(
            drop_on(&doc, &rows, &[inner, moved], frame, DropPos::Into),
            None
        );
        // On the page, and in another section, it is fine.
        let onto_page = drop_on(&doc, &rows, &[moved], frame, DropPos::Above).unwrap();
        assert_eq!(onto_page.parent, page);
        let into_section = drop_on(&doc, &rows, &[moved], outer, DropPos::Into).unwrap();
        assert_eq!(into_section.parent, outer);
        // Other layers go into frames and sections as before.
        let group_in_frame = drop_on(&doc, &rows, &[group], frame, DropPos::Into).unwrap();
        assert_eq!(group_in_frame.parent, frame);
        let rectangle = drop_on(&doc, &rows, &[inner], outer, DropPos::Into).unwrap();
        assert_eq!(rectangle.parent, outer);
    }

    #[test]
    fn instances_and_what_is_inside_them_take_no_drops() {
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let main = add(&mut doc, page, component(), "Main");
        let instance = add(
            &mut doc,
            page,
            NodeKind::Instance {
                component: main,
                props: BTreeMap::new(),
            },
            "Instance",
        );
        let inside = add(&mut doc, instance, NodeKind::Frame, "Inside");
        let loose = add(&mut doc, page, NodeKind::Rectangle, "Loose");
        let board = file(doc);
        let doc = board.doc().unwrap();
        let rows = rows(&doc, page, &set(&[instance]), &[], "");
        assert_eq!(ids(&rows), [loose, instance, inside, main]);

        // The engine refuses to add, remove or reorder layers in an instance.
        for pos in [DropPos::Into, DropPos::Above, DropPos::Below] {
            assert_eq!(drop_on(&doc, &rows, &[loose], inside, pos), None, "{pos:?}");
        }
        assert_eq!(
            drop_on(&doc, &rows, &[loose], instance, DropPos::Into),
            None
        );
        // Next to the instance is the page, and its main component takes layers.
        let beside = drop_on(&doc, &rows, &[loose], instance, DropPos::Above).unwrap();
        assert_eq!(beside.parent, page);
        let into_main = drop_on(&doc, &rows, &[loose], main, DropPos::Into).unwrap();
        assert_eq!(into_main.parent, main);
    }

    #[test]
    fn pages_list_and_dividers() {
        let mut board = file(Document::new("Design"));
        let first = board.session.page().unwrap();
        board.execute("page.add", &json!({"name": "Two"})).unwrap();
        board.execute("page.add", &json!({"name": "-"})).unwrap();
        // Adding a page opens it, a divider doesn't open.
        let current = board.session.page().unwrap();
        let doc = board.doc().unwrap();
        assert_eq!(doc.pages.len(), 3);
        assert_eq!(doc.pages[0], first);
        assert_eq!(current, doc.pages[1]);

        let rows = pages(&doc, current);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), doc.pages);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["Page 1", "Two", "-"]);
        let current: Vec<bool> = rows.iter().map(|row| row.current).collect();
        assert_eq!(current, [false, true, false]);
        let divider: Vec<bool> = rows.iter().map(|row| row.divider).collect();
        assert_eq!(divider, [false, false, true]);
    }

    #[test]
    fn a_page_with_layers_is_not_a_divider_whatever_its_name() {
        let mut board = file(Document::new("Design"));
        board
            .execute("page.add", &json!({"name": "Notes"}))
            .unwrap();
        board
            .execute(
                "node.create",
                &json!({"type": "rectangle", "width": 10, "height": 10}),
            )
            .unwrap();
        let notes = board.session.page().unwrap();
        board
            .execute("page.rename", &json!({"id": notes, "name": "-notes"}))
            .unwrap();
        let doc = board.doc().unwrap();
        let rows = pages(&doc, notes);
        assert_eq!(rows[1].name, "-notes");
        // The engine opens it, so the panel must offer it.
        assert!(!rows[1].divider);
        board
            .execute("page.select", &json!({"id": doc.pages[0]}))
            .unwrap();
        board.execute("page.select", &json!({"id": notes})).unwrap();
    }

    #[test]
    fn reveal_expands_ancestors() {
        let t = tree();
        let doc = t.doc();
        assert_eq!(reveal(&doc, &[t.c]), set(&[t.f, t.g]));
        // The page is no row, a layer opens its ancestors and not itself, and layers add up.
        assert_eq!(reveal(&doc, &[t.g]), set(&[t.f]));
        assert_eq!(reveal(&doc, &[t.f]), set(&[]));
        assert_eq!(reveal(&doc, &[t.a, t.c]), set(&[t.f, t.g]));
        assert_eq!(reveal(&doc, &[]), set(&[]));
        assert_eq!(reveal(&doc, &[NodeId(9_999), t.page]), set(&[]));
    }

    #[test]
    fn reveal_shows_the_selection_in_the_rows() {
        let t = tree();
        let doc = t.doc();
        let open = reveal(&doc, &[t.c]);
        let rows = rows(&doc, t.page, &open, &[t.c], "");
        assert!(row(&rows, t.c).selected);
    }

    #[test]
    fn reveal_skips_board_items_that_hide_their_insides() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let section = make(
            &mut board,
            "node.create",
            json!({"type": "section", "x": 0, "y": 0, "width": 600, "height": 600}),
        );
        let sticky = make(
            &mut board,
            "board.sticky",
            json!({"x": 0, "y": 0, "text": "hi", "parent": section}),
        );
        let doc = board.doc().unwrap();
        assert_eq!(doc.node(sticky).unwrap().parent, Some(section));
        let text = doc.node(sticky).unwrap().children[0];
        // The note can't open, so opening it would show nothing; the section it is in opens.
        assert_eq!(reveal(&doc, &[text]), set(&[section]));
        assert_eq!(reveal(&doc, &[sticky]), set(&[section]));
    }

    #[test]
    fn deep_tree_rows_are_linear() {
        // 200 nested groups with a layer in the innermost: 201 rows, one per level.
        let mut doc = Document::new("Deep");
        let page = doc.first_page().unwrap();
        let mut parent = page;
        let mut groups = Vec::new();
        for n in 0..200 {
            parent = add(&mut doc, parent, NodeKind::Group, &format!("Group {n}"));
            groups.push(parent);
        }
        let leaf = add(&mut doc, parent, NodeKind::Rectangle, "Leaf");
        let expanded: HashSet<NodeId> = groups.iter().copied().collect();

        // Only the rows are timed, best of three so a busy machine doesn't fail it.
        let mut best = Duration::MAX;
        let mut listed = Vec::new();
        for _ in 0..3 {
            let start = Instant::now();
            listed = rows(&doc, page, &expanded, &[leaf], "");
            best = best.min(start.elapsed());
        }
        assert!(best < Duration::from_millis(20), "{best:?}");

        assert_eq!(listed.len(), 201);
        let depths: Vec<usize> = listed.iter().map(|row| row.depth).collect();
        assert_eq!(depths, (0..=200).collect::<Vec<_>>());
        let order: Vec<NodeId> = groups.iter().copied().chain([leaf]).collect();
        assert_eq!(ids(&listed), order);
        assert!(row(&listed, leaf).selected);

        // The filter and the reveal walk the same chain.
        let found = rows(&doc, page, &HashSet::new(), &[], "leaf");
        assert_eq!(ids(&found), order);
        assert_eq!(reveal(&doc, &[leaf]), expanded);
    }
}
