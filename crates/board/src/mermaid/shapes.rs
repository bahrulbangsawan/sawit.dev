//! What no node or edge drew, as Pele drew it: board shapes, and lines through what it outlined.

use kurbo::{Point, Rect};
use roxmltree::Node;
use serde_json::json;

use super::geometry::{length, point_list, shape_box, subpaths};
use super::style::{declarations, has_class, is_icon, tag, visible};
use super::text::text_of;
use super::{Converter, MIN_SIDE, TRANSPARENT};
use crate::skeleton::Skeleton;

impl<'a, 'input> Converter<'a, 'input> {
    // --- Everything else ------------------------------------------------------------------------

    /// What no node or edge drew: a text, or a shape as it is drawn.
    pub(super) fn loose(&mut self, element: Node<'a, 'input>) {
        if self.is_done(element) || !visible(element) {
            return;
        }
        if is_icon(element) {
            let icon = self.icon(element);
            self.out.extend(icon);
            return;
        }
        self.done.insert(element.id());
        if element.has_tag_name("text") {
            if !text_of(element).is_empty() {
                let text = self.text(element);
                self.out.push(text);
            }
        } else if !self.is_mask(element) {
            let shapes = self.shape(element);
            self.out.extend(shapes);
        }
    }

    /// A shape as it is drawn: a rectangle, an ellipse or a diamond as a board shape of its own;
    /// a polygon or a path as lines, one per subpath, a closed one with its fill as its
    /// `backgroundColor`. The engine draws a line of more than two points as ink, which has no
    /// fill, so a closed one that is only filled (an arrowhead) is outlined in its fill instead.
    pub(super) fn shape(&mut self, shape: Node<'a, 'input>) -> Vec<Skeleton> {
        self.done.insert(shape.id());
        if let Some(kind) = container_kind(shape) {
            // A box with no area isn't drawn.
            let frame = shape_box(shape).filter(|b| b.width() > 0.0 && b.height() > 0.0);
            let rounded = is_rounded(shape, shape.parent_element());
            return frame
                .map(|frame| self.box_shape(shape, kind, frame, rounded))
                .into_iter()
                .collect();
        }
        let (background, stroke) = self.shape_colours(shape);
        subpaths(shape)
            .into_iter()
            .filter(|points| points.len() >= 2)
            .map(|points| {
                let (first, last) = (points[0], points[points.len() - 1]);
                let closed = points.len() > 2 && first.distance(last) < 0.5;
                let fill = if closed { &background } else { TRANSPARENT };
                let outline = if stroke == TRANSPARENT { fill } else { &stroke };
                line_through(&points, outline.into(), fill.into())
            })
            .collect()
    }

    /// A board shape of `kind` in `frame`, coloured as `shape` is.
    pub(super) fn box_shape(
        &self,
        shape: Node,
        kind: &str,
        frame: Rect,
        rounded: bool,
    ) -> Skeleton {
        let (background, stroke) = self.shape_colours(shape);
        Skeleton {
            kind: kind.into(),
            background_color: Some(background),
            stroke_color: Some(stroke),
            roundness: rounded.then(|| json!({"type": 3})),
            ..in_frame(frame)
        }
    }
}

/// A skeleton in `frame`, no side of it under the engine's smallest.
fn in_frame(frame: Rect) -> Skeleton {
    Skeleton {
        x: frame.x0,
        y: frame.y0,
        width: Some(frame.width().max(MIN_SIDE)),
        height: Some(frame.height().max(MIN_SIDE)),
        ..Skeleton::default()
    }
}

/// A rectangle nobody sees, which holds a label and is what edges join.
pub(super) fn invisible(frame: Rect) -> Skeleton {
    Skeleton {
        kind: "rectangle".into(),
        background_color: Some(TRANSPARENT.into()),
        stroke_color: Some(TRANSPARENT.into()),
        ..in_frame(frame)
    }
}

/// A line through `points`, which are in the diagram's coordinates: a connector through two of
/// them, ink through more.
pub(super) fn line_through(points: &[Point], stroke: String, fill: String) -> Skeleton {
    let first = points.first().copied().unwrap_or(Point::ORIGIN);
    Skeleton {
        kind: "line".into(),
        x: first.x,
        y: first.y,
        points: Some(
            points
                .iter()
                .map(|p| [p.x - first.x, p.y - first.y])
                .collect(),
        ),
        stroke_color: Some(stroke),
        background_color: Some(fill),
        ..Skeleton::default()
    }
}

/// The board shape that draws `shape` itself, if there is one.
pub(super) fn container_kind(shape: Node) -> Option<&'static str> {
    match tag(shape) {
        "rect" => Some("rectangle"),
        "circle" | "ellipse" => Some("ellipse"),
        "polygon" if is_diamond(shape) => Some("diamond"),
        _ => None,
    }
}

/// Whether polygon `shape` is a diamond: four corners, each on its box's middle line across or
/// down.
fn is_diamond(shape: Node) -> bool {
    let corners = point_list(shape.attribute("points").unwrap_or_default());
    if corners.len() != 4 {
        return false;
    }
    let (low, high) = corners.iter().fold(
        (
            Point::new(f64::INFINITY, f64::INFINITY),
            Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(low, high), p| {
            (
                Point::new(low.x.min(p.x), low.y.min(p.y)),
                Point::new(high.x.max(p.x), high.y.max(p.y)),
            )
        },
    );
    let middle = low.midpoint(high);
    corners
        .iter()
        .all(|p| (p.x - middle.x).abs() < 0.5 || (p.y - middle.y).abs() < 0.5)
}

/// Whether rectangle `shape` (in `group`) has round corners.
pub(super) fn is_rounded(shape: Node, group: Option<Node>) -> bool {
    let radius = shape
        .attribute("rx")
        .and_then(length)
        .is_some_and(|rx| rx > 0.0);
    // A radius of nothing (a theme's `--pele-radius` the diagram doesn't declare) rounds nothing.
    let styled = shape.attribute("style").is_some_and(|style| {
        declarations(style)
            .any(|(property, value)| property == "rx" && length(value).is_some_and(|rx| rx > 0.0))
    });
    let shaped = group.is_some_and(|g| has_class(g, &["pele-shape-rounded", "pele-shape-stadium"]));
    shape.has_tag_name("rect") && (radius || styled || shaped)
}
