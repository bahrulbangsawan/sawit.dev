//! The geometry the overlay's parts share: a layer's box on screen, page points and rectangles on
//! screen, curves and circles as lines, a colour with less alpha, and the round points and Bézier
//! handles of the pen and of vector edit mode.

use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use figcraft_geom::Point;
use figcraft_geom::kurbo::{self, CubicBez, PathEl};

use super::{Rgba, SELECTION, Shape};

/// A Bézier handle from its point `vertex` to its `end`: the line, and a dot `size` across filled
/// with `fill`.
pub(super) fn bezier_handle(
    out: &mut Vec<Shape>,
    vertex: (f64, f64),
    end: (f64, f64),
    size: f64,
    fill: Rgba,
) {
    out.push(Shape::Outline {
        points: vec![vertex, end],
        closed: false,
        color: SELECTION,
        width: 1.0,
        dashed: false,
    });
    out.push(point(end, size, fill));
}

/// A round point `size` across at `center`, filled with `fill`.
pub(super) fn point(center: (f64, f64), size: f64, fill: Rgba) -> Shape {
    Shape::Handle {
        center,
        size,
        round: true,
        fill,
        stroke: SELECTION,
    }
}

/// Layer `id`'s box on screen: its corners (top-left, top-right, bottom-right, bottom-left) turned
/// and flipped with it.
pub(super) fn node_quad(doc: &Document, id: NodeId, view: &ViewState) -> Option<[(f64, f64); 4]> {
    let n = doc.node(id)?;
    let world = doc.world_transform(id);
    let corners = [
        (0.0, 0.0),
        (n.width, 0.0),
        (n.width, n.height),
        (0.0, n.height),
    ];
    Some(corners.map(|(x, y)| screen(view, world * Point::new(x, y))))
}

/// The page rectangle `[x0, y0, x1, y1]`'s corners on screen (top-left, top-right, bottom-right,
/// bottom-left).
pub(super) fn rect(view: &ViewState, [x0, y0, x1, y1]: [f64; 4]) -> Vec<(f64, f64)> {
    vec![
        at(view, x0, y0),
        at(view, x1, y0),
        at(view, x1, y1),
        at(view, x0, y1),
    ]
}

/// The smallest and largest x and y of `points`.
pub(super) fn extent(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    )
}

/// The cubic Bézier through screen points `[start, control, control, end]`, as a line to within a
/// quarter pixel.
pub(super) fn cubic([a, b, c, d]: [(f64, f64); 4]) -> Vec<(f64, f64)> {
    flat(CubicBez::new(a, b, c, d))
}

/// A circle on screen around `center`, `radius` px, as a closed line to within a quarter pixel.
/// No points for a radius that isn't positive.
pub(super) fn circle(center: (f64, f64), radius: f64) -> Vec<(f64, f64)> {
    if !(radius > 0.0 && radius.is_finite()) {
        return Vec::new();
    }
    flat(kurbo::Circle::new(center, radius))
}

/// The points of `shape`'s outline (one piece) to within a quarter pixel.
fn flat(shape: impl kurbo::Shape) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    kurbo::flatten(shape.path_elements(0.1), 0.25, |el| {
        if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el {
            points.push((p.x, p.y));
        }
    });
    points
}

pub(super) fn screen(view: &ViewState, p: Point) -> (f64, f64) {
    view.page_to_screen(p.x, p.y)
}

pub(super) fn at(view: &ViewState, x: f64, y: f64) -> (f64, f64) {
    view.page_to_screen(x, y)
}

/// `color` with its alpha scaled by `opacity`.
pub(super) fn alpha([r, g, b, a]: Rgba, opacity: f32) -> Rgba {
    [r, g, b, (f32::from(a) * opacity).round() as u8]
}

/// `v` to the nearest whole number.
pub(super) fn whole(v: f64) -> i64 {
    v.round() as i64
}
