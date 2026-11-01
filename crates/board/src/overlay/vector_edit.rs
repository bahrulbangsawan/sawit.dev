//! Vector edit mode (item 11 of [`super::overlay`]): each edited layer's segments, Bézier
//! handles and points, and the pen's next segment.

use figcraft_doc::NodeId;
use figcraft_engine::vectoredit::{self, EditOverlay, VHit, VTool};
use figcraft_engine::{DocState, Session, ViewState};

use super::geometry::{at, bezier_handle, circle, cubic, point};
use super::{POINT, POINT_HOVERED, SELECTION, Shape, WHITE};

/// Item 11: vector edit mode, each edited layer's segments (thicker when selected or under the
/// pointer) and, but for the Variable width tool, its Bézier handles, its points (filled when
/// selected, larger under the pointer), a ring around the point the pen goes on from, and the pen's
/// next segment dashed; the active layer last.
pub(super) fn vector_edit(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let Some(ve) = &ds.vector_edit else { return };
    let (doc, view, ov) = (&ds.doc, &ds.view, &s.tool.overlay);
    let points = ve.tool != VTool::Width;
    let hover_on = |id: NodeId| {
        ov.vector_hover
            .filter(|_| ov.vector_hover_on.unwrap_or(ve.id) == id)
    };
    for part in &ve.others {
        if let Some(o) = vectoredit::overlay(doc, &ve.view(part)) {
            edit_overlay(out, &o, view, points, hover_on(part.id), None);
        }
    }
    if let Some(o) = vectoredit::overlay(doc, ve) {
        edit_overlay(out, &o, view, points, hover_on(ve.id), ov.rubber);
    }
}

/// One edited layer's segments, and with `points` its handles and points, `hover` being the part
/// under the pointer, and the pen's `rubber` band.
fn edit_overlay(
    out: &mut Vec<Shape>,
    o: &EditOverlay,
    view: &ViewState,
    points: bool,
    hover: Option<VHit>,
    rubber: Option<[f64; 4]>,
) {
    let hovered_segment = match hover {
        Some(VHit::Segment { seg, .. }) => Some(seg),
        _ => None,
    };
    for (i, segment) in o.segments.iter().enumerate() {
        let width = if o.selected_segments.contains(&i) {
            3.0
        } else if hovered_segment == Some(i) {
            2.5
        } else {
            1.5
        };
        out.push(Shape::Outline {
            points: cubic(segment.map(|[x, y]| at(view, x, y))),
            closed: false,
            color: SELECTION,
            width,
            dashed: false,
        });
    }
    if !points {
        return;
    }
    for h in &o.handles {
        let hovered = hover
            == Some(VHit::Handle {
                seg: h.seg,
                start: h.start,
            });
        let size = if hovered { POINT_HOVERED } else { POINT };
        let selected = o.selected_handles.contains(&(h.seg, h.start));
        let fill = if selected { SELECTION } else { WHITE };
        let [vx, vy] = h.vertex;
        let [hx, hy] = h.handle;
        bezier_handle(out, at(view, vx, vy), at(view, hx, hy), size, fill);
    }
    for (i, [x, y]) in o.vertices.iter().enumerate() {
        let center = at(view, *x, *y);
        let hovered = matches!(hover, Some(VHit::Vertex { index }) if index == i);
        let size = if hovered { POINT_HOVERED } else { POINT };
        let selected = o.selected_vertices.contains(&i);
        out.push(point(
            center,
            size,
            if selected { SELECTION } else { WHITE },
        ));
        if o.anchor == Some(i) {
            out.push(Shape::Outline {
                points: circle(center, 7.5),
                closed: true,
                color: SELECTION,
                width: 1.0,
                dashed: false,
            });
        }
    }
    if let Some([x0, y0, x1, y1]) = rubber {
        out.push(Shape::Outline {
            points: vec![at(view, x0, y0), at(view, x1, y1)],
            closed: false,
            color: SELECTION,
            width: 1.0,
            dashed: true,
        });
    }
}
