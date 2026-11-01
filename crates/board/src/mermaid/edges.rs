//! Edges: connectors joined to the nodes at their ends, with their labels and the marks they keep.

use kurbo::Point;
use roxmltree::Node;

use super::geometry::{
    distance_to_outline, distance_to_route, is_straight, points, shape_box, subpaths,
};
use super::shapes::line_through;
use super::style::{has_class, tag};
use super::text::{text_of, text_place};
use super::{BIND_DISTANCE, Converter, ON_THE_ROUTE, ROUTES, TRANSPARENT};
use crate::skeleton::{End, Label, Skeleton};

impl<'a, 'input> Converter<'a, 'input> {
    // --- Edges ----------------------------------------------------------------------------------

    /// An edge: a connector from the start of its route to the end, joined to the nodes there,
    /// with its label.
    ///
    /// A connector has one arrowhead, at its end. An edge whose only arrowhead is at its start is
    /// drawn the other way round, so the arrowhead stays where it points; one with none is a line.
    /// The marks a connector has no cap for (a second arrowhead, the circles, diamonds and crosses
    /// of class diagrams and flowcharts, the bars and crow's feet of entity relationships) stay
    /// drawn as Pele drew them. So does an edge whose route bends and whose ends join nothing, or
    /// one node at both ends (a loop back to it): a connector would run straight across where it
    /// loops, and can't join one layer at both ends.
    pub(super) fn edge(&mut self, edge: Node<'a, 'input>) {
        if self.is_done(edge) {
            return;
        }
        self.done.insert(edge.id());
        // An edge is a group (its route and its marks) or, in mindmaps and git graphs, the bare
        // route.
        let route = if ROUTES.contains(&tag(edge)) {
            Some(edge)
        } else {
            edge.descendants()
                .skip(1)
                .find(|n| ROUTES.contains(&tag(*n)) && !has_class(*n, &["pele-marker"]))
        };
        let Some(route) = route else {
            return;
        };
        let mut points = points(route);
        self.done.insert(route.id());
        if points.len() < 2 {
            return;
        }
        // Each mark belongs to the end it is nearer.
        let (start, end) = (points[0], points[points.len() - 1]);
        let (mut at_start, mut at_end) = (Vec::new(), Vec::new());
        for mark in edge
            .descendants()
            .skip(1)
            .filter(|n| has_class(*n, &["pele-marker"]))
        {
            let Some(middle) = shape_box(mark).map(|b| b.center()) else {
                continue;
            };
            if middle.distance(start) < middle.distance(end) {
                at_start.push(mark);
            } else {
                at_end.push(mark);
            }
        }
        // Pele stops the line where a mark at its end begins: the end reaches the mark's tip again.
        if !at_end.is_empty() {
            let last = points.len() - 1;
            points[last] = reach(&at_end, points[last - 1], points[last]);
        }
        if !at_start.is_empty() {
            points[0] = reach(&at_start, points[1], points[0]);
        }
        let stroke = self.stroke_colour(route);
        let label = self.edge_label(edge);
        let message = has_class(edge, &["pele-message"]);
        let mut from = self.target(points[0], message);
        let mut to = self.target(points[points.len() - 1], message);
        // A loop from a node back to it ends where it starts, and a connector can't join one
        // layer at both ends: it joins neither, so that it is drawn as Pele drew it.
        if from.is_some() && from == to {
            (from, to) = (None, None);
        }
        if from.is_none() && to.is_none() && !is_straight(&points) {
            self.out
                .push(line_through(&points, stroke, TRANSPARENT.into()));
            if let Some(text) = label {
                let text = self.text(text);
                self.out.push(text);
            }
            return;
        }
        at_start.retain(|mark| is_arrowhead(*mark));
        at_end.retain(|mark| is_arrowhead(*mark));
        let head = if at_end.is_empty() && !at_start.is_empty() {
            points.reverse();
            std::mem::swap(&mut from, &mut to);
            at_start
        } else {
            at_end
        };
        for mark in &head {
            self.done.insert(mark.id());
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        let mut connector = Skeleton {
            kind: if head.is_empty() { "line" } else { "arrow" }.into(),
            x: first.x,
            y: first.y,
            // A connector runs from one end to the other, and the engine routes it.
            points: Some(vec![[0.0, 0.0], [last.x - first.x, last.y - first.y]]),
            stroke_color: Some(stroke),
            start: from.map(joined),
            end: to.map(joined),
            ..Skeleton::default()
        };
        let Some(text) = label else {
            self.out.push(connector);
            return;
        };
        // On the line (flowcharts): the connector's own label. Beside it (a sequence message's sits
        // above it): a text where Pele put it.
        let middle = text_place(text).bounds().center();
        if distance_to_route(middle, &points) < ON_THE_ROUTE {
            connector.label = Some(Label {
                text: text_of(text),
                font_size: None,
            });
            self.out.push(connector);
        } else {
            self.out.push(connector);
            let text = self.text(text);
            self.out.push(text);
        }
    }

    /// The text of the label Pele drew for `edge`, if it has one. Its group, the text and what
    /// hides the line behind it, is taken as drawn.
    fn edge_label(&mut self, edge: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
        let id = edge.attribute("data-id")?;
        let group = self.labels.get(id).copied()?;
        let text = group
            .descendants()
            .find(|n| n.has_tag_name("text"))
            .filter(|text| !text_of(*text).is_empty())?;
        for part in group.descendants() {
            self.done.insert(part.id());
        }
        Some(text)
    }

    /// The id of the node outline (or cluster) that `end` touches, the smallest if it touches
    /// several.
    ///
    /// The end of a sequence `message` joins no note. A message runs from lifeline to lifeline,
    /// or to the box of a participant it makes, never to a note; but Pele puts a note just under
    /// the message before it, near enough to join.
    fn target(&self, end: Point, message: bool) -> Option<String> {
        self.targets
            .iter()
            .filter(|target| !(message && target.note))
            .filter(|target| distance_to_outline(end, target.frame) <= BIND_DISTANCE)
            .min_by(|a, b| a.frame.area().total_cmp(&b.frame.area()))
            .map(|target| target.id.clone())
    }
}

/// An edge's end that joins skeleton `id`.
fn joined(id: String) -> End {
    End {
        id: Some(id),
        ..End::default()
    }
}

/// Whether `mark` (one of an edge's) is an arrowhead, which a connector has a cap for: an open or
/// a closed triangle drawn as a path or a polygon. On an entity relationship an open one is a
/// crow's foot.
fn is_arrowhead(mark: Node) -> bool {
    let closed = match tag(mark) {
        "polygon" => true,
        "polyline" => false,
        "path" => mark.attribute("d").is_some_and(|d| d.contains(['Z', 'z'])),
        _ => return false,
    };
    let subpaths = subpaths(mark);
    if closed {
        // Three corners: a triangle; four: a diamond.
        return subpaths
            .first()
            .is_some_and(|points| points.len().saturating_sub(1) != 4);
    }
    // Open strokes: a lone segment is a bar (or half a cross), a "<" an arrow.
    let relationship = mark
        .ancestors()
        .any(|n| n.attribute("class").is_some_and(|c| c.contains("pele-er")));
    !relationship && subpaths.iter().any(|points| points.len() > 2)
}

/// Where the end of a route whose last step runs from `from` to `end` goes with `marks` at it:
/// on along that step as far as their tip, when that lies beyond the end. An arrowhead's tip is
/// where it points, so the end reaches it; a mark that sits on the line (an entity
/// relationship's bars and crow's feet) leaves the end where it is, and one that sits across the
/// end (a lost message's cross) takes it only as far as its own far side.
fn reach(marks: &[Node], from: Point, end: Point) -> Point {
    let along = end - from;
    let length = along.hypot2();
    if length <= 0.0 {
        return end;
    }
    let beyond = (tip(marks, from) - from).dot(along) / length;
    if beyond > 1.0 {
        from + along * beyond
    } else {
        end
    }
}

/// The point of `marks` farthest from `from`: where an arrowhead points.
fn tip(marks: &[Node], from: Point) -> Point {
    let mut best = (from, -1.0);
    for point in marks.iter().flat_map(|mark| points(*mark)) {
        let far = point.distance(from);
        if far > best.1 {
            best = (point, far);
        }
    }
    best.0
}
