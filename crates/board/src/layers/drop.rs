//! Where a dragged layer lands: the move [`drop_target`] works out for a drop on a row, checked
//! against the document's rules and the engine's.

use std::collections::HashSet;

use figcraft_doc::{Document, Node, NodeId, NodeKind};
use figcraft_engine::components::structure_locked;

use super::LayerRow;

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
///   go on the page or in other sections;
/// - where it changes nothing: every layer that moves (a dragged layer that no other dragged layer
///   holds) is a layer of the parent already, and `node.reparent` would leave them in the order
///   they have. That is a layer dropped on the gap it fills, or on top of a parent it tops. A drop
///   line there would mark where the layers are, and running it would record an undo step that
///   changes nothing and work the layers' transforms out again.
///
/// Dragged layers that are no longer there, and pages, are left out, and one that is listed twice
/// moves once. Which layers are dragged is for the view to say: FigCraft drags the whole selection
/// when the dragged row is in it.
///
/// One thing is left to the engine: a dragged layer that is inside an instance is not checked, the
/// engine refuses to move it and says why.
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
    if leaves_in_place(doc, parent, &moving, index) {
        return None;
    }
    Some(Reparent {
        ids,
        parent: parent.id,
        index,
    })
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

/// Whether `node.reparent` of the `moving` layers into `parent` at `index` leaves `parent`'s layers
/// as they are. A layer moves when it is dragged and no other dragged layer holds it: a held one
/// goes along with its holder. Nothing changes when every one that moves is a layer of `parent`
/// already and they land in the order they have, which is the order the engine makes of them:
/// `parent`'s layers that stay, with the moved ones, in their order, put in at `index`.
fn leaves_in_place(doc: &Document, parent: &Node, moving: &HashSet<NodeId>, index: usize) -> bool {
    let in_parent = |id: &NodeId| {
        doc.node(*id)
            .is_some_and(|node| node.parent == Some(parent.id))
    };
    let goes_along = |id: &NodeId| doc.ancestors(*id).iter().any(|up| moving.contains(up));
    if !moving.iter().all(|id| in_parent(id) || goes_along(id)) {
        return false;
    }
    let stay = parent.children.iter().filter(|id| !moving.contains(id));
    let moved = parent.children.iter().filter(|id| moving.contains(id));
    stay.clone()
        .take(index)
        .chain(moved)
        .chain(stay.skip(index))
        .eq(&parent.children)
}
