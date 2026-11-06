//! The View menu's aids, drawn when their toggles are on (item 0 of [`super::overlay`], under
//! everything else): the frames' layout grids (`view.layoutGrids`, on by default), the pixel grid
//! once a page pixel is 8 screen px or more (`view.pixelGrid`), the frames' outlines
//! (`view.frameOutlines`), the masks' outlines (`view.maskOutlines`), the slices
//! (`view.slices`) and the rulers along the canvas's top and left edges with the page's guides
//! (`view.rulers`). Every aid is a fixed canvas colour, as the selection is.

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::{DocState, ViewState};
use kurbo::{Affine, Point};

use super::{Anchor, GUIDE, Rgba, Shape};

/// A frame's outline, and a slice's.
const FRAME_OUTLINE: Rgba = [0x99, 0x99, 0x99, 0xB0];
const SLICE: Rgba = [0x99, 0x99, 0x99, 0xFF];
/// A mask's outline.
const MASK: Rgba = [0x97, 0x47, 0xFF, 0xFF];
/// A comment's pin.
const PIN: Rgba = [0xFF, 0xC7, 0x00, 0xFF];
/// The pixel grid's lines.
const PIXEL: Rgba = [0x80, 0x80, 0x80, 0x40];
/// The rulers' band, ticks and numbers.
const RULER_BAND: Rgba = [0xF5, 0xF5, 0xF5, 0xF0];
const RULER_EDGE: Rgba = [0xD9, 0xD9, 0xD9, 0xFF];
const RULER_TEXT: Rgba = [0x6E, 0x6E, 0x6E, 0xFF];
/// How thick the rulers are (logical px).
pub const RULER: f64 = 18.0;
/// The zoom from which the pixel grid shows.
const PIXEL_GRID_ZOOM: f64 = 8.0;
/// The most lines one aid draws: past it, it is too dense to read.
const MOST_LINES: usize = 4000;

/// The aids that are on, back to front.
pub(super) fn aids(out: &mut Vec<Shape>, ds: &DocState) {
    if ds.focused().is_some() {
        return;
    }
    let (doc, view) = (&ds.doc, &ds.view);
    let page = ds.page;
    let layers = on_page(doc, page);
    if view.layout_grids {
        for id in layers.iter().copied() {
            layout_grids(out, doc, id, view);
        }
    }
    if view.pixel_grid && view.zoom >= PIXEL_GRID_ZOOM {
        pixel_grid(out, view);
    }
    for id in layers.iter().copied() {
        let Some(n) = doc.node(id) else { continue };
        let colour = match &n.kind {
            NodeKind::Slice if view.show_slices => Some((SLICE, true)),
            _ if view.mask_outlines && n.is_mask => Some((MASK, true)),
            kind if view.frame_outlines && kind.is_frame_like() => Some((FRAME_OUTLINE, false)),
            _ => None,
        };
        if let Some((color, dashed)) = colour
            && let Some(points) = quad(
                doc,
                id,
                Affine::IDENTITY,
                (0.0, 0.0, n.width, n.height),
                view,
            )
        {
            out.push(Shape::Outline {
                points,
                closed: true,
                color,
                width: 1.0,
                dashed,
            });
        }
    }
    if view.comments {
        for (_, (x, y), n) in crate::design::comments::pins(doc, page) {
            let (sx, sy) = view.page_to_screen(x, y);
            out.push(Shape::Handle {
                center: (sx, sy),
                size: 22.0,
                round: true,
                fill: PIN,
                stroke: [0xFF; 4],
            });
            out.push(Shape::Label {
                at: (sx - 3.5 * n.to_string().len() as f64, sy - 7.0),
                anchor: Anchor::TopLeft,
                text: n.to_string(),
                color: [0x1E, 0x1E, 0x1E, 0xFF],
                background: None,
                node: None,
            });
        }
    }
    if view.rulers {
        guides(out, doc, page, view);
        rulers(out, view);
    }
}

