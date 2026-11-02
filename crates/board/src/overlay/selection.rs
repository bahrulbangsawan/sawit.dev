//! The layer under the pointer and the selection (items 2 to 5 of [`super::overlay`]): their
//! outlines, the selection's box, handles and size, and a selected layer's arc and corner radius
//! handles with the readout of the one dragged.

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::arc_handles::ArcHandle;
use figcraft_engine::shape_handles::ShapeHandle;
use figcraft_engine::{DocState, Session, ToolId, ViewState, board, components, tools};
use figcraft_geom::Point;
use figcraft_geom::kurbo::{self, PathEl};

use super::geometry::{at, extent, node_quad, screen, whole};
use super::{
    ARC, ARC_DOT, ARC_HOVERED, Anchor, COMPONENT, CORNER, END, LABEL_LINE, RADIUS, RADIUS_HOVERED,
    READOUT_OFFSET, READOUT_PAD, Rgba, SELECTION, SIZE_GAP, Shape, WHITE,
};

/// Item 2: the layer under the pointer, outlined 1.5 px, unless it is selected: with the Move or
/// Scale tool, while the preference to highlight layers on hover is on, and not while measuring
/// (Alt).
pub(super) fn hover(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let Some(id) = ds.hover else { return };
    let shown = !ds.selection.contains(&id)
        && matches!(s.tool.tool, ToolId::Move | ToolId::Scale)
        && s.prefs.highlight_on_hover
        && s.tool.overlay.measure_box.is_none();
    if shown {
        let color = outline_color(&ds.doc, id);
        outline(out, &ds.doc, id, &ds.view, color, 1.5);
    }
}

/// Items 3 to 5: the selection, unless text or a vector is being edited: each layer's outline, in
/// the component colour for all of them when one is in a component; the selection's box, outlined
/// around several layers only (one layer's own outline is its box); 4 square handles on its
/// corners, or 2 on a line's ends, or none for a locked layer, while measuring or when the box is
/// too small on screen to show them; a selected connector's 2 round end handles instead; the size
/// label, `"{w} × {h}"` in whole pixels (a line's length) centred under the box, unless the
/// preference to show dimensions is off; and, but while measuring, the arc handles and corner
/// radius handles the Move tool answers ([`arc_overlay`], [`shape_overlay`]).
pub(super) fn selection(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let (doc, view, sel) = (&ds.doc, &ds.view, &ds.selection);
    if sel.is_empty() || ds.text_edit.is_some() || ds.vector_edit.is_some() {
        return;
    }
    let color = if sel.iter().any(|id| outline_color(doc, *id) == COMPONENT) {
        COMPONENT
    } else {
        SELECTION
    };
    for id in sel {
        outline(out, doc, *id, view, color, 1.0);
    }
    // The Shape Builder reshapes by regions: no box.
    if s.tool.tool == ToolId::ShapeBuilder {
        return;
    }
    let Some((frame, width, height)) = tools::selection_frame(doc, sel) else {
        return;
    };
    // The corners `tools::selection_box` gives, from this one frame: each of the two works the
    // frame out (the selection's top layers and their bounds) on every frame.
    let corners = [
        frame * Point::ZERO,
        frame * Point::new(width, 0.0),
        frame * Point::new(width, height),
        frame * Point::new(0.0, height),
    ];
    let line = match sel.as_slice() {
        [id] => doc
            .node(*id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Line)),
        _ => false,
    };
    let ends = board::selected_connector_ends(doc, sel).map(|(_, ends)| ends);
    let measuring = s.tool.overlay.measure_box.is_some();
    let locked = sel.iter().any(|id| tools::locked_in(doc, *id));
    let handles = if measuring || s.tool.dev_mode || ends.is_some() || locked {
        0
    } else if line {
        2
    } else if tools::box_sides_px(&corners, view.zoom).1 < tools::SMALL_BOX_PX {
        0
    } else {
        4
    };
    let corners = corners.map(|p| screen(view, p));
    if sel.len() > 1 {
        out.push(Shape::Outline {
            points: corners.to_vec(),
            closed: true,
            color,
            width: 1.0,
            dashed: false,
        });
    }
    for &center in corners.iter().take(handles) {
        out.push(Shape::Handle {
            center,
            size: CORNER,
            round: false,
            fill: WHITE,
            stroke: color,
        });
    }
    for end in ends.into_iter().flatten().filter(|_| !s.tool.dev_mode) {
        out.push(Shape::Handle {
            center: screen(view, end),
            size: END,
            round: true,
            fill: WHITE,
            stroke: color,
        });
    }
    if !measuring && ends.is_none() && s.prefs.show_dimensions {
        let text = if line {
            whole(width).to_string()
        } else {
            format!("{} × {}", whole(width), whole(height))
        };
        let (_, _, _, bottom) = extent(&corners);
        let middle = corners.iter().map(|c| c.0).sum::<f64>() / 4.0;
        let half = LABEL_LINE / 2.0 + READOUT_PAD.1;
        out.push(Shape::Label {
            at: (middle, bottom + SIZE_GAP + half),
            anchor: Anchor::Middle,
            text,
            color: WHITE,
            background: Some(color),
            node: None,
        });
    }
    if !measuring {
        arc_overlay(out, s, view, color);
        shape_overlay(out, s, view, color);
    }
}

