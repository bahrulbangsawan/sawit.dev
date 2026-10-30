//! Mermaid diagrams as board shapes that can be edited. [`to_skeletons`] reads the SVG that
//! `sawit_core::mermaid::render` draws with Pele and makes [`Skeleton`]s of it for
//! [`crate::skeleton::apply`]: a node becomes a rectangle, an ellipse or a diamond with its label
//! inside, an edge a connector joined to the nodes at its ends, with its label, and whatever else
//! the diagram draws comes in as it is drawn. Charts (pie, gantt, ...) have no shapes worth
//! editing and come in as one SVG picture.
//!
//! It is the converter of `web/canvas/src/pele.ts`, which read the same SVG laid out in a browser,
//! ported to the SVG's own markup with the same passes: clusters, then nodes, then edges, then
//! what is left. Styles are resolved down the tree as a browser resolves them, transforms are
//! composed with `kurbo`, and a shape's box comes from its geometry. A text has no box until
//! something lays it out, so its place is its `x`/`y` and its `text-anchor`; where a width is
//! needed it is estimated, because the engine sets the text again in its own font.
//!
//! A board holds less than an Excalidraw scene, so some of what pele.ts kept is left out: line
//! widths and dashes, opacity, the size and colour of the text inside a shape, and groups. A
//! connector has one arrowhead, at its end, so the marks it has no cap for stay drawn where Pele
//! drew them. An edge whose route bends and whose ends join nothing, or one node at both ends (a
//! message to oneself, a loop back to a node), stays drawn along its route.

use std::collections::{HashMap, HashSet};

use figcraft_color::Color;
use figcraft_doc::{BoardShape, TextData};
use kurbo::{Affine, BezPath, CubicBez, ParamCurve, PathEl, Point, QuadBez, Rect, Shape};
use roxmltree::{Document, Node, NodeId as XmlId};
use sawit_core::mermaid::{Diagram, Palette};
use serde_json::json;

use crate::skeleton::{End, Label, Skeleton};

/// The diagram types that come in as shapes that can be edited. Any other (pie, gantt, ...) comes
/// in as one picture.
pub const EDITABLE: &[&str] = &[
    "flowchart",
    "sequence",
    "class",
    "state",
    "er",
    "mindmap",
    "requirement",
    "block",
    "kanban",
    "c4",
    "gitGraph",
    "timeline",
];

/// The elements Pele draws shapes with.
const SHAPES: &[&str] = &[
    "rect", "circle", "ellipse", "polygon", "polyline", "path", "line",
];
/// The elements an edge's route can be.
const ROUTES: &[&str] = &["path", "line", "polyline"];
/// Elements whose content isn't drawn where it is written.
const NOT_DRAWN: &[&str] = &[
    "defs",
    "clipPath",
    "mask",
    "marker",
    "pattern",
    "symbol",
    "linearGradient",
    "radialGradient",
];

/// The colour that a line or a text in one of Pele's neutral colours comes in as.
const INK: &str = "#1e1e1e";
/// The fill that Pele's second surface colour comes in as.
const SURFACE_ALT_FILL: &str = "#e9ecef";
const TRANSPARENT: &str = "transparent";

/// How far an edge's end may sit from a node's outline and still join it.
const BIND_DISTANCE: f64 = 14.0;
/// How near its route the middle of an edge's label must be to be the connector's own label.
const ON_THE_ROUTE: f64 = 6.0;
/// How far a route may stray from the line between its ends and still run straight.
const STRAIGHT: f64 = 1.0;
/// The straight steps a curve is sampled in.
const CURVE_STEPS: u32 = 8;
/// The font size of a text that names none (the browser's).
const FONT_SIZE: f64 = 16.0;
/// Pele sets a line of text with its baseline this far below the line's middle, in ems.
const BASELINE: f64 = 0.35;
/// The height of a line of text, in ems.
const LINE_HEIGHT: f64 = 1.2;
/// The size the engine sets a board shape's label in.
const LABEL_SIZE: f64 = 16.0;
/// The height of a line of a label, in ems, with room to spare.
const LABEL_LINE_HEIGHT: f64 = 1.25;
/// The room around the text of a node that is only text (pele.ts `pad`).
const TEXT_PAD: f64 = 4.0;
/// The smallest side the engine takes for a shape.
const MIN_SIDE: f64 = 1.0;

/// The shapes that draw `diagram`, in the diagram's own coordinates (0, 0 is its top left).
///
/// A diagram of a type in [`EDITABLE`] comes in as shapes. It is read as rendered with
/// `Palette::default()`, whose neutral colours become the canvas's own: unfilled shapes, ink lines
/// and ink text. The nodes that edges can join get the ids `mermaid-1`, `mermaid-2`..., for the
/// edges to name. Any other type comes in as one `image` skeleton of its SVG, `diagram.svg`, as
/// big as it is drawn.
///
/// The error is for an SVG that can't be read.
pub fn to_skeletons(diagram: &Diagram) -> Result<Vec<Skeleton>, String> {
    if !EDITABLE.contains(&diagram.kind.as_str()) {
        return Ok(vec![picture(diagram)]);
    }
    let svg = Document::parse(&diagram.svg)
        .map_err(|error| format!("the diagram's SVG can't be read ({error})"))?;
    Ok(Converter::new(&svg).run())
}

/// The whole diagram as one picture.
fn picture(diagram: &Diagram) -> Skeleton {
    Skeleton {
        kind: "image".into(),
        svg: Some(diagram.svg.clone()),
        name: Some("diagram.svg".into()),
        width: Some(diagram.width),
        height: Some(diagram.height),
        ..Skeleton::default()
    }
}

/// What the colours of Pele's palette mean: its neutral ones carry no colour of their own.
struct Neutrals {
    /// A line or a text in one of these comes in as ink.
    strokes: Vec<String>,
    /// A shape filled with one of these comes in unfilled.
    fills: Vec<String>,
    /// The second surface, which comes in as a light grey of the canvas's own.
    surface_alt: Option<String>,
    /// The page: a rectangle of it with no outline only hides what is behind a label.
    background: Option<String>,
}

impl Neutrals {
    fn of(palette: &Palette) -> Self {
        let colours = |colours: &[&String]| colours.iter().filter_map(|c| hex(c)).collect();
        Self {
            strokes: colours(&[&palette.foreground, &palette.line, &palette.border]),
            fills: colours(&[&palette.surface, &palette.background]),
            surface_alt: hex(&palette.surface_alt),
            background: hex(&palette.background),
        }
    }
}

/// A conversion in progress.
struct Converter<'a, 'input> {
    root: Node<'a, 'input>,
    neutrals: Neutrals,
    /// The groups of the edge labels, by the id of their edge.
    labels: HashMap<&'a str, Node<'a, 'input>>,
    /// The elements already drawn, or drawn as part of something else.
    done: HashSet<XmlId>,
    /// What an edge can join: each node and cluster.
    targets: Vec<Target>,
    out: Vec<Skeleton>,
}

/// A node or a cluster that an edge can join.
struct Target {
    /// The id of its skeleton.
    id: String,
    /// Its outline as Pele drew it.
    frame: Rect,
    /// Whether it is a note, which no sequence message ends at.
    note: bool,
}

