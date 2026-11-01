//! Geometry: lengths and number lists, transforms, the boxes and lines shapes draw, distances.

use kurbo::{Affine, BezPath, CubicBez, ParamCurve, PathEl, Point, QuadBez, Rect, Shape};
use roxmltree::Node;

use super::style::tag;
use super::{CURVE_STEPS, STRAIGHT};

// --- Geometry -------------------------------------------------------------------------------------

/// A length in px: `12` or `12px`.
pub(super) fn length(value: &str) -> Option<f64> {
    let value = value.trim();
    let number: f64 = value
        .strip_suffix("px")
        .unwrap_or(value)
        .trim()
        .parse()
        .ok()?;
    number.is_finite().then_some(number)
}

/// Attribute `name` of `node` as a length, 0 when it has none.
fn number(node: Node, name: &str) -> f64 {
    node.attribute(name).and_then(length).unwrap_or(0.0)
}

/// The first of the positions in attribute `name` (a text's `x` and `y` can list one a
/// character).
pub(super) fn coordinate(node: Node, name: &str) -> Option<f64> {
    numbers(node.attribute(name)?).first().copied()
}

/// The numbers in `text`, as SVG lists them: apart by spaces or commas, or run together where a
/// sign or a second point starts the next one (`10-5`, `.5.5`).
pub(super) fn numbers(text: &str) -> Vec<f64> {
    fn digits(bytes: &[u8], at: &mut usize) -> bool {
        let start = *at;
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
        *at > start
    }
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        if matches!(bytes[at], b'+' | b'-') {
            at += 1;
        }
        let mut whole = digits(bytes, &mut at);
        if bytes.get(at) == Some(&b'.') {
            at += 1;
            whole |= digits(bytes, &mut at);
        }
        if !whole {
            at = start + 1;
            continue;
        }
        if matches!(bytes.get(at), Some(b'e' | b'E')) {
            let mark = at;
            at += 1;
            if matches!(bytes.get(at), Some(b'+' | b'-')) {
                at += 1;
            }
            if !digits(bytes, &mut at) {
                at = mark;
            }
        }
        if let Ok(number) = text[start..at].parse::<f64>() {
            found.push(number);
        }
    }
    found
}

/// The points of a `points` attribute.
pub(super) fn point_list(text: &str) -> Vec<Point> {
    let numbers = numbers(text);
    let (pairs, _) = numbers.as_chunks::<2>();
    pairs.iter().map(|&[x, y]| Point::new(x, y)).collect()
}

/// An SVG `transform` list as one transform. A step that isn't one moves nothing.
fn parse_transform(text: &str) -> Affine {
    let mut transform = Affine::IDENTITY;
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')').map(|close| open + close) else {
            break;
        };
        let name = rest[..open].trim_matches(|c: char| c.is_whitespace() || c == ',');
        let step = match (name, numbers(&rest[open + 1..close]).as_slice()) {
            ("matrix", &[a, b, c, d, e, f]) => Affine::new([a, b, c, d, e, f]),
            ("translate", &[x]) => Affine::translate((x, 0.0)),
            ("translate", &[x, y]) => Affine::translate((x, y)),
            ("scale", &[s]) => Affine::scale(s),
            ("scale", &[x, y]) => Affine::scale_non_uniform(x, y),
            ("rotate", &[angle]) => Affine::rotate(angle.to_radians()),
            ("rotate", &[angle, x, y]) => Affine::rotate_about(angle.to_radians(), (x, y)),
            ("skewX", &[angle]) => Affine::skew(angle.to_radians().tan(), 0.0),
            ("skewY", &[angle]) => Affine::skew(0.0, angle.to_radians().tan()),
            _ => Affine::IDENTITY,
        };
        transform *= step;
        rest = &rest[close + 1..];
    }
    transform
}

/// The transform from `node`'s coordinates to the diagram's: its own `transform` and those of
/// the groups around it, down from the root (whose coordinates are the diagram's).
pub(super) fn transform_of(node: Node) -> Affine {
    node.ancestors()
        .filter(|n| n.is_element() && n.parent_element().is_some())
        .fold(Affine::IDENTITY, |inner, n| {
            n.attribute("transform")
                .map_or(Affine::IDENTITY, parse_transform)
                * inner
        })
}

/// The box around `shape` in its own coordinates, from its geometry.
fn local_box(shape: Node) -> Option<Rect> {
    let n = |name| number(shape, name);
    Some(match tag(shape) {
        "rect" => {
            let (width, height) = (
                shape.attribute("width").and_then(length)?,
                shape.attribute("height").and_then(length)?,
            );
            Rect::new(n("x"), n("y"), n("x") + width, n("y") + height)
        }
        "circle" => Rect::from_center_size((n("cx"), n("cy")), (2.0 * n("r"), 2.0 * n("r"))),
        "ellipse" => Rect::from_center_size((n("cx"), n("cy")), (2.0 * n("rx"), 2.0 * n("ry"))),
        "line" => Rect::from_points((n("x1"), n("y1")), (n("x2"), n("y2"))),
        // An icon: its viewport.
        "svg" => {
            let (width, height) = (
                shape.attribute("width").and_then(length)?,
                shape.attribute("height").and_then(length)?,
            );
            Rect::new(n("x"), n("y"), n("x") + width, n("y") + height)
        }
        "polygon" | "polyline" => {
            let points = point_list(shape.attribute("points")?);
            let first = *points.first()?;
            points
                .iter()
                .fold(Rect::from_points(first, first), |b, p| b.union_pt(*p))
        }
        "path" => BezPath::from_svg(shape.attribute("d")?)
            .ok()?
            .bounding_box(),
        _ => return None,
    })
}

