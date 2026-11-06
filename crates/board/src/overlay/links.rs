//! Prototype connections, drawn while the Design panel shows its Prototype tab: from each layer
//! with an interaction to the frame its first action goes to, a curve that leaves the layer's
//! right edge and comes into the frame's left edge, with a dot at its start and an arrowhead at its
//! end (`crate::design::prototype::links`).

use figcraft_engine::Session;

use super::{Rgba, Shape};
use crate::design::prototype;

/// The connections' colour, FigCraft's prototype green.
pub const LINK: Rgba = [0x14, 0xAE, 0x5C, 0xFF];
/// How many straight pieces a curve is drawn with.
const STEPS: usize = 24;

/// The connections on the page on show, in logical px from the canvas's top-left.
pub fn prototype_links(s: &Session) -> Vec<Shape> {
    let Ok(ds) = s.ds() else {
        return Vec::new();
    };
    let view = &ds.view;
    let mut out = Vec::new();
    for link in prototype::links(&ds.doc, ds.page) {
        let [fx0, fy0, fx1, fy1] = link.from_box;
        let [tx0, ty0, tx1, ty1] = link.to_box;
        // Leave rightwards when the frame is to the right, else leftwards; come in on the near side.
        let rightwards = tx0 >= (fx0 + fx1) / 2.0;
        let start = view.page_to_screen(if rightwards { fx1 } else { fx0 }, (fy0 + fy1) / 2.0);
        let end = view.page_to_screen(if rightwards { tx0 } else { tx1 }, (ty0 + ty1) / 2.0);
        let reach = ((end.0 - start.0).abs() / 2.0).max(40.0);
        let sign = if rightwards { 1.0 } else { -1.0 };
        let (c1, c2) = (
            (start.0 + sign * reach, start.1),
            (end.0 - sign * reach, end.1),
        );
        let points: Vec<(f64, f64)> = (0..=STEPS)
            .map(|i| {
                let t = i as f64 / STEPS as f64;
                let u = 1.0 - t;
                let x = u * u * u * start.0
                    + 3.0 * u * u * t * c1.0
                    + 3.0 * u * t * t * c2.0
                    + t * t * t * end.0;
                let y = u * u * u * start.1
                    + 3.0 * u * u * t * c1.1
                    + 3.0 * u * t * t * c2.1
                    + t * t * t * end.1;
                (x, y)
            })
            .collect();
        if points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
            continue;
        }
        out.push(Shape::Outline {
            points,
            closed: false,
            color: LINK,
            width: 2.0,
            dashed: false,
        });
        out.push(Shape::Handle {
            center: start,
            size: 8.0,
            round: true,
            fill: LINK,
            stroke: [0xFF; 4],
        });
        let (x, y) = end;
        let back = -sign * 9.0;
        out.push(Shape::Fill {
            points: vec![(x, y), (x + back, y - 5.0), (x + back, y + 5.0)],
            color: LINK,
        });
    }
    out
}

/// Dev Mode's annotations, while it is on: each annotated layer outlined, and its notes in a pill at
/// its top-right corner, the first note and how many more.
pub fn annotation_callouts(s: &Session) -> Vec<Shape> {
    let Ok(ds) = s.ds() else {
        return Vec::new();
    };
    let (doc, view) = (&ds.doc, &ds.view);
    let mut out = Vec::new();
    for node in doc.nodes() {
        if node.annotations.is_empty() || doc.page_of(node.id) != Some(ds.page) {
            continue;
        }
        let Some(b) = doc.world_bounds(node.id) else {
            continue;
        };
        let corners = [(b.x0, b.y0), (b.x1, b.y0), (b.x1, b.y1), (b.x0, b.y1)]
            .map(|(x, y)| view.page_to_screen(x, y));
        out.push(Shape::Outline {
            points: corners.to_vec(),
            closed: true,
            color: ANNOTATION,
            width: 1.0,
            dashed: true,
        });
        let notes = crate::design::devmode::annotations(doc, node.id);
        let first = notes.first().cloned().unwrap_or_default();
        let text = match notes.len() {
            0 | 1 => first,
            n => format!("{first} (+{})", n - 1),
        };
        let (x, y) = corners[1];
        out.push(Shape::Label {
            at: (x + 6.0, y),
            anchor: super::Anchor::TopLeft,
            text,
            color: [0xFF; 4],
            background: Some(ANNOTATION),
            node: None,
        });
    }
    out
}

/// Annotations' colour: FigCraft's Development category, a deep purple.
const ANNOTATION: super::Rgba = [0x7B, 0x61, 0xFF, 0xFF];
