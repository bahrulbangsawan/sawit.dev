//! What a gesture under way draws (items 7 to 10 of [`super::overlay`]): the marquee, snap guides
//! with their distances, where a drag drops, and the previews of the pen, pencil, marker,
//! highlighter, connector and eraser.

use figcraft_doc::Document;
use figcraft_engine::tools::{Overlay, PENCIL_STROKE};
use figcraft_engine::{DocState, Session, ToolId, ViewState, components};

use super::geometry::{alpha, at, bezier_handle, circle, cubic, node_quad, point, rect};
use super::{Anchor, COMPONENT, GUIDE, POINT, Rgba, SELECTION, Shape, WHITE};
use crate::view;

/// The marker's and the highlighter's strokes: weight (page units) and opacity, as `board.ink`
/// makes them (FigCraft's `boards/board.rs` keeps these private).
const MARKER: (f64, f32) = (4.0, 1.0);
const HIGHLIGHTER: (f64, f32) = (20.0, 0.45);

/// Item 7: the marquee, tinted 10 % with the selection colour, and outlined.
pub(super) fn marquee(out: &mut Vec<Shape>, ov: &Overlay, view: &ViewState) {
    let Some([x0, y0, x1, y1]) = ov.marquee else {
        return;
    };
    let points = rect(view, [x0, y0, x1, y1]);
    out.push(Shape::Fill {
        points: points.clone(),
        color: alpha(SELECTION, 0.1),
    });
    out.push(Shape::Outline {
        points,
        closed: true,
        color: SELECTION,
        width: 1.0,
        dashed: false,
    });
}

/// Item 8: the snap guides and measurements of the gesture under way: lines, dashed extension lines
/// and a small cross on a point snapped onto, then each distance on a pill centred on its line.
pub(super) fn guides(out: &mut Vec<Shape>, ov: &Overlay, view: &ViewState) {
    let mut distances = Vec::new();
    for g in &ov.guides {
        let (a, b) = (at(view, g.x0, g.y0), at(view, g.x1, g.y1));
        if g.x0 == g.x1 && g.y0 == g.y1 {
            for (dx, dy) in [(4.0, 4.0), (4.0, -4.0)] {
                out.push(Shape::Outline {
                    points: vec![(a.0 - dx, a.1 - dy), (a.0 + dx, a.1 + dy)],
                    closed: false,
                    color: GUIDE,
                    width: 1.5,
                    dashed: false,
                });
            }
        } else {
            out.push(Shape::Outline {
                points: vec![a, b],
                closed: false,
                color: GUIDE,
                width: 1.0,
                dashed: g.dashed,
            });
        }
        if let Some(text) = &g.label {
            distances.push(Shape::Label {
                at: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
                anchor: Anchor::Middle,
                text: text.clone(),
                color: WHITE,
                background: Some(GUIDE),
                node: None,
            });
        }
    }
    out.extend(distances);
}

/// Item 9: where a drag drops: the frame or section it goes into, outlined (a component's slot in
/// the component colour), and the line where it goes in an auto layout, 2 px wide.
pub(super) fn drops(out: &mut Vec<Shape>, ov: &Overlay, doc: &Document, view: &ViewState) {
    if let Some(id) = ov.drop_target
        && let Some(quad) = node_quad(doc, id, view)
    {
        let color = if components::is_slot(doc, id) {
            COMPONENT
        } else {
            SELECTION
        };
        out.push(Shape::Outline {
            points: quad.to_vec(),
            closed: true,
            color,
            width: 1.0,
            dashed: false,
        });
    }
    if let Some([x0, y0, x1, y1]) = ov.insert_line {
        out.push(Shape::Outline {
            points: vec![at(view, x0, y0), at(view, x1, y1)],
            closed: false,
            color: SELECTION,
            width: 2.0,
            dashed: false,
        });
    }
}

/// Item 10: what a tool is making: the stroke the pencil, marker or highlighter is drawing, in its
/// colour and weight (at least 1 px); the pen's points, their handles and the segment a click would
/// add; the connector being drawn, or the end of one being dragged; and the eraser's reach.
pub(super) fn previews(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let (ov, view) = (&s.tool.overlay, &ds.view);
    let points: Vec<(f64, f64)> = ov.pen.iter().map(|[x, y]| at(view, *x, *y)).collect();
    if !points.is_empty() {
        match stroke(s, &ds.doc) {
            Some((color, weight)) => out.push(Shape::Outline {
                points,
                closed: false,
                color,
                width: (weight * view.zoom).max(1.0),
                dashed: false,
            }),
            None => {
                for [x0, y0, x1, y1] in &ov.pen_handles {
                    let (vertex, end) = (at(view, *x0, *y0), at(view, *x1, *y1));
                    bezier_handle(out, vertex, end, POINT, WHITE);
                }
                for center in points {
                    out.push(point(center, POINT, WHITE));
                }
                if let Some([x0, y0, x1, y1]) = ov.rubber {
                    let (a, b) = (at(view, x0, y0), at(view, x1, y1));
                    // Along the last point's out handle, when it has one.
                    let points = match ov.rubber_handle {
                        Some([hx, hy]) => cubic([a, at(view, hx, hy), b, b]),
                        None => vec![a, b],
                    };
                    out.push(Shape::Outline {
                        points,
                        closed: false,
                        color: SELECTION,
                        width: 1.0,
                        dashed: false,
                    });
                }
            }
        }
    } else if let Some([x0, y0, x1, y1]) = ov.rubber
        && ds.vector_edit.is_none()
    {
        // From where the connector starts to the pointer. Vector edit mode dashes its own.
        out.push(Shape::Outline {
            points: vec![at(view, x0, y0), at(view, x1, y1)],
            closed: false,
            color: SELECTION,
            width: 1.5,
            dashed: false,
        });
    }
    if let Some([x, y, r]) = ov.brush {
        let ring = circle(at(view, x, y), r * view.zoom);
        for (color, width) in [(WHITE, 3.0), (SELECTION, 1.5)] {
            out.push(Shape::Outline {
                points: ring.clone(),
                closed: true,
                color,
                width,
                dashed: false,
            });
        }
    }
}

/// The stroke the current tool draws, which it shows while drawing: its colour as the screen shows
/// it, and its weight in page units. `None` for the Pen, whose path is a layer as it is drawn.
fn stroke(s: &Session, doc: &Document) -> Option<(Rgba, f64)> {
    let (color, (weight, opacity)) = match s.tool.tool {
        ToolId::Pencil => (PENCIL_STROKE.0, (PENCIL_STROKE.1, 1.0)),
        ToolId::Marker => (s.tool.board.marker, MARKER),
        ToolId::Highlighter => (s.tool.board.highlighter, HIGHLIGHTER),
        _ => return None,
    };
    let [r, g, b, a] = color.to_rgba8();
    let [r, g, b] = view::screen_rgb(doc.color_profile, [r, g, b]);
    Some((alpha([r, g, b, a], opacity), weight))
}