impl<'a, 'input> Converter<'a, 'input> {
    fn new(svg: &'a Document<'input>) -> Self {
        let root = svg.root_element();
        let mut labels = HashMap::new();
        for group in root
            .descendants()
            .filter(|n| has_class(*n, &["pele-edge-label"]))
        {
            if let Some(id) = group.attribute("data-id") {
                labels.entry(id).or_insert(group);
            }
        }
        Self {
            root,
            neutrals: Neutrals::of(&Palette::default()),
            labels,
            done: HashSet::new(),
            targets: Vec::new(),
            out: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<Skeleton> {
        // The page that `flatten` paints behind the diagram is no shape of it.
        let first = self.root.children().find(Node::is_element);
        if let Some(background) = first.filter(|n| is_background(*n)) {
            self.done.insert(background.id());
        }
        let elements: Vec<Node> = self
            .root
            .descendants()
            .skip(1)
            .filter(Node::is_element)
            .collect();
        // Clusters first, so that they sit under their nodes; nodes before edges, so that edges
        // can join them.
        let clusters = ["pele-cluster", "pele-frame", "pele-column"];
        for group in elements.iter().filter(|n| has_class(**n, &clusters)) {
            self.group(*group, false);
        }
        let nodes = ["pele-node", "pele-commit", "pele-note"];
        for group in elements.iter().filter(|n| has_class(**n, &nodes)) {
            self.group(*group, true);
        }
        for edge in elements.iter().filter(|n| has_class(**n, &["pele-edge"])) {
            self.edge(*edge);
        }
        for element in elements
            .iter()
            .filter(|n| is_shape(**n) || n.has_tag_name("text"))
        {
            self.loose(*element);
        }
        self.out
    }

    fn is_done(&self, node: Node) -> bool {
        self.done.contains(&node.id())
    }

    // --- Nodes ----------------------------------------------------------------------------------

    /// A node or a cluster: its outline as a shape, and what else it draws. With `bind_label`, a
    /// node's one text is its outline's label, and its outline is what edges join; a cluster's
    /// outline is that too, and its texts stay texts.
    ///
    /// The outline is the largest shape. A rectangle, an ellipse or a diamond is a board shape of
    /// its own; any other outline (a cylinder, a hexagon) is drawn as lines under a box nobody
    /// sees, which holds the label and is what edges join. So is a node that is only text.
    fn group(&mut self, group: Node<'a, 'input>, bind_label: bool) {
        if self.is_done(group) || !visible(group) {
            return;
        }
        let shapes: Vec<Node> = group
            .descendants()
            .skip(1)
            .filter(|n| is_shape(*n) && !self.is_done(*n) && visible(*n) && !self.is_mask(*n))
            .collect();
        let texts: Vec<Node> = group
            .descendants()
            .skip(1)
            .filter(|n| n.has_tag_name("text") && !self.is_done(*n) && !text_of(*n).is_empty())
            .collect();
        self.done.insert(group.id());
        let outline = shapes
            .iter()
            .filter_map(|shape| Some((*shape, shape_box(*shape)?)))
            .fold(
                None,
                |largest: Option<(Node, Rect)>, (shape, frame)| match largest {
                    Some((_, most)) if most.area() >= frame.area() => largest,
                    _ => Some((shape, frame)),
                },
            );
        // The container, its frame as Pele drew it, and how much room it makes for a label.
        let container = match outline {
            Some((shape, frame)) => match container_kind(shape) {
                Some(kind) => {
                    self.done.insert(shape.id());
                    let rounded = is_rounded(shape, Some(group));
                    let drawn = self.box_shape(shape, kind, frame, rounded);
                    Some((drawn, frame, Room::Words))
                }
                None => Some((invisible(frame), frame, Room::Words)),
            },
            None if bind_label && texts.len() == 1 => {
                let frame = text_place(texts[0]).bounds().inflate(TEXT_PAD, TEXT_PAD);
                Some((invisible(frame), frame, Room::Lines))
            }
            None => None,
        };
        let mut members = Vec::new();
        for shape in &shapes {
            if !self.is_done(*shape) {
                members.extend(self.shape(*shape));
            }
        }
        if let Some((mut container, frame, room)) = container {
            let label = texts.first().copied().filter(|text| {
                bind_label && texts.len() == 1 && holds_middle(frame, text_place(*text).bounds())
            });
            if let Some(text) = label {
                let said = text_of(text);
                fit(&mut container, &said, room);
                container.label = Some(Label { text: said });
                self.done.insert(text.id());
            }
            if bind_label || has_class(group, &["pele-cluster"]) {
                let id = format!("mermaid-{}", self.targets.len() + 1);
                container.id = Some(id.clone());
                // Pele ended its edges on the outline as it drew it, so that is what they join,
                // whatever room the label took.
                self.targets.push(Target {
                    id,
                    frame,
                    note: has_class(group, &["pele-note"]),
                });
            }
            members.push(container);
        }
        for text in &texts {
            if !self.is_done(*text) {
                members.push(self.text(*text));
            }
        }
        self.out.extend(members);
    }

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
    fn edge(&mut self, edge: Node<'a, 'input>) {
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

    // --- Everything else ------------------------------------------------------------------------

    /// What no node or edge drew: a text, or a shape as it is drawn.
    fn loose(&mut self, element: Node<'a, 'input>) {
        if self.is_done(element) || !visible(element) {
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
    fn shape(&mut self, shape: Node<'a, 'input>) -> Vec<Skeleton> {
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
    fn box_shape(&self, shape: Node, kind: &str, frame: Rect, rounded: bool) -> Skeleton {
        let (background, stroke) = self.shape_colours(shape);
        Skeleton {
            kind: kind.into(),
            background_color: Some(background),
            stroke_color: Some(stroke),
            roundness: rounded.then(|| json!({"type": 3})),
            ..in_frame(frame)
        }
    }

    /// A text of the board's. Its anchor stays where Pele put it, at the size the engine gives
    /// new text, and its colour (its fill) travels in `strokeColor`, as an Excalidraw text's does.
    fn text(&mut self, text: Node<'a, 'input>) -> Skeleton {
        self.done.insert(text.id());
        let place = text_place(text);
        let size = TextData::default().style.font_size;
        let width = place.width(size);
        let height = place.lines as f64 * size * LINE_HEIGHT;
        Skeleton {
            kind: "text".into(),
            x: place.at.x - place.anchor.before(width),
            y: place.at.y - height / 2.0,
            text: Some(text_of(text)),
            stroke_color: Some(self.ink_or(paint(text, "fill"))),
            ..Skeleton::default()
        }
    }

    // --- Colours --------------------------------------------------------------------------------

    /// A shape's colours as a skeleton has them: its fill as `backgroundColor`, its stroke as
    /// `strokeColor`. Pele's neutral colours become the canvas's own: no fill, ink lines.
    fn shape_colours(&self, shape: Node) -> (String, String) {
        let background = match paint(shape, "fill") {
            Some(fill) if !self.neutrals.fills.contains(&fill) => {
                let colour = if Some(&fill) == self.neutrals.surface_alt.as_ref() {
                    SURFACE_ALT_FILL.to_string()
                } else if self.neutrals.strokes.contains(&fill) {
                    INK.to_string()
                } else {
                    fill
                };
                // A tint (mindmap branches, clusters): the colour as it looks on white.
                let alpha = inherited(shape, "fill-opacity").and_then(length);
                match alpha {
                    Some(alpha) if alpha < 1.0 => tint(&colour, alpha),
                    _ => colour,
                }
            }
            _ => TRANSPARENT.to_string(),
        };
        (background, self.stroke_colour(shape))
    }

    fn stroke_colour(&self, shape: Node) -> String {
        match paint(shape, "stroke") {
            None => TRANSPARENT.to_string(),
            stroke => self.ink_or(stroke),
        }
    }

    /// `colour`, or ink for none or a neutral one.
    fn ink_or(&self, colour: Option<String>) -> String {
        colour
            .filter(|c| !self.neutrals.strokes.contains(c))
            .unwrap_or_else(|| INK.to_string())
    }

    /// Whether `shape` only hides what is behind a label: a rectangle of the page's colour with no
    /// outline.
    fn is_mask(&self, shape: Node) -> bool {
        shape.has_tag_name("rect")
            && paint(shape, "fill") == self.neutrals.background
            && paint(shape, "stroke").is_none()
    }
}

/// An edge's end that joins skeleton `id`.
fn joined(id: String) -> End {
    End {
        id: Some(id),
        ..End::default()
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
fn invisible(frame: Rect) -> Skeleton {
    Skeleton {
        kind: "rectangle".into(),
        background_color: Some(TRANSPARENT.into()),
        stroke_color: Some(TRANSPARENT.into()),
        ..in_frame(frame)
    }
}

/// A line through `points`, which are in the diagram's coordinates: a connector through two of
/// them, ink through more.
fn line_through(points: &[Point], stroke: String, fill: String) -> Skeleton {
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

/// How much room [`fit`] makes for a label.
#[derive(Clone, Copy)]
enum Room {
    /// For its longest word. The engine breaks a label's lines between words where it must, and
    /// a node grown to keep Pele's lines whole would run into its neighbours; but a word too long
    /// for its line is broken inside it.
    Words,
    /// For each of its lines as it is: the box made around a node that is only text, which
    /// nobody sees.
    Lines,
}

/// Grows `shape` around its middle until the engine can set `label` in it with the `room` asked
/// for. A board shape keeps its label inside its insets, so an ellipse or a diamond holds less
/// than the shape Pele drew around the same text (pele.ts `fit`, with FigCraft's insets for
/// Excalidraw's).
fn fit(shape: &mut Skeleton, label: &str, room: Room) {
    let insets = match shape.kind.as_str() {
        "ellipse" => BoardShape::Ellipse,
        "diamond" => BoardShape::Diamond,
        _ => BoardShape::Rectangle,
    }
    .text_insets();
    let [top, right, bottom, left] = insets;
    let (widest, tall) = match room {
        Room::Words => (widest_word(label), 0.0),
        Room::Lines => {
            let lines = label.lines().count().max(1) as f64;
            let tall = lines * LABEL_SIZE * LABEL_LINE_HEIGHT / (1.0 - top - bottom);
            (widest_line(label), tall)
        }
    };
    let wide = widest * LABEL_SIZE / (1.0 - left - right);
    let (width, height) = (shape.width.unwrap_or(0.0), shape.height.unwrap_or(0.0));
    let (grown_width, grown_height) = (width.max(wide), height.max(tall));
    shape.x -= (grown_width - width) / 2.0;
    shape.y -= (grown_height - height) / 2.0;
    shape.width = Some(grown_width);
    shape.height = Some(grown_height);
}

/// The board shape that draws `shape` itself, if there is one.
fn container_kind(shape: Node) -> Option<&'static str> {
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
fn is_rounded(shape: Node, group: Option<Node>) -> bool {
    let radius = shape
        .attribute("rx")
        .and_then(length)
        .is_some_and(|rx| rx > 0.0);
    let styled = shape
        .attribute("style")
        .is_some_and(|style| declarations(style).any(|(property, _)| property == "rx"));
    let shaped = group.is_some_and(|g| has_class(g, &["pele-shape-rounded", "pele-shape-stadium"]));
    shape.has_tag_name("rect") && (radius || styled || shaped)
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

// --- Styles ---------------------------------------------------------------------------------------

/// The `property: value` declarations of a `style` attribute.
fn declarations(style: &str) -> impl Iterator<Item = (&str, &str)> {
    style
        .split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .map(|(property, value)| (property.trim(), value.trim()))
}

/// What `node`'s own markup sets `property` to: its last declaration in the `style` attribute,
/// which wins over the attribute of that name.
fn declared<'a>(node: Node<'a, '_>, property: &str) -> Option<&'a str> {
    let styled = node.attribute("style").and_then(|style| {
        declarations(style)
            .filter(|(name, _)| name.eq_ignore_ascii_case(property))
            .map(|(_, value)| value)
            .last()
    });
    styled
        .or_else(|| node.attribute(property).map(str::trim))
        .filter(|value| *value != "inherit")
}

/// An inherited property (fill, stroke, text-anchor...): what `node` or the nearest group around
/// it sets it to.
fn inherited<'a>(node: Node<'a, '_>, property: &str) -> Option<&'a str> {
    node.ancestors()
        .filter(Node::is_element)
        .find_map(|n| declared(n, property))
}

/// `node`'s fill or stroke as `#rrggbb`, or `None` for no paint: `none`, a transparent colour, or a
/// paint that is no colour (a gradient). Without a fill anywhere, the fill is SVG's black.
fn paint(node: Node, property: &str) -> Option<String> {
    let value = match inherited(node, property) {
        Some(value) => value,
        None if property == "fill" => "black",
        None => return None,
    };
    if value.eq_ignore_ascii_case("currentcolor") {
        return inherited(node, "color").and_then(hex);
    }
    hex(value)
}

/// A CSS colour as `#rrggbb`, or `None` when it is transparent or no colour.
fn hex(colour: &str) -> Option<String> {
    let colour = Color::parse_css(colour).filter(|c| c.a > 0.0)?;
    let [r, g, b, _] = colour.to_rgba8();
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// `colour` as it looks at `alpha` over white.
fn tint(colour: &str, alpha: f64) -> String {
    let Some(colour) = Color::parse_css(colour) else {
        return colour.to_string();
    };
    let alpha = alpha.clamp(0.0, 1.0);
    let over_white = |v: u8| (f64::from(v) * alpha + 255.0 * (1.0 - alpha)).round() as u8;
    let [r, g, b, _] = colour.to_rgba8();
    format!(
        "#{:02x}{:02x}{:02x}",
        over_white(r),
        over_white(g),
        over_white(b)
    )
}

/// Whether `node` is drawn: not hidden, not fully transparent, not in a definition and, unless it
/// is a group or a text, painted.
fn visible(node: Node) -> bool {
    let hidden = node
        .ancestors()
        .filter(Node::is_element)
        .any(|n| declared(n, "display") == Some("none") || NOT_DRAWN.contains(&tag(n)));
    if hidden || matches!(inherited(node, "visibility"), Some("hidden" | "collapse")) {
        return false;
    }
    if declared(node, "opacity").and_then(length) == Some(0.0) {
        return false;
    }
    matches!(tag(node), "g" | "text")
        || paint(node, "fill").is_some()
        || paint(node, "stroke").is_some()
}

/// Whether `node` is the page that `sawit_core::mermaid::flatten` paints behind the diagram.
fn is_background(node: Node) -> bool {
    node.has_tag_name("rect")
        && node.attribute("width") == Some("100%")
        && node.attribute("height") == Some("100%")
}

fn tag<'a>(node: Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

fn is_shape(node: Node) -> bool {
    node.is_element() && SHAPES.contains(&tag(node))
}

fn has_class(node: Node, classes: &[&str]) -> bool {
    node.attribute("class")
        .is_some_and(|class| class.split_whitespace().any(|c| classes.contains(&c)))
}

// --- Text -----------------------------------------------------------------------------------------

/// What a text says: its lines (a text's spans are one line while they share a `y`), each trimmed.
fn text_of(text: Node) -> String {
    let spans: Vec<Node> = text
        .descendants()
        .filter(|n| n.has_tag_name("tspan"))
        .collect();
    if spans.len() <= 1 {
        return content(text).trim().to_string();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line_y = None;
    for span in spans {
        let y = span.attribute("y").or_else(|| span.attribute("dy"));
        match lines.last_mut() {
            Some(line) if line_y == Some(y) => line.push_str(&content(span)),
            _ => lines.push(content(span)),
        }
        line_y = Some(y);
    }
    let lines: Vec<&str> = lines.iter().map(|line| line.trim()).collect();
    lines.join("\n")
}

/// All the text in `node`.
fn content(node: Node) -> String {
    node.descendants()
        .filter_map(|n| n.text().filter(|_| n.is_text()))
        .collect()
}

/// About how wide `c` is in a sans-serif face like the engine's, in ems: a little wide rather
/// than narrow, so that a label given room by it isn't broken.
fn advance(c: char) -> f64 {
    match c {
        ' ' | 'i' | 'j' | 'l' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' | '`' => 0.28,
        'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '{' | '}' | '/' | '\\' | '-' | '"' | '*' => 0.4,
        'm' | 'w' => 0.85,
        'M' | 'W' => 0.95,
        'A'..='Z' => 0.7,
        '0'..='9' => 0.62,
        'a'..='z' => 0.58,
        c if c.is_ascii() => 0.6,
        c if is_wide(c) => 1.0,
        _ => 0.62,
    }
}

/// Whether `c` takes a whole em: Chinese, Japanese and Korean script, full-width forms and emoji.
fn is_wide(c: char) -> bool {
    matches!(
        u32::from(c),
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
            | 0x20000..=0x3FFFD
    )
}

/// About how wide `text` is on one line, in ems.
fn ems(text: &str) -> f64 {
    text.chars().map(advance).sum()
}

/// About how wide the widest line of `text` is, in ems.
fn widest_line(text: &str) -> f64 {
    text.lines().map(ems).fold(0.0, f64::max)
}

/// About how wide the widest word of `text` is, in ems.
fn widest_word(text: &str) -> f64 {
    text.split_whitespace().map(ems).fold(0.0, f64::max)
}

/// Which end of a line of text its anchor is.
#[derive(Clone, Copy)]
enum Anchor {
    Start,
    Middle,
    End,
}

impl Anchor {
    /// How far left of the anchor a line `width` wide starts.
    fn before(self, width: f64) -> f64 {
        match self {
            Self::Start => 0.0,
            Self::Middle => width / 2.0,
            Self::End => width,
        }
    }
}

/// Where a text is drawn, as far as its markup says.
struct TextPlace {
    anchor: Anchor,
    /// The anchor's x, and the middle of the text's lines, in the diagram's coordinates.
    at: Point,
    /// The font size Pele set it in.
    size: f64,
    lines: usize,
    /// About how wide its widest line is, in ems.
    ems: f64,
}

impl TextPlace {
    /// About how wide its widest line is at font `size`.
    fn width(&self, size: f64) -> f64 {
        self.ems * size
    }

    /// About the box Pele drew it in.
    fn bounds(&self) -> Rect {
        let width = self.width(self.size);
        let height = self.lines as f64 * self.size * LINE_HEIGHT;
        let left = self.at.x - self.anchor.before(width);
        Rect::new(
            left,
            self.at.y - height / 2.0,
            left + width,
            self.at.y + height / 2.0,
        )
    }
}

/// Where `text` is drawn: its anchor is its `x` (or its first span's), and its lines run from its
/// `y` (or its spans') down, each with its baseline [`BASELINE`] below its middle.
fn text_place(text: Node) -> TextPlace {
    let said = text_of(text);
    let size = font_size(text);
    let spans: Vec<Node> = text
        .descendants()
        .filter(|n| n.has_tag_name("tspan"))
        .collect();
    let x = spans
        .iter()
        .find_map(|span| coordinate(*span, "x"))
        .or_else(|| coordinate(text, "x"))
        .unwrap_or(0.0);
    let mut baselines: Vec<f64> = spans
        .iter()
        .filter_map(|span| coordinate(*span, "y"))
        .collect();
    if baselines.is_empty() {
        baselines.push(coordinate(text, "y").unwrap_or(0.0));
    }
    let lines = said.lines().count().max(1);
    let first = baselines[0];
    let last = match baselines.last() {
        Some(last) if baselines.len() > 1 => *last,
        _ => first + (lines - 1) as f64 * size * LINE_HEIGHT,
    };
    let middle = Point::new(x, (first + last) / 2.0 - BASELINE * size);
    let anchor = match inherited(text, "text-anchor") {
        Some("middle") => Anchor::Middle,
        Some("end") => Anchor::End,
        _ => Anchor::Start,
    };
    TextPlace {
        anchor,
        at: transform_of(text) * middle,
        size,
        lines,
        ems: widest_line(&said),
    }
}

/// The font size of `node`'s text, rounded: from `font-size` or the `font` shorthand, on it or
/// the nearest group around it that sets one.
fn font_size(node: Node) -> f64 {
    let size = node.ancestors().filter(Node::is_element).find_map(|n| {
        let styled = n.attribute("style").and_then(|style| {
            declarations(style)
                .filter_map(|(property, value)| match property {
                    "font-size" => length(value),
                    // `[style] [weight] size[/line height] family`: the size is the length in px
                    // (a weight is a bare number).
                    "font" => value.split_whitespace().find_map(|part| {
                        let size = part.split('/').next()?;
                        size.ends_with("px").then(|| length(size)).flatten()
                    }),
                    _ => None,
                })
                .last()
        });
        styled.or_else(|| n.attribute("font-size").and_then(length))
    });
    size.filter(|size| *size > 0.0)
        .map_or(FONT_SIZE, f64::round)
}

// --- Geometry -------------------------------------------------------------------------------------

/// A length in px: `12` or `12px`.
fn length(value: &str) -> Option<f64> {
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
fn coordinate(node: Node, name: &str) -> Option<f64> {
    numbers(node.attribute(name)?).first().copied()
}

/// The numbers in `text`, as SVG lists them: apart by spaces or commas, or run together where a
/// sign or a second point starts the next one (`10-5`, `.5.5`).
fn numbers(text: &str) -> Vec<f64> {
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
fn point_list(text: &str) -> Vec<Point> {
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
fn transform_of(node: Node) -> Affine {
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
fn shape_box(shape: Node) -> Option<Rect> {
    Some(transform_of(shape).transform_rect_bbox(local_box(shape)?))
}

/// The lines `shape` draws, one list of points for each subpath, in the diagram's coordinates: a
/// path's curves are sampled, and a rectangle or an ellipse is its box.
fn subpaths(shape: Node) -> Vec<Vec<Point>> {
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
fn points(shape: Node) -> Vec<Point> {
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
fn distance_to_outline(point: Point, frame: Rect) -> f64 {
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
fn distance_to_route(point: Point, route: &[Point]) -> f64 {
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
fn is_straight(route: &[Point]) -> bool {
    let (Some(first), Some(last)) = (route.first(), route.last()) else {
        return true;
    };
    let line = [*first, *last];
    route
        .iter()
        .all(|point| distance_to_route(*point, &line) <= STRAIGHT)
}

/// Whether the middle of `inner` is in `outer`.
fn holds_middle(outer: Rect, inner: Rect) -> bool {
    let middle = inner.center();
    (outer.x0..=outer.x1).contains(&middle.x) && (outer.y0..=outer.y1).contains(&middle.y)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use figcraft_doc::{BoardItem, NodeId};
    use sawit_core::mermaid::render;
    use serde_json::Value;

    use super::*;
    use crate::{BoardDoc, ops};

    const FLOW: &str = include_str!("../tests/fixtures/mermaid/flow.mmd");
    const SEQUENCE: &str = include_str!("../tests/fixtures/mermaid/sequence.mmd");
    const PIE: &str = include_str!("../tests/fixtures/mermaid/pie.mmd");

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// The skeletons of `source`, rendered as the op renders it.
    fn skeletons(source: &str) -> Vec<Skeleton> {
        let diagram = render(source, &Palette::default()).unwrap();
        to_skeletons(&diagram).unwrap()
    }

    fn of_kind<'a>(items: &'a [Skeleton], kind: &str) -> Vec<&'a Skeleton> {
        items.iter().filter(|item| item.kind == kind).collect()
    }

    fn label(item: &Skeleton) -> Option<&str> {
        item.label.as_ref().map(|label| label.text.as_str())
    }

    fn point_count(item: &Skeleton) -> usize {
        item.points.as_ref().map_or(0, Vec::len)
    }

    /// The label of the node skeleton that `end` joins.
    fn joins<'a>(items: &'a [Skeleton], end: &Option<End>) -> &'a str {
        let id = end.as_ref().and_then(|end| end.id.as_deref());
        let id = id.expect("an end that joins a node");
        let node = items.iter().find(|item| item.id.as_deref() == Some(id));
        label(node.expect("the id of a node")).expect("a node with a label")
    }

    /// Runs the mermaid op on `board` and checks that it answered with what it added.
    fn draw(board: &mut BoardDoc, op: Value) -> Vec<NodeId> {
        let reply = ops::run(board, &op);
        assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
        let ids: Vec<NodeId> = reply["ids"]
            .as_array()
            .expect("ids")
            .iter()
            .map(|id| id.as_str().expect("an id string").parse().unwrap())
            .collect();
        assert_eq!(reply["added"], json!(ids.len()), "{reply}");
        assert!(!ids.is_empty(), "{reply}");
        ids
    }

    /// The box around layers `ids` on the page, read from the document.
    fn bounds_of(board: &BoardDoc, ids: &[NodeId]) -> Rect {
        let doc = board.doc().unwrap();
        doc.bounds_of(ids).expect("layers with bounds")
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn flowchart_gives_nodes_and_bound_arrows() {
        let items = skeletons(FLOW);
        // Three nodes and two arrows, and nothing else: neither the page behind the diagram nor
        // what hides the line behind the edge label is a shape of it.
        assert_eq!(items.len(), 5, "{items:#?}");
        let nodes: Vec<&Skeleton> = items.iter().filter(|item| item.kind != "arrow").collect();
        let arrows = of_kind(&items, "arrow");
        let kinds: Vec<&str> = nodes.iter().map(|node| node.kind.as_str()).collect();
        assert_eq!(kinds, ["rectangle", "diamond", "rectangle"]);
        let labels: Vec<Option<&str>> = nodes.iter().map(|node| label(node)).collect();
        assert_eq!(labels, [Some("Start"), Some("Ok?"), Some("Done")]);

        // Each arrow joins two of the nodes, by the ids of their skeletons.
        let joined: Vec<(&str, &str)> = arrows
            .iter()
            .map(|arrow| (joins(&items, &arrow.start), joins(&items, &arrow.end)))
            .collect();
        assert_eq!(joined, [("Start", "Ok?"), ("Ok?", "Done")]);
        let arrow_labels: Vec<Option<&str>> = arrows.iter().map(|arrow| label(arrow)).collect();
        assert_eq!(arrow_labels, [None, Some("yes")]);
    }

    #[test]
    fn sequence_is_editable() {
        let items = skeletons(SEQUENCE);
        assert!(of_kind(&items, "rectangle").len() >= 2, "{items:#?}");
        assert!(!of_kind(&items, "arrow").is_empty(), "{items:#?}");
        assert!(of_kind(&items, "image").is_empty(), "{items:#?}");
    }

    #[test]
    fn pie_comes_in_as_svg_image() {
        let diagram = render(PIE, &Palette::default()).unwrap();
        assert_eq!(diagram.kind, "pie");
        let items = to_skeletons(&diagram).unwrap();
        assert_eq!(items.len(), 1, "{items:#?}");
        let picture = &items[0];
        assert_eq!(picture.kind, "image");
        let svg = picture.svg.as_deref().expect("the diagram's SVG");
        assert!(svg.starts_with("<svg"), "{svg}");
        assert_eq!(picture.name.as_deref(), Some("diagram.svg"));
        assert_eq!((picture.x, picture.y), (0.0, 0.0));
        assert_eq!(
            (picture.width, picture.height),
            (Some(diagram.width), Some(diagram.height))
        );
    }

    #[test]
    fn a_pie_is_drawn_as_one_imported_picture() {
        let mut board = board();
        let reply = ops::run(&mut board, &json!({"op": "mermaid", "source": PIE}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["added"], json!(1), "{reply}");
        assert!(reply.get("notes").is_none(), "{reply}");
        let id: NodeId = reply["ids"][0].as_str().unwrap().parse().unwrap();
        // At the top left of the empty page, as big as Pele drew it.
        let diagram = render(PIE, &Palette::default()).unwrap();
        let at = bounds_of(&board, &[id]);
        assert!(near(at.x0, 0.0) && near(at.y0, 0.0), "{at:?}");
        assert!(
            near(at.width(), diagram.width) && near(at.height(), diagram.height),
            "{at:?}"
        );
    }

    #[test]
    fn mermaid_syntax_error_is_reported() {
        let mut board = board();
        let before = board.doc().unwrap();
        let reply = ops::run(
            &mut board,
            &json!({"op": "mermaid", "source": "flowchart LR; A-->"}),
        );
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.starts_with("mermaid: "), "{error}");
        // Pele's own words, which quote the source.
        assert!(error.contains("Parse error"), "{error}");
        assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    }

    #[test]
    fn mermaid_op_places_right_of_content() {
        let mut board = board();
        let sticky = board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "first"}))
            .unwrap();
        let sticky: NodeId = sticky["id"].to_string().parse().unwrap();
        let there = bounds_of(&board, &[sticky]);
        assert!(near(there.x0, 0.0) && near(there.x1, 200.0), "{there:?}");

        let ids = draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
        let placed = bounds_of(&board, &ids);
        assert!(placed.x0 >= 300.0 - 1e-6, "{placed:?}");
        // 100 px right of the sticky, level with its top.
        assert!(near(placed.x0, 300.0) && near(placed.y0, 0.0), "{placed:?}");
    }

    #[test]
    fn the_op_puts_a_diagram_at_x_y_or_at_the_origin_of_an_empty_page() {
        let mut board = board();
        let first = draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
        let at = bounds_of(&board, &first);
        assert!(near(at.x0, 0.0) && near(at.y0, 0.0), "{at:?}");

        let ids = draw(
            &mut board,
            json!({"op": "mermaid", "source": FLOW, "x": -500, "y": 1000}),
        );
        let at = bounds_of(&board, &ids);
        assert!(near(at.x0, -500.0) && near(at.y0, 1000.0), "{at:?}");

        // Only x: the diagram is still level with the top of what the page holds.
        let ids = draw(
            &mut board,
            json!({"op": "mermaid", "source": FLOW, "x": 2000}),
        );
        let at = bounds_of(&board, &ids);
        assert!(near(at.x0, 2000.0) && near(at.y0, 0.0), "{at:?}");
    }

    #[test]
    fn the_op_draws_shapes_with_connectors_bound_to_them() {
        let mut board = board();
        let ids = draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
        let doc = board.doc().unwrap();
        let text = |id: NodeId| {
            let text = figcraft_engine::board::text_of(&doc, id)?;
            Some(doc.node(text)?.kind.text()?.characters.clone())
        };
        let mut shapes = Vec::new();
        let mut connectors = Vec::new();
        for id in &ids {
            match doc.node(*id).and_then(|n| n.board.clone()) {
                Some(BoardItem::Shape { shape }) => shapes.push((*id, shape, text(*id))),
                Some(BoardItem::Connector(c)) => connectors.push((*id, c)),
                other => panic!("{id} is neither a shape nor a connector: {other:?}"),
            }
        }
        let drawn: Vec<(BoardShape, Option<String>)> = shapes
            .iter()
            .map(|(_, shape, text)| (*shape, text.clone()))
            .collect();
        assert_eq!(
            drawn,
            [
                (BoardShape::RoundedRectangle, Some("Start".into())),
                (BoardShape::Diamond, Some("Ok?".into())),
                (BoardShape::RoundedRectangle, Some("Done".into())),
            ]
        );
        // Both connectors are attached at both ends: Start to Ok?, Ok? to Done.
        let node = |at: usize| shapes[at].0;
        let ends: Vec<(Option<NodeId>, Option<NodeId>)> = connectors
            .iter()
            .map(|(_, c)| (c.from.node(), c.to.node()))
            .collect();
        assert_eq!(
            ends,
            [
                (Some(node(0)), Some(node(1))),
                (Some(node(1)), Some(node(2)))
            ]
        );
        // The second one carries the label "yes".
        let label_of = |at: usize| figcraft_engine::board::label_of(&doc, connectors[at].0);
        assert_eq!(label_of(0), None);
        assert_eq!(label_of(1).and_then(text).as_deref(), Some("yes"));
    }

    #[test]
    fn the_op_is_one_undo_step() {
        let mut board = board();
        draw(&mut board, json!({"op": "mermaid", "source": FLOW}));
        assert_eq!(board.session.undo_depth(), (1, 0));
        board.execute("edit.undo", &json!({})).unwrap();
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        assert!(doc.node(page).unwrap().children.is_empty());
    }

    #[test]
    fn double_quotes_that_trip_up_a_diagram_are_tried_again_as_single_ones() {
        let source = "flowchart LR\n  A[\"a\"] --> B\n  style A fill:\"#f9f\"\n";
        assert!(render(source, &Palette::default()).is_err());
        let mut board = board();
        let ids = draw(&mut board, json!({"op": "mermaid", "source": source}));
        // Two nodes and the arrow between them.
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn the_op_needs_a_source() {
        let mut board = board();
        for op in [
            json!({"op": "mermaid"}),
            json!({"op": "mermaid", "source": " \n"}),
            json!({"op": "mermaid", "source": 5}),
        ] {
            let reply = ops::run(&mut board, &op);
            assert_eq!(
                reply,
                json!({"ok": false, "error": "mermaid: `source` must be a non-empty string"}),
                "{op}"
            );
        }
    }

    #[test]
    fn a_diagram_without_shapes_is_reported() {
        let mut board = board();
        let reply = ops::run(
            &mut board,
            &json!({"op": "mermaid", "source": "flowchart LR"}),
        );
        assert_eq!(
            reply,
            json!({"ok": false, "error": "mermaid: the diagram is empty"})
        );
    }

    /// A diagram of every type that comes in as shapes.
    const EVERY_KIND: &[(&str, &str)] = &[
        (
            "flowchart",
            "flowchart TD\n  A([Start]) --> B{Ok?}\n  B -->|yes| C[(Store)]\n  B -.-> D{{Retry}}\n  subgraph S [Group]\n    D --> E((End))\n  end\n",
        ),
        (
            "sequence",
            "sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi\n  Alice->>Alice: Think\n  Note right of Bob: A note\n  Alice-xBob: Lost\n",
        ),
        (
            "class",
            "classDiagram\n  class Animal {\n    +String name\n    +eat() void\n  }\n  Animal <|-- Dog\n  Dog *-- Tail\n  Dog o-- Toy\n",
        ),
        (
            "state",
            "stateDiagram-v2\n  [*] --> Still\n  Still --> Moving : push\n  Moving --> [*]\n",
        ),
        (
            "er",
            "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n",
        ),
        (
            "mindmap",
            "mindmap\n  root((Root))\n    Branch\n      Leaf\n    Other\n",
        ),
        (
            "requirement",
            "requirementDiagram\n  requirement need {\n    id: 1\n    text: the need.\n    risk: high\n    verifymethod: test\n  }\n  element thing {\n    type: simulation\n  }\n  thing - satisfies -> need\n",
        ),
        (
            "block",
            "block-beta\n  columns 2\n  a[\"A\"] b[\"B\"]\n  a --> b\n",
        ),
        (
            "kanban",
            "kanban\n  Todo\n    [Write]\n  Done\n    [Read]\n",
        ),
        (
            "c4",
            "C4Context\n  title Context\n  Person(customer, \"Customer\", \"A customer\")\n  System(bank, \"Bank\", \"The bank\")\n  Rel(customer, bank, \"Uses\")\n",
        ),
        (
            "gitGraph",
            "gitGraph\n  commit\n  branch dev\n  commit\n  checkout main\n  merge dev\n",
        ),
        (
            "timeline",
            "timeline\n  title History\n  2020 : One\n  2021 : Two : Three\n",
        ),
    ];

    #[test]
    fn every_editable_kind_comes_in_as_shapes() {
        let mut kinds: Vec<&str> = EVERY_KIND.iter().map(|(kind, _)| *kind).collect();
        let mut editable = EDITABLE.to_vec();
        kinds.sort_unstable();
        editable.sort_unstable();
        assert_eq!(kinds, editable);

        for (kind, source) in EVERY_KIND {
            let diagram = render(source, &Palette::default()).unwrap();
            assert_eq!(diagram.kind, *kind);
            let items = to_skeletons(&diagram).unwrap();
            assert!(!items.is_empty(), "{kind}");
            assert!(of_kind(&items, "image").is_empty(), "{kind}: {items:#?}");
            // The engine takes every one of them: the op answers, and draws something.
            let mut board = board();
            draw(&mut board, json!({"op": "mermaid", "source": source}));
        }
    }

    #[test]
    fn an_arrowhead_at_the_start_turns_the_arrow_around() {
        // `Animal <|-- Dog`: Pele draws the line from Animal down to Dog, the triangle at Animal.
        let items = skeletons("classDiagram\n  Animal <|-- Dog\n");
        let arrows = of_kind(&items, "arrow");
        assert_eq!(arrows.len(), 1, "{items:#?}");
        let arrow = arrows[0];
        assert_eq!(
            (joins(&items, &arrow.start), joins(&items, &arrow.end)),
            ("Dog", "Animal")
        );
        // The triangle is the arrow's own head: the only lines left are the classes' dividers.
        let lines = of_kind(&items, "line");
        assert!(
            lines.iter().all(|line| point_count(line) == 2),
            "{lines:#?}"
        );
    }

    #[test]
    fn marks_a_connector_has_no_cap_for_stay_drawn_and_the_edge_still_joins_its_nodes() {
        // A composition: a diamond at Dog's end of the line, which stops where the diamond begins.
        let items = skeletons("classDiagram\n  Dog *-- Tail\n");
        assert!(of_kind(&items, "arrow").is_empty(), "{items:#?}");
        let lines = of_kind(&items, "line");
        let joined: Vec<&&Skeleton> = lines
            .iter()
            .filter(|line| line.start.is_some() && line.end.is_some())
            .collect();
        assert_eq!(joined.len(), 1, "{items:#?}");
        assert_eq!(
            (
                joins(&items, &joined[0].start),
                joins(&items, &joined[0].end)
            ),
            ("Dog", "Tail")
        );
        // Ink has no fill, so the filled diamond is outlined in its colour.
        let diamond = lines.iter().find(|line| point_count(line) == 5);
        let diamond = diamond.expect("the diamond, closed: four corners and the first again");
        assert_eq!(diamond.stroke_color.as_deref(), Some(INK));

        // A double arrow: one head is the connector's, the other stays where Pele drew it.
        let items = skeletons("flowchart LR\n  A <--> B\n");
        let arrows = of_kind(&items, "arrow");
        assert_eq!(arrows.len(), 1, "{items:#?}");
        assert_eq!(
            (
                joins(&items, &arrows[0].start),
                joins(&items, &arrows[0].end)
            ),
            ("A", "B")
        );
        let heads = of_kind(&items, "line");
        assert_eq!(heads.len(), 1, "{items:#?}");
        assert_eq!(point_count(heads[0]), 4);
        assert_eq!(heads[0].stroke_color.as_deref(), Some(INK));

        // An entity relationship: a line between the entities, with its label; its bars, circle
        // and crow's foot stay drawn on it.
        let items = skeletons("erDiagram\n  CUSTOMER ||--o{ ORDER : places\n");
        let lines = of_kind(&items, "line");
        let joined: Vec<&&Skeleton> = lines.iter().filter(|line| line.end.is_some()).collect();
        assert_eq!(joined.len(), 1, "{items:#?}");
        assert_eq!(
            (
                joins(&items, &joined[0].start),
                joins(&items, &joined[0].end)
            ),
            ("CUSTOMER", "ORDER")
        );
        assert_eq!(label(joined[0]), Some("places"));
        // Marks that sit on the line leave its ends where they are: it runs straight down.
        let run = joined[0].points.as_ref().unwrap()[1];
        assert!(near(run[0], 0.0) && run[1] > 0.0, "{run:?}");
        assert_eq!(
            lines.len(),
            4,
            "the line, two bars and a crow's foot: {items:#?}"
        );
        assert_eq!(of_kind(&items, "ellipse").len(), 1, "{items:#?}");
    }

    #[test]
    fn colours_are_pele_s_and_its_neutral_ones_the_canvas_s() {
        let items =
            skeletons("flowchart LR\n  A[Plain] --> B[Red]\n  style B fill:#f9d0c4,stroke:#c00\n");
        let colours: Vec<(&str, Option<&str>, Option<&str>)> = items
            .iter()
            .map(|item| {
                (
                    item.kind.as_str(),
                    item.background_color.as_deref(),
                    item.stroke_color.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            colours,
            [
                // Pele's surface and border: unfilled, outlined in ink.
                ("rectangle", Some("transparent"), Some(INK)),
                // A colour of the diagram's own.
                ("rectangle", Some("#f9d0c4"), Some("#cc0000")),
                // Pele's line colour.
                ("arrow", None, Some(INK)),
            ]
        );

        // A cluster in Pele's second surface, half see-through: as it looks on white. Its title is
        // a text, whose colour travels in strokeColor.
        let items = skeletons("flowchart LR\n  subgraph S [Group]\n    A\n  end\n");
        let cluster = &items[0];
        assert_eq!(cluster.kind, "rectangle");
        assert_eq!(cluster.background_color.as_deref(), Some("#f4f6f7"));
        assert_eq!(cluster.stroke_color.as_deref(), Some(INK));
        let title = items
            .iter()
            .find(|item| item.text.as_deref() == Some("Group"));
        let title = title.expect("the cluster's title");
        assert_eq!(title.kind, "text");
        assert_eq!(title.stroke_color.as_deref(), Some("#6e6e6e"));
        assert_eq!(title.background_color, None);
    }

    /// The width a board shape skeleton's label is set in: its width inside the shape's insets.
    fn room(item: &Skeleton) -> f64 {
        let shape = match item.kind.as_str() {
            "ellipse" => BoardShape::Ellipse,
            "diamond" => BoardShape::Diamond,
            _ => BoardShape::Rectangle,
        };
        let [_, right, _, left] = shape.text_insets();
        item.width.expect("a size") * (1.0 - left - right)
    }

    /// The box Pele drew the first `tag` element of `diagram` in.
    fn drawn(diagram: &Diagram, tag: &str) -> Rect {
        let svg = Document::parse(&diagram.svg).unwrap();
        let shape = svg.descendants().find(|n| n.has_tag_name(tag)).unwrap();
        shape_box(shape).unwrap()
    }

    #[test]
    fn a_node_that_is_only_text_gets_an_invisible_box_that_its_edges_join() {
        let items = skeletons("mindmap\n  root((Root))\n    Leaf\n");
        let leaf = items.iter().find(|item| label(item) == Some("Leaf"));
        let leaf = leaf.expect("the leaf, holding its text as a label");
        assert_eq!(leaf.kind, "rectangle");
        assert_eq!(leaf.background_color.as_deref(), Some("transparent"));
        assert_eq!(leaf.stroke_color.as_deref(), Some("transparent"));
        // Room for its text on the one line Pele drew it on.
        assert!(room(leaf) >= widest_line("Leaf") * LABEL_SIZE - 1e-9);
        // The edge from the root joins it.
        let edges = of_kind(&items, "line");
        assert_eq!(edges.len(), 1, "{items:#?}");
        assert_eq!(
            (joins(&items, &edges[0].start), joins(&items, &edges[0].end)),
            ("Root", "Leaf")
        );
    }

    #[test]
    fn a_self_message_keeps_its_loop() {
        // A message to oneself loops out from the lifeline and back. Its ends join nothing, so it
        // is drawn as Pele drew it, its head where Pele put it.
        let items = skeletons("sequenceDiagram\n  Alice->>Alice: Think\n");
        let loops: Vec<&Skeleton> = of_kind(&items, "line")
            .into_iter()
            .filter(|line| point_count(line) > 4)
            .collect();
        assert_eq!(loops.len(), 1, "{items:#?}");
        let route = loops[0].points.as_ref().unwrap();
        let reach = route.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        assert!(reach > 20.0, "it loops out to the right: {route:?}");
        assert!(of_kind(&items, "arrow").is_empty(), "{items:#?}");
        // Its label is a text beside it.
        assert!(
            items
                .iter()
                .any(|item| item.text.as_deref() == Some("Think")),
            "{items:#?}"
        );
    }

    #[test]
    fn a_loop_back_to_its_own_node_keeps_its_loop() {
        // Both ends of a loop are on its node, and a connector can't join one layer at both ends.
        // It joins neither and is drawn as Pele drew it, as a message to oneself is: out of the
        // node and back, its marks and its label where Pele put them.
        let diagrams = [
            (
                "stateDiagram-v2\n  Still --> Still : tick\n  Still --> Moving\n",
                "Still",
                Some("tick"),
                Some(("Still", "Moving")),
            ),
            (
                "flowchart LR\n  A[Retry] --> A\n  A --> B[Done]\n",
                "Retry",
                None,
                Some(("Retry", "Done")),
            ),
            (
                "classDiagram\n  Node --> Node : next\n",
                "Node",
                Some("next"),
                None,
            ),
            (
                "erDiagram\n  PERSON ||--o{ PERSON : parent\n",
                "PERSON",
                Some("parent"),
                None,
            ),
        ];
        for (source, name, said, other) in diagrams {
            let items = skeletons(source);
            // No connector names one node at both ends.
            let id = |end: &Option<End>| end.as_ref().and_then(|end| end.id.clone());
            for item in &items {
                if let (Some(start), Some(end)) = (id(&item.start), id(&item.end)) {
                    assert_ne!(start, end, "{source}: {items:#?}");
                }
            }
            // The loop: one line through more than 4 points, joined to nothing, from the node's
            // outline out of it and back.
            let node = items.iter().find(|item| label(item) == Some(name));
            let node = node.expect("the node");
            let frame = Rect::from_origin_size(
                (node.x, node.y),
                (node.width.unwrap(), node.height.unwrap()),
            );
            let lines = of_kind(&items, "line");
            let loops: Vec<&&Skeleton> =
                lines.iter().filter(|line| point_count(line) > 4).collect();
            assert_eq!(loops.len(), 1, "{source}: {items:#?}");
            let ring = loops[0];
            assert!(
                ring.start.is_none() && ring.end.is_none(),
                "{source}: {ring:#?}"
            );
            let route: Vec<Point> = ring
                .points
                .as_ref()
                .unwrap()
                .iter()
                .map(|p| Point::new(ring.x + p[0], ring.y + p[1]))
                .collect();
            let (first, last) = (route[0], route[route.len() - 1]);
            assert!(
                distance_to_outline(first, frame) <= BIND_DISTANCE
                    && distance_to_outline(last, frame) <= BIND_DISTANCE,
                "{source}: {route:?} starts and ends on {frame:?}"
            );
            let out = frame.inflate(10.0, 10.0);
            assert!(
                route.iter().any(|p| !out.contains(*p)),
                "{source}: {route:?} loops out of {frame:?}"
            );
            if let Some(said) = said {
                let text = items.iter().find(|item| item.text.as_deref() == Some(said));
                assert_eq!(
                    text.map(|text| text.kind.as_str()),
                    Some("text"),
                    "{source}"
                );
            }
            // The diagram's other edge still joins its two nodes.
            let arrows = of_kind(&items, "arrow");
            let joined: Vec<(&str, &str)> = arrows
                .iter()
                .map(|arrow| (joins(&items, &arrow.start), joins(&items, &arrow.end)))
                .collect();
            assert_eq!(joined, other.into_iter().collect::<Vec<_>>(), "{source}");
            // The op draws it all, with nothing to say about it.
            let mut board = board();
            let reply = ops::run(&mut board, &json!({"op": "mermaid", "source": source}));
            assert_eq!(reply["ok"], json!(true), "{source}: {reply}");
            assert!(reply.get("notes").is_none(), "{source}: {reply}");
        }
    }

    #[test]
    fn a_message_joins_no_note_under_it() {
        // Pele puts a note 10 px under the message before it, near enough to join. A message runs
        // from lifeline to lifeline, so it stays free and level, whether the note is over one
        // lifeline or over both.
        for source in [
            "sequenceDiagram\n  Alice->>Bob: Hello\n  Note over Bob: Thinks\n",
            "sequenceDiagram\n  Alice->>Bob: Hello\n  Note right of Bob: A note\n  Bob-->>Alice: Hi\n  Note over Alice,Bob: Shared\n  Alice->>Bob: Again\n",
        ] {
            let items = skeletons(source);
            let messages = of_kind(&items, "arrow");
            assert!(!messages.is_empty(), "{source}: {items:#?}");
            for message in messages {
                assert!(
                    message.start.is_none() && message.end.is_none(),
                    "{source}: {message:#?}"
                );
                let run = message.points.as_ref().unwrap()[1];
                assert_eq!(run[1], 0.0, "{source}: {message:#?}");
            }
            // The notes are still there, each holding its text.
            let notes = ["Thinks", "A note", "Shared"];
            let held = items
                .iter()
                .filter(|item| label(item).is_some_and(|said| notes.contains(&said)));
            assert_eq!(held.count(), source.matches("Note ").count(), "{items:#?}");
            let mut board = board();
            let reply = ops::run(&mut board, &json!({"op": "mermaid", "source": source}));
            assert_eq!(reply["ok"], json!(true), "{source}: {reply}");
            assert!(reply.get("notes").is_none(), "{source}: {reply}");
        }

        // What a message does end at, the box of a participant it makes, it still joins.
        let items = skeletons(
            "sequenceDiagram\n  Alice->>Bob: Hi\n  create participant Carl\n  Alice->>Carl: Hi Carl\n",
        );
        let ends: Vec<Option<&str>> = of_kind(&items, "arrow")
            .iter()
            .map(|arrow| arrow.end.as_ref().map(|_| joins(&items, &arrow.end)))
            .collect();
        assert_eq!(ends, [None, Some("Carl")], "{items:#?}");
        // And a note's own line, which is no message, still joins its note.
        let items = skeletons("classDiagram\n  class Dog\n  note for Dog \"can bark\"\n");
        let joined: Vec<(&str, &str)> = of_kind(&items, "line")
            .iter()
            .filter(|line| line.start.is_some() || line.end.is_some())
            .map(|line| (joins(&items, &line.start), joins(&items, &line.end)))
            .collect();
        assert_eq!(joined, [("can bark", "Dog")], "{items:#?}");
    }

    #[test]
    fn a_long_word_gets_room_and_a_long_label_breaks_between_words() {
        // Pele's circle around one long word leaves the engine's label too little room, so the
        // word would be broken. The circle grows around its middle until the word fits.
        let source = "flowchart LR\n  A((Authentication)) --> B[Next]\n";
        let diagram = render(source, &Palette::default()).unwrap();
        let circle = drawn(&diagram, "circle");
        let word = widest_word("Authentication") * LABEL_SIZE;
        let [_, right, _, left] = BoardShape::Ellipse.text_insets();
        assert!(circle.width() * (1.0 - left - right) < word, "{circle:?}");
        let items = to_skeletons(&diagram).unwrap();
        let node = items
            .iter()
            .find(|item| label(item) == Some("Authentication"));
        let node = node.expect("the circle");
        assert!(room(node) >= word - 1e-9, "{node:?}");
        let middle = Point::new(
            node.x + node.width.unwrap() / 2.0,
            node.y + node.height.unwrap() / 2.0,
        );
        assert!(middle.distance(circle.center()) < 1e-6, "{node:?}");
        assert!(near(node.height.unwrap(), circle.height()), "{node:?}");
        // The arrow still joins it: Pele ended it on the circle as drawn.
        let arrows = of_kind(&items, "arrow");
        assert_eq!(arrows.len(), 1, "{items:#?}");
        assert_eq!(
            (
                joins(&items, &arrows[0].start),
                joins(&items, &arrows[0].end)
            ),
            ("Authentication", "Next")
        );

        // A label of several words that doesn't fit on one line is broken between them, as the
        // engine sets it, so the diamond keeps the size Pele drew it at and its neighbours keep
        // their room.
        let label_text = "Is the payload valid?";
        let diagram = render(
            &format!("flowchart LR\n  A{{{label_text}}}\n"),
            &Palette::default(),
        )
        .unwrap();
        let diamond = drawn(&diagram, "polygon");
        let items = to_skeletons(&diagram).unwrap();
        let node = &items[0];
        assert_eq!(
            (node.kind.as_str(), label(node)),
            ("diamond", Some(label_text))
        );
        assert!(
            room(node) < widest_line(label_text) * LABEL_SIZE,
            "{node:?}"
        );
        let (x, width) = (node.x, node.width.unwrap());
        assert!(
            near(x, diamond.x0) && near(width, diamond.width()),
            "{node:?}"
        );
    }

    #[test]
    fn a_lost_message_runs_straight_to_its_cross() {
        // `-x`: the message ends in a cross, which a connector has no cap for. The cross stays
        // drawn, and the line runs on straight to its far side.
        let items = skeletons("sequenceDiagram\n  Alice-xBob: Lost\n");
        assert!(of_kind(&items, "arrow").is_empty(), "{items:#?}");
        let lines = of_kind(&items, "line");
        let run = |line: &&Skeleton| line.points.as_ref().unwrap()[1];
        let across: Vec<&&Skeleton> = lines
            .iter()
            .filter(|line| point_count(line) == 2 && run(line)[1].abs() < 1e-6)
            .collect();
        assert_eq!(across.len(), 1, "the message: {items:#?}");
        let strokes: Vec<&&Skeleton> = lines
            .iter()
            .filter(|line| run(line)[0].abs() > 1e-6 && run(line)[1].abs() > 1e-6)
            .collect();
        assert_eq!(strokes.len(), 2, "the cross: {items:#?}");
        let far_side = strokes
            .iter()
            .map(|stroke| stroke.x + run(stroke)[0].max(0.0))
            .fold(f64::NEG_INFINITY, f64::max);
        let message = across[0];
        assert!(near(message.x + run(message)[0], far_side), "{items:#?}");
    }

    /// The skeletons of a diagram whose SVG is `svg` (a flowchart, as far as the converter knows).
    fn converted(svg: &str) -> Vec<Skeleton> {
        let diagram = Diagram {
            svg: svg.into(),
            width: 100.0,
            height: 100.0,
            kind: "flowchart".into(),
        };
        to_skeletons(&diagram).unwrap()
    }

    #[test]
    fn a_text_keeps_its_anchor_at_the_size_the_engine_gives_it() {
        let items = converted(
            r#"<svg xmlns="http://www.w3.org/2000/svg" font-size="16">
                 <g transform="translate(100, 0)">
                   <text x="0" y="50">abcd</text>
                   <text x="0" y="50" text-anchor="middle">abcd</text>
                   <text x="0" y="50" style="text-anchor: end">abcd</text>
                 </g>
               </svg>"#,
        );
        assert_eq!(items.len(), 3, "{items:#?}");
        // New text comes out at the engine's size, so that is the size its width is guessed at.
        let size = TextData::default().style.font_size;
        let width = ems("abcd") * size;
        let lefts: Vec<f64> = items.iter().map(|text| text.x).collect();
        assert!(near(lefts[0], 100.0), "{lefts:?}");
        assert!(near(lefts[1], 100.0 - width / 2.0), "{lefts:?}");
        assert!(near(lefts[2], 100.0 - width), "{lefts:?}");
        // Each is centred on the middle of Pele's line, BASELINE ems above its baseline.
        for text in &items {
            assert_eq!(text.kind, "text");
            assert_eq!(text.text.as_deref(), Some("abcd"));
            let middle = text.y + size * LINE_HEIGHT / 2.0;
            assert!(near(middle, 50.0 - BASELINE * 16.0), "{text:?}");
        }
    }

    #[test]
    fn text_is_estimated_wider_for_wider_letters() {
        assert!(ems("iiii") < ems("aaaa") && ems("aaaa") < ems("AAAA"));
        assert!(ems("AAAA") < ems("WWWW") && ems("WWWW") <= ems("漢字漢字"));
        assert!(near(ems("漢"), 1.0));
        assert!(near(widest_line("ab\nabcd\nabc"), ems("abcd")));
        assert!(near(widest_word("a abcd abc"), ems("abcd")));
        assert_eq!(widest_word(""), 0.0);
    }

    #[test]
    fn an_edge_label_on_its_route_is_the_connector_s_and_one_beside_it_a_text() {
        // Down the page, a label that starts left of the line and runs across it; across it, one
        // that sits above the line.
        let items = converted(
            r##"<svg xmlns="http://www.w3.org/2000/svg" font-size="16" fill="#1f1f1f">
                 <g class="pele-edges" fill="none" stroke="#1f1f1f">
                   <g class="pele-edge" data-id="down"><path d="M50,0V100"/></g>
                   <g class="pele-edge" data-id="across"><path d="M100,50H200"/></g>
                 </g>
                 <g class="pele-edge-labels">
                   <g class="pele-edge-label" data-id="down"><text x="33" y="55.6">abcd</text></g>
                   <g class="pele-edge-label" data-id="across">
                     <text x="150" y="30" text-anchor="middle">efgh</text>
                   </g>
                 </g>
               </svg>"##,
        );
        let kinds: Vec<&str> = items.iter().map(|item| item.kind.as_str()).collect();
        assert_eq!(kinds, ["line", "line", "text"]);
        assert_eq!(label(&items[0]), Some("abcd"));
        assert_eq!(label(&items[1]), None);
        assert_eq!(items[2].text.as_deref(), Some("efgh"));
    }

    fn assert_box(actual: Rect, (x0, y0, x1, y1): (f64, f64, f64, f64)) {
        let expected = Rect::new(x0, y0, x1, y1);
        let same = near(actual.x0, x0) && near(actual.y0, y0);
        assert!(
            same && near(actual.x1, x1) && near(actual.y1, y1),
            "{actual:?} is not {expected:?}"
        );
    }

    #[test]
    fn transforms_compose_down_the_tree_and_boxes_come_from_the_geometry() {
        // The root's own transform is not the diagram's: its coordinates are.
        let svg = Document::parse(
            r#"<svg xmlns="http://www.w3.org/2000/svg" transform="translate(1000, 1000)">
                 <g transform="translate(100,50)">
                   <g transform="scale(2)"><rect x="10" y="10" width="20" height="10"/></g>
                   <g transform="rotate(90)"><rect width="10" height="20"/></g>
                   <g transform="matrix(1 0 0 1 5 6) translate(1)"><circle r="5"/></g>
                   <path d="M0,0 C0,-10 10,-10 10,0"/>
                   <polygon points="0,0 10-5 20,0" transform="scale(1, 2)"/>
                   <line x1="5" y1="5" x2="-5" y2="15"/>
                   <ellipse cx="1" cy="2" rx="3" ry="4" transform="rotate(90 1 2)"/>
                 </g>
               </svg>"#,
        )
        .unwrap();
        let boxes: Vec<Rect> = svg
            .descendants()
            .filter(|node| is_shape(*node))
            .map(|shape| shape_box(shape).unwrap())
            .collect();
        assert_eq!(boxes.len(), 7);
        assert_box(boxes[0], (120.0, 70.0, 160.0, 90.0));
        assert_box(boxes[1], (80.0, 50.0, 100.0, 60.0));
        assert_box(boxes[2], (101.0, 51.0, 111.0, 61.0));
        // A curve's box is its extent, not its control points'.
        assert_box(boxes[3], (100.0, 42.5, 110.0, 50.0));
        assert_box(boxes[4], (100.0, 40.0, 120.0, 50.0));
        assert_box(boxes[5], (95.0, 55.0, 105.0, 65.0));
        assert_box(boxes[6], (97.0, 49.0, 105.0, 55.0));
    }

    #[test]
    fn number_lists_are_read_as_svg_writes_them() {
        assert_eq!(numbers("1,2 3"), [1.0, 2.0, 3.0]);
        assert_eq!(numbers("10-5.5.5"), [10.0, -5.5, 0.5]);
        assert_eq!(numbers("-1e2, +.5e-1 x 7"), [-100.0, 0.05, 7.0]);
        assert_eq!(numbers("1e"), [1.0]);
        assert!(numbers("- . e ,").is_empty());
    }

    #[test]
    fn an_svg_that_cannot_be_read_is_an_error() {
        let diagram = Diagram {
            svg: "<svg><g></svg>".into(),
            width: 10.0,
            height: 10.0,
            kind: "flowchart".into(),
        };
        let error = to_skeletons(&diagram).unwrap_err();
        assert!(
            error.starts_with("the diagram's SVG can't be read"),
            "{error}"
        );
    }
}
