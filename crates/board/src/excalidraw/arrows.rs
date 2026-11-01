//! Where an arrow or a line runs: its ends, joined or free, and which way its heads point.

use std::collections::HashMap;

use serde_json::Value;

use super::fields::{finite, number};
use super::items::Kind;
use super::{Fields, HEAD_ANGLE, HEAD_LENGTH};
use crate::skeleton::{End, Skeleton};

/// One end of an arrow or a line: the element named in its `binding` when `joinable` has it and
/// the line doesn't run beside it, else the end is free, where it is drawn. `route` is the line's
/// points from that end inwards, relative to `origin`, the line's own place.
///
/// A line runs beside the element it is bound to when Excalidraw says so and the drawing agrees.
/// Excalidraw's `focus` says where the line, carried on past its end, meets the element: from -1
/// to 1 across it, so a line with a focus outside that runs beside it, as a sequence message of
/// the old canvas runs beside the note under it. Excalidraw draws that end where it is; joined,
/// the end would be moved onto the element. The drawing must agree ([`misses`]), because the focus
/// can be wrong: Excalidraw 0.18 measures a line into a diamond against the wrong half of it, so
/// one straight into a wide diamond can read 1.6. And Excalidraw must say so, because a line that
/// misses what it is bound to while its focus says it meets it (or that has no focus) is still
/// joined to it: a route that the old canvas left behind when a script moved its shape (it moved
/// only two-point arrows along), and still listed as joined.
pub(super) fn end_of<'p>(
    fields: &Fields,
    binding: &str,
    mut route: impl Iterator<Item = &'p [f64; 2]>,
    origin: (f64, f64),
    joinable: &HashMap<&str, &Fields>,
) -> End {
    let point = route.next();
    let binding = fields.get(binding);
    let bound = binding
        .and_then(|binding| binding.get("elementId"))
        .and_then(Value::as_str)
        .and_then(|id| Some((id, *joinable.get(id)?)));
    let Some((id, element)) = bound else {
        return free_at(point, origin);
    };
    let said_beside = binding
        .and_then(|binding| binding.get("focus"))
        .and_then(finite)
        .is_some_and(|focus| focus.abs() > 1.0);
    // The point the line comes from on its way to its end, the step that it is carried on along.
    let from = point.and_then(|end| route.find(|other| *other != end));
    let on_page = |p: &[f64; 2]| [origin.0 + p[0], origin.1 + p[1]];
    let beside = said_beside
        && point
            .zip(from)
            .is_some_and(|(end, from)| misses(element, on_page(from), on_page(end)));
    if beside {
        return free_at(point, origin);
    }
    End {
        id: Some(id.to_string()),
        ..End::default()
    }
}

/// Whether a line that runs from `from` to `to` (points of the page), carried on past `to`,
/// misses the box of `element`, as Excalidraw draws it: turned by its `angle` about its centre. A
/// line along an edge meets it. `false` for an element with no size, of which nothing can be said.
///
/// It is the box even for an ellipse or a diamond, as Excalidraw measures the focus of every shape
/// but a diamond: a line that meets the box near one of its corners passes close to the shape, and
/// an end bound there is joined.
fn misses(element: &Fields, from: [f64; 2], to: [f64; 2]) -> bool {
    let (Some(width), Some(height)) = (number(element, "width"), number(element, "height")) else {
        return false;
    };
    let centre = [
        number(element, "x").unwrap_or(0.0) + width / 2.0,
        number(element, "y").unwrap_or(0.0) + height / 2.0,
    ];
    // In the element's own frame, where its box stands upright about the origin, as Excalidraw
    // measures a focus: the points turned back by its angle.
    let (sin, cos) = (-number(element, "angle").unwrap_or(0.0)).sin_cos();
    let upright = |point: [f64; 2]| {
        let (x, y) = (point[0] - centre[0], point[1] - centre[1]);
        [x * cos - y * sin, x * sin + y * cos]
    };
    let (end, back) = (upright(to), upright(from));
    let way = [end[0] - back[0], end[1] - back[1]];
    let half = [width.abs() / 2.0, height.abs() / 2.0];
    // The stretch of the line past its end, end + t × way for t from 0 on, that is inside the box
    // along each axis in turn; it meets the box when the two overlap.
    let (mut near, mut far) = (0.0_f64, f64::INFINITY);
    for ((end, way), half) in end.into_iter().zip(way).zip(half) {
        if way == 0.0 {
            if end.abs() > half {
                return true;
            }
        } else {
            let (one, other) = ((-half - end) / way, (half - end) / way);
            near = near.max(one.min(other));
            far = far.min(one.max(other));
        }
    }
    near > far
}

/// A free end of a line, at `point`, which is relative to `origin`, the line's own place.
pub(super) fn free_at(point: Option<&[f64; 2]>, origin: (f64, f64)) -> End {
    End {
        id: None,
        x: point.map(|p| origin.0 + p[0]),
        y: point.map(|p| origin.1 + p[1]),
    }
}