/// The selected ellipse's arc handles, as the Move tool shows and answers them
/// (`Session::arc_handles`, which leaves them out with another tool, while another gesture runs,
/// and on an ellipse locked or small on screen): round, larger under the pointer or while
/// dragged, Start with a dot inside; and while one is dragged, its readout beside it (ui-egui
/// `arc_overlay`).
fn arc_overlay(out: &mut Vec<Shape>, s: &Session, view: &ViewState, color: Rgba) {
    let ov = &s.tool.overlay;
    if let Some(handles) = s.arc_handles() {
        for handle in [ArcHandle::Ratio, ArcHandle::Start, ArcHandle::Sweep] {
            let Some([x, y]) = handles.at(handle) else {
                continue;
            };
            let center = at(view, x, y);
            out.push(Shape::Handle {
                center,
                size: if ov.arc_hover == Some(handle) {
                    ARC_HOVERED
                } else {
                    ARC
                },
                round: true,
                fill: WHITE,
                stroke: color,
            });
            if handle == ArcHandle::Start {
                out.push(Shape::Handle {
                    center,
                    size: ARC_DOT,
                    round: true,
                    fill: color,
                    stroke: color,
                });
            }
        }
    }
    if let Some(hint) = &ov.arc {
        out.push(readout_beside(view, hint.at, hint.label.clone(), color));
    }
}

/// The selected rectangle's, frame's (a sticky's, a board shape's), polygon's or star's corner
/// radius handles and a star's ratio handle, as the Move tool shows and answers them
/// (`Session::shape_handles`, which leaves them out as for the arc handles, and on a layer under
/// 48 px on screen): round, larger under the pointer or while dragged; and while one is dragged,
/// its value beside it (ui-egui `shape_overlay`).
fn shape_overlay(out: &mut Vec<Shape>, s: &Session, view: &ViewState, color: Rgba) {
    let ov = &s.tool.overlay;
    for (handle, [x, y]) in s.shape_handles() {
        out.push(Shape::Handle {
            center: at(view, x, y),
            size: if ov.shape_hover == Some(handle) {
                RADIUS_HOVERED
            } else {
                RADIUS
            },
            round: true,
            fill: WHITE,
            stroke: color,
        });
    }
    if let Some(hint) = &ov.shape {
        let text = match hint.handle {
            ShapeHandle::Radius { .. } => format!("Radius {}", hint.value),
            ShapeHandle::Ratio => format!("Ratio {}%", hint.value),
        };
        out.push(readout_beside(view, hint.at, text, color));
    }
}

/// A dragged handle's readout: `text` in white on a pill of `color`, its top-left corner
/// [`READOUT_OFFSET`] px right of and below the handle at page point `handle`.
fn readout_beside(view: &ViewState, [x, y]: [f64; 2], text: String, color: Rgba) -> Shape {
    let (hx, hy) = at(view, x, y);
    Shape::Label {
        at: (hx + READOUT_OFFSET, hy + READOUT_OFFSET),
        anchor: Anchor::TopLeft,
        text,
        color: WHITE,
        background: Some(color),
        node: None,
    }
}

/// Layer `id`'s outline, `width` px: its shape for ellipses, vectors (connectors and ink among
/// them), polygons, stars and boolean groups, its box otherwise.
fn outline(
    out: &mut Vec<Shape>,
    doc: &Document,
    id: NodeId,
    view: &ViewState,
    color: Rgba,
    width: f64,
) {
    let lines = shape_lines(doc, id, view)
        .or_else(|| node_quad(doc, id, view).map(|quad| vec![(quad.to_vec(), true)]));
    for (points, closed) in lines.into_iter().flatten() {
        out.push(Shape::Outline {
            points,
            closed,
            color,
            width,
            dashed: false,
        });
    }
}

/// Points on screen, and whether the line through them closes.
type Line = (Vec<(f64, f64)>, bool);

/// The outline of an ellipse, vector, polygon, star or boolean group on screen, as lines to within
/// half a pixel. `None` for other layers.
fn shape_lines(doc: &Document, id: NodeId, view: &ViewState) -> Option<Vec<Line>> {
    let n = doc.node(id)?;
    let shaped = matches!(
        n.kind,
        NodeKind::Ellipse { .. }
            | NodeKind::Vector { .. }
            | NodeKind::Polygon { .. }
            | NodeKind::Star { .. }
            | NodeKind::BooleanOp { .. }
    );
    if !shaped {
        return None;
    }
    let (path, _) = figcraft_render::node_outline(doc, n)?;
    let world = doc.world_transform(id);
    let mut lines = Vec::new();
    let mut line: Vec<(f64, f64)> = Vec::new();
    kurbo::flatten(path.iter(), 0.5 / view.zoom.max(1e-6), |el| match el {
        PathEl::MoveTo(p) => {
            if line.len() > 1 {
                lines.push((std::mem::take(&mut line), false));
            }
            line.clear();
            line.push(screen(view, world * p));
        }
        PathEl::LineTo(p) => line.push(screen(view, world * p)),
        PathEl::ClosePath => {
            // The next line, if any, starts where this one did.
            let start = line.first().copied();
            if line.len() > 1 {
                lines.push((std::mem::take(&mut line), true));
            }
            line.clear();
            line.extend(start);
        }
        _ => {}
    });
    if line.len() > 1 {
        lines.push((line, false));
    }
    Some(lines)
}

/// The colour of a layer's outline: [`COMPONENT`] for a component set, and for a component, an
/// instance and every layer in one; [`SELECTION`] otherwise.
fn outline_color(doc: &Document, id: NodeId) -> Rgba {
    let set = matches!(
        doc.node(id).map(|n| &n.kind),
        Some(NodeKind::ComponentSet { .. })
    );
    let in_component = components::owning_component(doc, id).is_some()
        || components::owning_instance(doc, id).is_some();
    if set || in_component {
        COMPONENT
    } else {
        SELECTION
    }
}