/// Every visible layer on `page`, parents before their children.
fn on_page(doc: &Document, page: NodeId) -> Vec<NodeId> {
    fn walk(doc: &Document, id: NodeId, depth: usize, out: &mut Vec<NodeId>) {
        let Some(n) = doc.node(id) else { return };
        if depth > 0 {
            if !n.visible {
                return;
            }
            out.push(id);
        }
        if depth < 64 {
            for child in &n.children {
                walk(doc, *child, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(doc, page, 0, &mut out);
    out
}

/// The screen quad of the box `(x0, y0, x1, y1)` in layer `id`'s own coordinates, moved by `local`
/// first.
fn quad(
    doc: &Document,
    id: NodeId,
    local: Affine,
    (x0, y0, x1, y1): (f64, f64, f64, f64),
    view: &ViewState,
) -> Option<Vec<(f64, f64)>> {
    doc.node(id)?;
    let xf = doc.world_transform(id) * local;
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| {
        let p = xf * Point::new(x, y);
        view.page_to_screen(p.x, p.y)
    });
    corners
        .iter()
        .all(|(x, y)| x.is_finite() && y.is_finite())
        .then(|| corners.to_vec())
}

/// Frame `id`'s visible layout grids: columns and rows as bands, a square grid as lines.
fn layout_grids(out: &mut Vec<Shape>, doc: &Document, id: NodeId, view: &ViewState) {
    let Some(n) = doc.node(id) else { return };
    if n.grids.is_empty() || !n.kind.is_frame_like() || !figcraft_layout::grids_shown(doc, id) {
        return;
    }
    for grid in n.grids.iter().filter(|g| g.visible) {
        let [r, g, b, a] = grid.color.to_rgba8();
        let color: Rgba = [r, g, b, a.max(24)];
        let lines = figcraft_layout::grid_lines(grid, n.width, n.height);
        for rect in lines.iter().take(MOST_LINES) {
            let Some(points) = quad(
                doc,
                id,
                Affine::IDENTITY,
                (rect.x0, rect.y0, rect.x1, rect.y1),
                view,
            ) else {
                continue;
            };
            if rect.width() < 1e-9 || rect.height() < 1e-9 {
                out.push(Shape::Outline {
                    points: vec![points[0], points[2]],
                    closed: false,
                    color: [r, g, b, a.saturating_mul(2).max(48)],
                    width: 1.0,
                    dashed: false,
                });
            } else {
                out.push(Shape::Fill { points, color });
            }
        }
    }
}

/// A line along every page pixel's edge across the canvas.
fn pixel_grid(out: &mut Vec<Shape>, view: &ViewState) {
    let (left, top) = view.screen_to_page(0.0, 0.0);
    let (right, bottom) = view.screen_to_page(view.vw, view.vh);
    let (mut x, mut y) = (left.floor(), top.floor());
    let mut lines = 0;
    while x <= right && lines < MOST_LINES {
        let (sx, _) = view.page_to_screen(x, 0.0);
        out.push(line((sx, 0.0), (sx, view.vh), PIXEL));
        x += 1.0;
        lines += 1;
    }
    while y <= bottom && lines < MOST_LINES {
        let (_, sy) = view.page_to_screen(0.0, y);
        out.push(line((0.0, sy), (view.vw, sy), PIXEL));
        y += 1.0;
        lines += 1;
    }
}

fn line(from: (f64, f64), to: (f64, f64), color: Rgba) -> Shape {
    Shape::Outline {
        points: vec![from, to],
        closed: false,
        color,
        width: 1.0,
        dashed: false,
    }
}

/// The ruler guides: the page's across the whole canvas, and each frame's across that frame.
fn guides(out: &mut Vec<Shape>, doc: &Document, page: NodeId, view: &ViewState) {
    if let Some(p) = doc.node(page) {
        for guide in &p.guides {
            out.push(if guide.vertical {
                let (sx, _) = view.page_to_screen(guide.offset, 0.0);
                line((sx, 0.0), (sx, view.vh), GUIDE)
            } else {
                let (_, sy) = view.page_to_screen(0.0, guide.offset);
                line((0.0, sy), (view.vw, sy), GUIDE)
            });
        }
    }
    for id in on_page(doc, page) {
        let Some(n) = doc.node(id).filter(|n| !n.guides.is_empty()) else {
            continue;
        };
        for guide in &n.guides {
            let edge = if guide.vertical {
                (guide.offset, 0.0, guide.offset, n.height)
            } else {
                (0.0, guide.offset, n.width, guide.offset)
            };
            if let Some(points) = quad(doc, id, Affine::IDENTITY, edge, view) {
                out.push(line(points[0], points[2], GUIDE));
            }
        }
    }
}

/// The spacing of the rulers' numbered ticks in page units: the first of 1, 2, 5, 10, 20, 50, …
/// that is 60 screen px or more apart.
fn tick_step(zoom: f64) -> f64 {
    let mut step = 1.0;
    for _ in 0..12 {
        for k in [1.0, 2.0, 5.0] {
            if step * k * zoom >= 60.0 {
                return step * k;
            }
        }
        step *= 10.0;
    }
    step
}

/// The rulers along the canvas's top and left edges, numbered in page units.
fn rulers(out: &mut Vec<Shape>, view: &ViewState) {
    let (w, h) = (view.vw, view.vh);
    out.push(Shape::Fill {
        points: vec![(0.0, 0.0), (w, 0.0), (w, RULER), (0.0, RULER)],
        color: RULER_BAND,
    });
    out.push(Shape::Fill {
        points: vec![(0.0, RULER), (RULER, RULER), (RULER, h), (0.0, h)],
        color: RULER_BAND,
    });
    out.push(line((0.0, RULER), (w, RULER), RULER_EDGE));
    out.push(line((RULER, RULER), (RULER, h), RULER_EDGE));
    let step = tick_step(view.zoom);
    if !step.is_finite() || step <= 0.0 {
        return;
    }
    let (left, top) = view.screen_to_page(RULER, RULER);
    let (right, bottom) = view.screen_to_page(w, h);
    let mut x = (left / step).ceil() * step;
    let mut count = 0;
    while x <= right && count < 400 {
        let (sx, _) = view.page_to_screen(x, 0.0);
        out.push(line((sx, RULER - 6.0), (sx, RULER), RULER_TEXT));
        out.push(Shape::Label {
            at: (sx + 3.0, 1.0),
            anchor: Anchor::TopLeft,
            text: number(x),
            color: RULER_TEXT,
            background: None,
            node: None,
        });
        x += step;
        count += 1;
    }
    let mut y = (top / step).ceil() * step;
    count = 0;
    while y <= bottom && count < 400 {
        let (_, sy) = view.page_to_screen(0.0, y);
        out.push(line((RULER - 6.0, sy), (RULER, sy), RULER_TEXT));
        out.push(Shape::Label {
            at: (2.0, sy + 2.0),
            anchor: Anchor::TopLeft,
            text: number(y),
            color: RULER_TEXT,
            background: None,
            node: None,
        });
        y += step;
        count += 1;
    }
}

fn number(n: f64) -> String {
    let n = n + 0.0;
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n:.1}")
    }
}

#[cfg(test)]
mod tests;