/// Where an arrow has a head, which is which way it points. A skeleton's `arrow` has one, at its
/// end, and its `line` has none; so the kind of head, and a second head at the start, are not
/// carried.
pub(super) enum Heads {
    /// A head at the end, or at both ends: an arrow as it is.
    End,
    /// A head at the start only: an arrow that points back, from its end to its start.
    Start,
    /// No head at either end: a line.
    Neither,
}

impl Heads {
    /// The heads that an arrow element has, as Excalidraw draws them: `null` in `startArrowhead`
    /// or `endArrowhead` is no head, and any other value is a head of some kind. A field that
    /// isn't there is what Excalidraw makes of it (`restoreElements`, which the canvas runs on
    /// every scene it loads, and its renderer): no head at the start, one at the end.
    pub(super) fn of(fields: &Fields) -> Self {
        let head =
            |key: &str, missing: bool| fields.get(key).map_or(missing, |kind| !kind.is_null());
        match (head("startArrowhead", false), head("endArrowhead", true)) {
            (_, true) => Self::End,
            (true, false) => Self::Start,
            (false, false) => Self::Neither,
        }
    }
}

/// Makes `item`, the skeleton of an arrow whose only head is at its start, the arrow that runs
/// the other way: the same route from its end to its start, with the same things joined (the ends
/// change places), and the head at the end it points to.
///
/// The points of a skeleton are relative to its `x`, `y`, which is where it starts. The turned
/// arrow starts where the old one ended, so `x`, `y` move to the old last point, and the points
/// are the old ones in reverse, relative to it.
///
/// An arrow with fewer than two points has no route to turn around, and stays as it is.
pub(super) fn turn_around(item: &mut Skeleton) {
    let Some(points) = item.points.as_mut().filter(|points| points.len() >= 2) else {
        return;
    };
    let [dx, dy] = points[points.len() - 1];
    points.reverse();
    for point in points.iter_mut() {
        *point = [point[0] - dx, point[1] - dy];
    }
    item.x += dx;
    item.y += dy;
    std::mem::swap(&mut item.start, &mut item.end);
}

/// Makes `item`, the skeleton of an arrow with no head, a `line`, as Excalidraw draws it.
///
/// A line through more than two points is ink, which joins nothing and has no text. An arrow that
/// joins something or has a label would lose that, so it keeps only its first and last point and
/// stays a connector between them. That is the case of the links that Mermaid diagrams made on the
/// old canvas, written with the points sampled along the route Pele drew. One that joins nothing
/// and has no label keeps its bends, and is ink, as a line is.
pub(super) fn without_heads(item: &mut Skeleton) {
    item.kind = Kind::Line.skeleton().to_string();
    let joins = [&item.start, &item.end]
        .into_iter()
        .flatten()
        .any(|end| end.id.is_some());
    if !joins && item.label.is_none() {
        return;
    }
    if let Some(points) = item.points.as_mut().filter(|points| points.len() > 2) {
        let (first, last) = (points[0], points[points.len() - 1]);
        *points = vec![first, last];
    }
}

/// Makes `item`, the skeleton of an arrow that loops back to the element it starts from, its
/// route drawn as Excalidraw draws it: ink, which joins nothing, through every point of the route,
/// with its head drawn in it at the end ([`head_at_end`]), since ink has no cap. A connector would
/// run straight across where it loops ([`Item::run_between`]).
///
/// An arrow with no bend to draw, two points or fewer, stays as it is: a connector between its
/// ends, which are free.
pub(super) fn drawn_along_its_route(item: &mut Skeleton) {
    let Some(route) = item.points.as_mut().filter(|route| route.len() > 2) else {
        return;
    };
    if let Some(head) = head_at_end(route) {
        route.extend(head);
    }
    item.kind = Kind::Line.skeleton().to_string();
}

/// The outline of the head that Excalidraw draws at the end of `route` for a `triangle` head:
/// from the tip along the route's last step, [`HEAD_ANGLE`] to each side of it, and back to the
/// tip. Its sides are as long as half of that step, and at most [`HEAD_LENGTH`]. `None` when the
/// route has no step.
fn head_at_end(route: &[[f64; 2]]) -> Option<[[f64; 2]; 3]> {
    let tip = *route.last()?;
    let before = route.iter().rev().find(|point| **point != tip)?;
    let back = [before[0] - tip[0], before[1] - tip[1]];
    let step = back[0].hypot(back[1]);
    let length = (step / 2.0).min(HEAD_LENGTH);
    let (along_x, along_y) = (back[0] / step * length, back[1] / step * length);
    let (sin, cos) = HEAD_ANGLE.to_radians().sin_cos();
    let side = |turn: f64| {
        [
            tip[0] + along_x * cos - along_y * sin * turn,
            tip[1] + along_x * sin * turn + along_y * cos,
        ]
    };
    Some([side(1.0), side(-1.0), tip])
}
