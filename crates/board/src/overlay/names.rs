//! The names of the frames and sections on the page (item 1 of [`super::overlay`]), and how wide
//! each may be on screen.

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::{DocState, ViewState};

use super::geometry::{extent, node_quad};
use super::{
    Anchor, COMPONENT, LABEL_LINE, NAME_GAP, NAME_MIN, NAME_PAD, NAME_REACH, ON_DARK, ON_LIGHT,
    SECTION_NAME_MAX, SELECTION, Shape, WHITE,
};

/// How wide the name of the frame or section `id` may be on screen, as ui-egui cuts names
/// (`frame_labels.rs`): a frame's name, the frame's width there, at least 20 px; a section's, that
/// less its pill's padding, 20 to 480 px. A name stays upright on a turned frame, so the width is
/// that of the frame's box on screen. The app cuts a longer name with an ellipsis, so the name, and
/// where the engine takes a press on it, stay over the frame. `None` for a layer not in `doc`.
pub fn name_room(doc: &Document, id: NodeId, view: &ViewState) -> Option<f64> {
    let n = doc.node(id)?;
    let (x0, _, x1, _) = extent(&node_quad(doc, id, view)?);
    let width = Some(x1 - x0).filter(|w| w.is_finite()).unwrap_or(0.0);
    Some(if matches!(n.kind, NodeKind::Section { .. }) {
        (width - 2.0 * NAME_PAD.0).clamp(NAME_MIN, SECTION_NAME_MAX)
    } else {
        width.max(NAME_MIN)
    })
}

/// Item 1 of [`overlay`](super::overlay): the names of the frames and sections on the page, and of
/// those in its sections, as ui-egui places them, upright: a frame's just above its top-left
/// corner, in its component colour, the selection colour while it is selected or under the pointer,
/// else grey; a section's on a pill inside its top-left corner, white on the selection colour while
/// it is selected. Board objects (stickies, shapes, stamps) are frames without a name shown. A
/// frame off the canvas has none drawn, and the focus view (Dev Mode) shows none. The app cuts each
/// name to its [`name_room`].
pub(super) fn frame_names(out: &mut Vec<Shape>, ds: &DocState) {
    if ds.focused().is_some() {
        return;
    }
    let (doc, view) = (&ds.doc, &ds.view);
    let names = if dark_page(doc, ds.page) {
        &ON_DARK
    } else {
        &ON_LIGHT
    };
    for id in named(doc, ds.page) {
        let (Some(n), Some(quad)) = (doc.node(id), node_quad(doc, id, view)) else {
            continue;
        };
        let (x0, y0, x1, y1) = extent(&quad);
        let off = x1 < -NAME_REACH
            || y1 < -NAME_REACH
            || x0 > view.vw + NAME_REACH
            || y0 > view.vh + NAME_REACH;
        if off {
            continue;
        }
        let selected = ds.selection.contains(&id);
        let label = if matches!(n.kind, NodeKind::Section { .. }) {
            Shape::Label {
                at: (x0, y0),
                anchor: Anchor::TopLeft,
                text: n.name.clone(),
                color: if selected { WHITE } else { names.section },
                background: Some(if selected { SELECTION } else { names.pill }),
                node: Some(id),
            }
        } else {
            let component = matches!(
                n.kind,
                NodeKind::Component { .. } | NodeKind::ComponentSet { .. }
            );
            let color = if component {
                COMPONENT
            } else if selected || ds.hover == Some(id) {
                SELECTION
            } else {
                names.frame
            };
            Shape::Label {
                at: (x0, y0 - NAME_GAP - LABEL_LINE),
                anchor: Anchor::TopLeft,
                text: n.name.clone(),
                color,
                background: None,
                node: Some(id),
            }
        };
        out.push(label);
    }
}

/// The layers whose name the canvas shows, in the order they are drawn: the visible frames and
/// sections on `page`, and in its sections whose contents show, each section after what it holds
/// (ui-egui's `rulers::boards` with its `labelled` filter). Board objects are left out.
fn named(doc: &Document, page: NodeId) -> Vec<NodeId> {
    fn walk(doc: &Document, id: NodeId, depth: usize, out: &mut Vec<NodeId>) {
        let Some(n) = doc.node(id) else { return };
        for &child in &n.children {
            let Some(c) = doc.node(child).filter(|c| c.visible) else {
                continue;
            };
            if matches!(
                c.kind,
                NodeKind::Section {
                    contents_hidden: false
                }
            ) && depth < 32
            {
                walk(doc, child, depth + 1, out);
            }
            let framed = c.kind.is_frame_like() || matches!(c.kind, NodeKind::Section { .. });
            if framed && !c.is_board_atom() {
                out.push(child);
            }
        }
    }
    let mut out = Vec::new();
    walk(doc, page, 0, &mut out);
    out
}

/// Whether `page`'s colour is dark: names on it are set light.
fn dark_page(doc: &Document, page: NodeId) -> bool {
    matches!(
        doc.node(page).map(|n| &n.kind),
        Some(NodeKind::Page { background, .. }) if background.is_dark()
    )
}