/// The box around `shape` in the diagram's coordinates: its own box, through the transforms
/// around it.
pub(super) fn shape_box(shape: Node) -> Option<Rect> {
    Some(transform_of(shape).transform_rect_bbox(local_box(shape)?))
}

/// The lines `shape` draws, one list of points for each subpath, in the diagram's coordinates: a
/// path's curves are sampled, and a rectangle or an ellipse is its box.
pub(super) fn subpaths(shape: Node) -> Vec<Vec<Point>> {
    let n = |name| number(shape, name);
    let local = match tag(shape) {
        "line" => vec![vec![
            Point::new(n("x1"), n("y1")),
            Point::new(n("x2"), n("y2")),
        ]],
        tag @ ("polygon" | "polyline") => {
            let mut points = point_list(shape.attribute("points").unwrap_or_default());
            if let Some(first) = points.first().copied().filter(|_| tag == "polygon") {
                points.push(first);
            }
            vec![points]
        }
        "path" => shape
            .attribute("d")
            .and_then(|d| BezPath::from_svg(d).ok())
            .map(|path| sample(&path))
            .unwrap_or_default(),
        _ => local_box(shape)
            .map(|b| {
                let corners = [(b.x0, b.y0), (b.x1, b.y0), (b.x1, b.y1), (b.x0, b.y1)];
                let mut points: Vec<Point> = corners.into_iter().map(Point::from).collect();
                points.push(points[0]);
                vec![points]
            })
            .unwrap_or_default(),
    };
    let to_diagram = transform_of(shape);
    local
        .into_iter()
        .map(|points| dedupe(points.into_iter().map(|p| to_diagram * p)))
        .collect()
}

/// All the points `shape` draws through, one subpath after another.
pub(super) fn points(shape: Node) -> Vec<Point> {
    subpaths(shape).concat()
}

/// The points along `path`, one list for each subpath, its curves in [`CURVE_STEPS`] straight
/// steps each.
fn sample(path: &BezPath) -> Vec<Vec<Point>> {
    let steps = || (1..=CURVE_STEPS).map(|k| f64::from(k) / f64::from(CURVE_STEPS));
    let mut subpaths = Vec::new();
    let mut points: Vec<Point> = Vec::new();
    let (mut start, mut last) = (Point::ORIGIN, Point::ORIGIN);
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(p) => {
                if !points.is_empty() {
                    subpaths.push(std::mem::take(&mut points));
                }
                points.push(p);
                (start, last) = (p, p);
            }
            PathEl::LineTo(p) => {
                points.push(p);
                last = p;
            }
            PathEl::QuadTo(c, p) => {
                let curve = QuadBez::new(last, c, p);
                points.extend(steps().map(|t| curve.eval(t)));
                last = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let curve = CubicBez::new(last, c1, c2, p);
                points.extend(steps().map(|t| curve.eval(t)));
                last = p;
            }
            PathEl::ClosePath => {
                points.push(start);
                last = start;
            }
        }
    }
    if !points.is_empty() {
        subpaths.push(points);
    }
    subpaths
}

/// `points` without those that repeat the one before.
fn dedupe(points: impl IntoIterator<Item = Point>) -> Vec<Point> {
    let mut kept: Vec<Point> = Vec::new();
    for point in points {
        if kept.last().is_none_or(|last| last.distance(point) > 0.01) {
            kept.push(point);
        }
    }
    kept
}

/// How far `point` is from the outline of `frame`: 0 on it, and inside it too the distance to
/// the nearest side.
pub(super) fn distance_to_outline(point: Point, frame: Rect) -> f64 {
    let dx = (frame.x0 - point.x).max(0.0).max(point.x - frame.x1);
    let dy = (frame.y0 - point.y).max(0.0).max(point.y - frame.y1);
    if dx > 0.0 || dy > 0.0 {
        return dx.hypot(dy);
    }
    (point.x - frame.x0)
        .min(frame.x1 - point.x)
        .min(point.y - frame.y0)
        .min(frame.y1 - point.y)
}

/// How far `point` is from the line through `route`.
pub(super) fn distance_to_route(point: Point, route: &[Point]) -> f64 {
    route
        .windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let along = b - a;
            let length = along.hypot2();
            let t = if length > 0.0 {
                ((point - a).dot(along) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            point.distance(a + along * t)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Whether `route` runs straight from its first point to its last: none of it strays more than
/// [`STRAIGHT`] from the line between them.
pub(super) fn is_straight(route: &[Point]) -> bool {
    let (Some(first), Some(last)) = (route.first(), route.last()) else {
        return true;
    };
    let line = [*first, *last];
    route
        .iter()
        .all(|point| distance_to_route(*point, &line) <= STRAIGHT)
}

/// Whether the middle of `inner` is in `outer`.
pub(super) fn holds_middle(outer: Rect, inner: Rect) -> bool {
    let middle = inner.center();
    (outer.x0..=outer.x1).contains(&middle.x) && (outer.y0..=outer.y1).contains(&middle.y)
}
