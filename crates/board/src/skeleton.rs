//! Element skeletons: the JSON that agents send to `canvas_add_elements`, that the Mermaid
//! converter makes from a diagram and that the Excalidraw importer makes from a scene. [`apply`]
//! turns a list of them into board objects (shapes, stickies, text, sections, images, connectors
//! and ink) in one undo step, and [`resolve`] finds an element by the id an agent gave it.
//!
//! A [`Skeleton`] has Excalidraw's shape, so that agents' existing calls keep working. Its
//! coordinates do too: `x`, `y` and the `x`, `y` of a free [`End`] are page coordinates, and
//! `points` are relative to the element's own `x`, `y`. The `offset` of [`apply`] moves all of
//! them.

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use figcraft_color::Color;
use figcraft_doc::{BoardItem, Document, Node, NodeId, NodeKind};
use figcraft_engine::params::id_of;
use figcraft_engine::{EngineError, Result as EngineResult, Session};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::doc::{BoardDoc, BoardError};

/// The `pluginData` key that holds the id an agent gave an element.
pub const SAWIT_ID: &str = "sawit.id";

/// The size `board.shape` gives a shape that has none. [`bounds`] counts it too.
const SHAPE_SIZE: f64 = 160.0;
/// What `node.create` gives a section that has no size.
const SECTION_SIZE: (f64, f64) = (400.0, 300.0);
/// How far a line or an arrow that has no size at all runs to the right (Excalidraw's default).
const LINE_LENGTH: f64 = 100.0;
/// Points closer than this are one point: the engine draws no line through them.
const SAME_POINT: f64 = 1e-9;

/// The types a skeleton can have, for the note on one that has another.
const TYPES: &str =
    "rectangle, ellipse, diamond, sticky, text, arrow, line, frame, image, freedraw";

/// One element to draw.
///
/// `kind` is one of `rectangle`, `ellipse`, `diamond`, `sticky`, `text`, `arrow`, `line`, `frame`,
/// `image` or `freedraw`; anything else is skipped with a note. Fields that a type has no use for
/// are ignored, and so is any other key in the JSON, and a missing field is its default.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Skeleton {
    #[serde(rename = "type")]
    pub kind: String,
    /// The agent's own id. Other elements name it in `start`, `end` and `frameId`, and it works
    /// wherever the canvas takes an id (see [`resolve`]).
    pub id: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// The text inside a shape, or on an arrow or a line.
    pub label: Option<Label>,
    /// The text of a `text` or a `sticky`.
    pub text: Option<String>,
    /// Where an `arrow` or a `line` starts and ends.
    pub start: Option<End>,
    pub end: Option<End>,
    /// The points of a line, an arrow or a `freedraw`, relative to `x`, `y`.
    pub points: Option<Vec<[f64; 2]>>,
    #[serde(rename = "strokeColor")]
    pub stroke_color: Option<String>,
    #[serde(rename = "backgroundColor")]
    pub background_color: Option<String>,
    /// A rectangle with `roundness` (anything but `null`, `false` or 0) has rounded corners.
    pub roundness: Option<Value>,
    /// The `id` of the `frame` this element goes into.
    #[serde(rename = "frameId")]
    pub frame_id: Option<String>,
    /// An `image` as SVG text, or as base64 `data`; `name` is its file name, and its extension
    /// tells the importer which format `data` is in.
    pub svg: Option<String>,
    pub data: Option<String>,
    /// The file name of an `image`, or the title of a `frame`.
    pub name: Option<String>,
}

/// The text inside a shape, or on an arrow or a line.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Label {
    pub text: String,
}

/// One end of an arrow or a line: attached to the element whose `id` it names, else a free point.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct End {
    pub id: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// What [`apply`] made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The node id of each element that was made, in input order. A skipped element has none.
    pub ids: Vec<String>,
    /// What was skipped or could not be kept, one sentence each.
    pub notes: Vec<String>,
}

/// Draw `items` on the board's current page, moved by `offset`, as one undo step ("Add
/// elements"). The selection stays as it was.
///
/// What can't be drawn is skipped with a note in the result, never an error: an element of an
/// unknown type, a picture that can't be imported, a line with no length. Everything the engine
/// refuses beyond that (a size or a colour it doesn't take, a design file that has no board
/// objects) is an error that names the element, and then nothing is changed.
///
/// Elements are made in this order, whatever order they are given in: sections first (so one never
/// covers what is drawn on it), then shapes, stickies and pictures, then text, then arrows, lines
/// and ink (so what they join exists). An arrow's `start`/`end` and an element's `frameId` name an
/// `id` of this call first, then one already on the board, then a node id. An end that names
/// nothing it can join stays free, at its own `x`/`y` or where the arrow is drawn. Elements with a
/// `frameId` then move into that section, and keep their place on the page.
pub fn apply(
    board: &mut BoardDoc,
    items: &[Skeleton],
    offset: (f64, f64),
) -> Result<Applied, BoardError> {
    let selection = board.session.selection();
    let applied = board.session.grouped("Add elements", |session| {
        Maker::new(session, offset)?.make(items)
    })?;
    // Making a layer selects it. The selection is the user's, so it goes back (unless nothing was
    // made, which leaves it as it is, and the engine's change counter too).
    if board.session.selection() != selection {
        board.session.set_selection(selection);
    }
    Ok(applied)
}

/// The element that `id` names: the one an agent gave that id (the newest, if several have it),
/// else the node with that id. An agent's id wins, so `"1"` is the element an agent called `"1"`
/// even though node 1 exists.
pub fn resolve(doc: &Document, id: &str) -> Option<NodeId> {
    doc.nodes()
        .filter(|node| agent_id(node) == Some(id))
        .map(|node| node.id)
        .max()
        .or_else(|| node_id(doc, id))
}

/// The box around `items` as `(min x, min y, max x, max y)`, in the coordinates of the skeletons,
/// or `None` when none of them can be drawn. A shape, a sticky or a section without a size counts
/// as big as it is drawn. Text and pictures count as the size they are given, else as their
/// position: how big they come out is not known before they are drawn.
pub fn bounds(items: &[Skeleton]) -> Option<(f64, f64, f64, f64)> {
    let mut found: Option<(f64, f64, f64, f64)> = None;
    let mut cover = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            found = Some(match found {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    };
    for item in items {
        let Some(element) = Element::of(&item.kind) else {
            continue;
        };
        cover(item.x, item.y);
        if element.is_linear() {
            let points = item.points.as_deref().unwrap_or_default();
            for p in points {
                cover(item.x + p[0], item.y + p[1]);
            }
            if points.is_empty() {
                let (width, height) = line_extent(item);
                cover(item.x + width, item.y + height);
            }
            for end in [&item.start, &item.end].into_iter().flatten() {
                if let (Some(x), Some(y)) = (end.x, end.y) {
                    cover(x, y);
                }
            }
        } else {
            let (width, height) = element.size(item);
            cover(item.x + width, item.y + height);
        }
    }
    found
}

/// Colour layer `id` from a skeleton's `backgroundColor` and `strokeColor`, by the spec's rules.
/// `apply` colours what it draws with it. It is public so that changing the colours of a layer
/// that is already there does the same as drawing it with them.
///
/// - Stickies and shapes with text: the background is `board.setColor` (a board colour name or
///   any colour; transparent clears the fill); the stroke is the outline's stroke paint.
/// - Connectors and ink have a line and no fill: the stroke is the line's colour, and the
///   background is ignored, because `board.setColor` would recolour the line with it.
/// - Text is coloured by its fill, and Excalidraw's text colour is its `strokeColor`: that is the
///   fill, and the background is ignored (nothing is drawn behind text).
/// - Any other layer (sections, frames, pictures): the background is its fill and the stroke its
///   stroke paint.
///
/// Transparent (`transparent`, `none`, or a colour with no alpha) is no paint at all. A colour that
/// is empty counts as not given. Colours the engine doesn't know are its error.
pub fn set_colors(
    session: &mut Session,
    id: NodeId,
    background: Option<&str>,
    stroke: Option<&str>,
) -> EngineResult<()> {
    let (background, stroke) = (given(background), given(stroke));
    let (item, is_text, outline) = {
        let node = session.doc()?.get(id)?;
        (
            node.board.clone(),
            matches!(node.kind, NodeKind::Text(_)),
            node.children.first().copied(),
        )
    };
    match item {
        Some(BoardItem::Connector(_) | BoardItem::Ink) => {
            if let Some(colour) = stroke {
                paint(session, id, "stroke", colour)?;
            }
        }
        Some(BoardItem::Sticky { .. }) => board_object(session, id, id, background, stroke)?,
        Some(BoardItem::Shape { .. }) => {
            board_object(session, id, outline.unwrap_or(id), background, stroke)?;
        }
        _ if is_text => {
            if let Some(colour) = stroke {
                paint(session, id, "fill", colour)?;
            }
        }
        _ => {
            let mut props = Map::new();
            for (key, colour) in [("fill", background), ("stroke", stroke)] {
                if let Some(colour) = colour {
                    props.insert(key.into(), json!(paint_value(colour)));
                }
            }
            if !props.is_empty() {
                session.execute("node.set", &json!({"ids": [id], "props": props}))?;
            }
        }
    }
    Ok(())
}

/// A colour as it was given: an empty one is none.
fn given(colour: Option<&str>) -> Option<&str> {
    colour.map(str::trim).filter(|c| !c.is_empty())
}

/// Colour a sticky or a shape with text, `id`, whose fill and stroke are `drawn`'s (the outline of
/// a shape; the sticky itself). The background comes first: `board.setColor` sets the outline's
/// stroke too, which the stroke colour then replaces.
fn board_object(
    session: &mut Session,
    id: NodeId,
    drawn: NodeId,
    background: Option<&str>,
    stroke: Option<&str>,
) -> EngineResult<()> {
    if let Some(colour) = background {
        if is_transparent(colour) {
            paint(session, drawn, "fill", colour)?;
        } else {
            session.execute("board.setColor", &json!({"ids": [id], "color": colour}))?;
        }
    }
    if let Some(colour) = stroke {
        paint(session, drawn, "stroke", colour)?;
    }
    Ok(())
}

/// Give layer `id` the paint `key` (`fill` or `stroke`) of `colour`.
fn paint(session: &mut Session, id: NodeId, key: &str, colour: &str) -> EngineResult<()> {
    let props = json!({ (key): paint_value(colour) });
    session.execute("node.set", &json!({"ids": [id], "props": props}))?;
    Ok(())
}

/// What `node.set` takes for `colour`: transparent is no paint.
fn paint_value(colour: &str) -> &str {
    if is_transparent(colour) {
        "none"
    } else {
        colour
    }
}

fn is_transparent(colour: &str) -> bool {
    colour.eq_ignore_ascii_case("none") || Color::parse_css(colour).is_some_and(|c| c.a <= 0.0)
}

/// The id an agent gave `node`.
fn agent_id(node: &Node) -> Option<&str> {
    node.plugin_data.get(SAWIT_ID).map(String::as_str)
}

/// The node `id` names, if the document has it: `12` or Figma's `0:12`.
fn node_id(doc: &Document, id: &str) -> Option<NodeId> {
    id.parse::<NodeId>().ok().filter(|node| doc.contains(*node))
}

/// What a skeleton's `type` can be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Element {
    Rectangle,
    Ellipse,
    Diamond,
    Sticky,
    Text,
    Arrow,
    Line,
    Frame,
    Image,
    Freedraw,
}

/// The number of [`Element::stage`]s.
const STAGES: usize = 4;

impl Element {
    /// The element a `type` names, in any case.
    fn of(kind: &str) -> Option<Self> {
        Some(match kind.trim().to_ascii_lowercase().as_str() {
            "rectangle" => Self::Rectangle,
            "ellipse" => Self::Ellipse,
            "diamond" => Self::Diamond,
            "sticky" => Self::Sticky,
            "text" => Self::Text,
            "arrow" => Self::Arrow,
            "line" => Self::Line,
            "frame" => Self::Frame,
            "image" => Self::Image,
            "freedraw" => Self::Freedraw,
            _ => return None,
        })
    }

    /// When it is made: sections, then shapes, stickies and pictures, then text, then what runs
    /// from one thing to another.
    fn stage(self) -> usize {
        match self {
            Self::Frame => 0,
            Self::Rectangle | Self::Ellipse | Self::Diamond | Self::Sticky | Self::Image => 1,
            Self::Text => 2,
            Self::Arrow | Self::Line | Self::Freedraw => 3,
        }
    }

    /// Whether it is drawn through points rather than as a box.
    fn is_linear(self) -> bool {
        matches!(self, Self::Arrow | Self::Line | Self::Freedraw)
    }

    /// The size of its box: what the skeleton gives, else what it is drawn with.
    fn size(self, item: &Skeleton) -> (f64, f64) {
        match self {
            Self::Rectangle | Self::Ellipse | Self::Diamond => (
                item.width.unwrap_or(SHAPE_SIZE),
                item.height.unwrap_or(SHAPE_SIZE),
            ),
            Self::Sticky => (
                figcraft_engine::board::STICKY,
                figcraft_engine::board::STICKY,
            ),
            Self::Frame => (
                item.width.unwrap_or(SECTION_SIZE.0),
                item.height.unwrap_or(SECTION_SIZE.1),
            ),
            _ => (item.width.unwrap_or(0.0), item.height.unwrap_or(0.0)),
        }
    }
}

/// How far a line or an arrow without points runs from its `x`, `y`: by its size; with none at
/// all, [`LINE_LENGTH`] to the right.
fn line_extent(item: &Skeleton) -> (f64, f64) {
    match (item.width, item.height) {
        (None, None) => (LINE_LENGTH, 0.0),
        (width, height) => (width.unwrap_or(0.0), height.unwrap_or(0.0)),
    }
}

/// Whether `points` draw a line: two or more, not all in one place.
fn draws_a_line(points: &[[f64; 2]]) -> bool {
    let spread = |axis: usize| {
        let (low, high) = points
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), p| {
                (low.min(p[axis]), high.max(p[axis]))
            });
        high - low
    };
    points.len() >= 2 && (spread(0) > SAME_POINT || spread(1) > SAME_POINT)
}

/// Whether `roundness` asks for rounded corners: anything but `null`, `false` or 0.
fn is_rounded(roundness: Option<&Value>) -> bool {
    match roundness {
        None | Some(Value::Null) => false,
        Some(Value::Bool(rounded)) => *rounded,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(_) => true,
    }
}

/// The text of a shape, a sticky, an arrow or a line: its label, else its `text`.
fn label_or_text(item: &Skeleton) -> Option<&str> {
    let label = item
        .label
        .as_ref()
        .map(|l| l.text.as_str())
        .filter(|t| !t.is_empty());
    label.or(item.text.as_deref())
}

/// The text of a text element: its `text`, else its label.
fn text_or_label(item: &Skeleton) -> Option<&str> {
    let label = item.label.as_ref().map(|l| l.text.as_str());
    item.text.as_deref().or(label)
}

/// `args[key] = value` when there is a value.
fn put<T: Serialize>(args: &mut Value, key: &str, value: Option<T>) {
    if let Some(value) = value {
        args[key] = json!(value);
    }
}

/// The note on an element that has no known type.
fn unknown_type(index: usize, kind: &str) -> String {
    let problem = if kind.trim().is_empty() {
        "it has no type".to_string()
    } else {
        format!("`{kind}` is not a type")
    };
    format!("skipped elements[{index}]: {problem} (the types are {TYPES})")
}

/// `error`, said to be that of element `index`.
fn in_element(index: usize, item: &Skeleton, error: EngineError) -> EngineError {
    EngineError::Other(format!("elements[{index}] ({}): {error}", item.kind))
}

/// One end of a connector: the layer it joins, or else where it is. A free end is where it is;
/// for one that joins a layer, `at` is where it would be if it didn't.
struct Anchor {
    node: Option<NodeId>,
    at: (f64, f64),
}

impl Anchor {
    /// The end as `board.connector` takes it.
    fn json(&self) -> Value {
        match self.node {
            Some(node) => json!(node),
            None => json!({"x": self.at.0, "y": self.at.1}),
        }
    }
}

/// An `apply` in progress.
struct Maker<'a> {
    session: &'a mut Session,
    offset: (f64, f64),
    /// The page everything is drawn on.
    page: NodeId,
    /// The agent ids on the board when the call began, each as its newest holder.
    before: HashMap<String, NodeId>,
    /// The agent ids of what this call has made. They come before `before`: an id in a call
    /// means that call's own element, whatever the board already held under it.
    made: HashMap<String, NodeId>,
    notes: Vec<String>,
}

impl<'a> Maker<'a> {
    fn new(session: &'a mut Session, offset: (f64, f64)) -> EngineResult<Self> {
        let page = session.page()?;
        let mut before: HashMap<String, NodeId> = HashMap::new();
        for node in session.doc()?.nodes() {
            if let Some(id) = agent_id(node) {
                let newest = before.entry(id.to_string()).or_insert(node.id);
                *newest = (*newest).max(node.id);
            }
        }
        Ok(Self {
            session,
            offset,
            page,
            before,
            made: HashMap::new(),
            notes: Vec::new(),
        })
    }

    fn make(mut self, items: &[Skeleton]) -> EngineResult<Applied> {
        let elements: Vec<Option<Element>> = items.iter().map(|i| Element::of(&i.kind)).collect();
        for (index, (item, element)) in items.iter().zip(&elements).enumerate() {
            if element.is_none() {
                self.notes.push(unknown_type(index, &item.kind));
            }
        }
        let mut nodes: Vec<Option<NodeId>> = vec![None; items.len()];
        for stage in 0..STAGES {
            for (index, (item, element)) in items.iter().zip(&elements).enumerate() {
                let Some(element) = element.filter(|e| e.stage() == stage) else {
                    continue;
                };
                nodes[index] = self
                    .element(index, item, element)
                    .map_err(|e| in_element(index, item, e))?;
            }
        }
        for (index, (item, node)) in items.iter().zip(&nodes).enumerate() {
            self.place_in_frame(item, *node)
                .map_err(|e| in_element(index, item, e))?;
        }
        Ok(Applied {
            ids: nodes.iter().flatten().map(NodeId::to_string).collect(),
            notes: self.notes,
        })
    }

    /// Make one element, and remember the id the agent gave it.
    fn element(
        &mut self,
        index: usize,
        item: &Skeleton,
        element: Element,
    ) -> EngineResult<Option<NodeId>> {
        let node = match element {
            Element::Rectangle | Element::Ellipse | Element::Diamond => {
                Some(self.shape(item, element)?)
            }
            Element::Sticky => Some(self.sticky(item)?),
            Element::Text => Some(self.text(item)?),
            Element::Frame => Some(self.frame(item)?),
            Element::Image => self.image(index, item)?,
            Element::Arrow | Element::Line | Element::Freedraw => {
                self.line(index, item, element)?
            }
        };
        if let (Some(node), Some(id)) = (node, item.id.as_deref().filter(|id| !id.is_empty())) {
            let props = json!({"pluginData": {(SAWIT_ID): id}});
            self.session
                .execute("node.set", &json!({"ids": [node], "props": props}))?;
            self.made.insert(id.to_string(), node);
        }
        Ok(node)
    }

    /// Run a command that makes a layer, and name the layer it made.
    fn create(&mut self, command: &str, args: &Value) -> EngineResult<NodeId> {
        let made = self.session.execute(command, args)?;
        made.get("id")
            .and_then(id_of)
            .ok_or_else(|| EngineError::Other(format!("`{command}` made no layer")))
    }

    /// Where `item` is on the page.
    fn at(&self, item: &Skeleton) -> (f64, f64) {
        (item.x + self.offset.0, item.y + self.offset.1)
    }

    fn colour(&mut self, id: NodeId, item: &Skeleton) -> EngineResult<()> {
        set_colors(
            self.session,
            id,
            item.background_color.as_deref(),
            item.stroke_color.as_deref(),
        )
    }

    fn shape(&mut self, item: &Skeleton, element: Element) -> EngineResult<NodeId> {
        let shape = match element {
            Element::Ellipse => "ellipse",
            Element::Diamond => "diamond",
            _ if is_rounded(item.roundness.as_ref()) => "roundedRectangle",
            _ => "rectangle",
        };
        let (x, y) = self.at(item);
        let mut args = json!({"shape": shape, "x": x, "y": y});
        put(&mut args, "width", item.width);
        put(&mut args, "height", item.height);
        put(&mut args, "text", label_or_text(item));
        let id = self.create("board.shape", &args)?;
        self.colour(id, item)?;
        Ok(id)
    }

    fn sticky(&mut self, item: &Skeleton) -> EngineResult<NodeId> {
        let (x, y) = self.at(item);
        let mut args = json!({"x": x, "y": y});
        put(&mut args, "text", label_or_text(item));
        let id = self.create("board.sticky", &args)?;
        self.colour(id, item)?;
        Ok(id)
    }

    fn text(&mut self, item: &Skeleton) -> EngineResult<NodeId> {
        let (x, y) = self.at(item);
        let text = text_or_label(item).unwrap_or_default();
        let mut args = json!({"type": "text", "x": x, "y": y, "text": text});
        // A width makes the text wrap at it; without one it is as wide as it is long.
        put(&mut args, "width", item.width.filter(|w| *w > 0.0));
        let id = self.create("node.create", &args)?;
        self.colour(id, item)?;
        Ok(id)
    }

    fn frame(&mut self, item: &Skeleton) -> EngineResult<NodeId> {
        let (x, y) = self.at(item);
        let mut args = json!({"type": "section", "x": x, "y": y});
        put(&mut args, "width", item.width);
        put(&mut args, "height", item.height);
        let title = item.name.as_deref().filter(|n| !n.is_empty());
        put(&mut args, "name", title.or(label_or_text(item)));
        let id = self.create("node.create", &args)?;
        self.colour(id, item)?;
        Ok(id)
    }

    /// A picture: SVG becomes an editable frame, anything else an image layer. One that can't be
    /// imported is skipped with a note, because its bytes come from outside and the shapes
    /// around it are no worse for its loss.
    fn image(&mut self, index: usize, item: &Skeleton) -> EngineResult<Option<NodeId>> {
        let name = item
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        let (data, name) = match (item.svg.as_deref(), item.data.as_deref()) {
            (Some(svg), _) => {
                let name = name.unwrap_or("diagram.svg");
                let svg_name = if name.to_ascii_lowercase().ends_with(".svg") {
                    name.to_string()
                } else {
                    format!("{name}.svg")
                };
                (STANDARD.encode(svg), svg_name)
            }
            (None, Some(data)) => (data.to_string(), name.unwrap_or("image.png").to_string()),
            (None, None) => {
                let note =
                    format!("skipped elements[{index}]: the image has neither `svg` nor `data`");
                self.notes.push(note);
                return Ok(None);
            }
        };
        let (x, y) = self.at(item);
        let args = json!({"data": data, "name": name, "x": x, "y": y});
        let placed = match self.session.execute("file.import", &args) {
            Ok(placed) => placed,
            Err(error) => {
                let note =
                    format!("skipped elements[{index}]: the image could not be imported ({error})");
                self.notes.push(note);
                return Ok(None);
            }
        };
        let id = placed
            .get("id")
            .and_then(id_of)
            .ok_or_else(|| EngineError::Other("`file.import` made no layer".into()))?;
        let warnings = placed.get("warnings").and_then(Value::as_array);
        for warning in warnings.into_iter().flatten().filter_map(Value::as_str) {
            self.notes.push(format!("elements[{index}]: {warning}"));
        }
        // SVG keeps the size it was drawn at; a picture takes the size it is given.
        if !name.to_ascii_lowercase().ends_with(".svg") {
            self.fit_picture(id, &placed, item)?;
        }
        Ok(Some(id))
    }

    /// Resize the picture `id`, just placed (`placed` is the engine's answer), to the size the
    /// skeleton gives: both sides as given, or one side as given and the other in proportion.
    fn fit_picture(&mut self, id: NodeId, placed: &Value, item: &Skeleton) -> EngineResult<()> {
        let natural = |key: &str| placed.get(key).and_then(Value::as_f64).filter(|n| *n > 0.0);
        let given = |size: Option<f64>| size.filter(|n| n.is_finite() && *n > 0.0);
        let (Some(natural_width), Some(natural_height)) = (natural("width"), natural("height"))
        else {
            return Ok(());
        };
        let (width, height) = match (given(item.width), given(item.height)) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) => (width, natural_height * width / natural_width),
            (None, Some(height)) => (natural_width * height / natural_height, height),
            (None, None) => return Ok(()),
        };
        let props = json!({"width": width, "height": height});
        self.session
            .execute("node.set", &json!({"ids": [id], "props": props}))?;
        Ok(())
    }

    /// An arrow, a line or a freedraw: ink when it is a freedraw or a line through more than two
    /// points, else a connector.
    fn line(
        &mut self,
        index: usize,
        item: &Skeleton,
        element: Element,
    ) -> EngineResult<Option<NodeId>> {
        let (x, y) = self.at(item);
        let points: Vec<[f64; 2]> = item
            .points
            .iter()
            .flatten()
            .map(|p| [x + p[0], y + p[1]])
            .collect();
        if element == Element::Freedraw || (element == Element::Line && points.len() > 2) {
            if !draws_a_line(&points) {
                let note = format!("skipped elements[{index}]: its points draw no line");
                self.notes.push(note);
                return Ok(None);
            }
            let id = self.create("board.ink", &json!({"points": points}))?;
            self.colour(id, item)?;
            return Ok(Some(id));
        }
        self.connector(index, item, element, &points)
    }

    /// A connector from the start of `item` to its end. An end joins the layer it names. One that
    /// names no layer it can join is free: at its own `x`/`y`, else where `item` is drawn from (its
    /// `x`, `y`) or to (its last point, else `x`, `y` plus its size).
    fn connector(
        &mut self,
        index: usize,
        item: &Skeleton,
        element: Element,
        points: &[[f64; 2]],
    ) -> EngineResult<Option<NodeId>> {
        let (x, y) = self.at(item);
        let (width, height) = line_extent(item);
        let end_at = points
            .last()
            .map_or((x + width, y + height), |p| (p[0], p[1]));
        let from = self.anchor(item.start.as_ref(), (x, y));
        let mut to = self.anchor(item.end.as_ref(), end_at);
        if from.node.is_some() && from.node == to.node {
            let note = format!(
                "elements[{index}]: both ends join the same layer, so the end is left free where it is drawn"
            );
            self.notes.push(note);
            to.node = None;
        }
        if from.node.is_none() && to.node.is_none() && from.at == to.at {
            let note = format!("skipped elements[{index}]: it starts and ends at the same point");
            self.notes.push(note);
            return Ok(None);
        }
        let end_cap = if element == Element::Arrow {
            "triangleArrow"
        } else {
            "none"
        };
        let mut args =
            json!({"from": from.json(), "to": to.json(), "startCap": "none", "endCap": end_cap});
        put(
            &mut args,
            "label",
            label_or_text(item).filter(|t| !t.is_empty()),
        );
        let id = self.create("board.connector", &args)?;
        self.colour(id, item)?;
        Ok(Some(id))
    }

    /// One end of a connector. `end`'s `x`/`y` are page coordinates, so the offset moves them;
    /// what `end` leaves out comes from `fallback`, which is already moved.
    fn anchor(&self, end: Option<&End>, fallback: (f64, f64)) -> Anchor {
        let at = end.map_or(fallback, |end| {
            (
                end.x.map_or(fallback.0, |x| x + self.offset.0),
                end.y.map_or(fallback.1, |y| y + self.offset.1),
            )
        });
        let node = end
            .and_then(|end| end.id.as_deref())
            .and_then(|id| self.lookup(id))
            .filter(|node| self.can_join(*node));
        Anchor { node, at }
    }

    /// The node `id` names for this call: its own element, else one the board has under that
    /// agent id, else the node with that id.
    fn lookup(&self, id: &str) -> Option<NodeId> {
        let named = self.made.get(id).or_else(|| self.before.get(id)).copied();
        named.or_else(|| self.session.doc().ok().and_then(|doc| node_id(doc, id)))
    }

    /// Whether a connector can end on `node`: a layer of this page, but not the page, and not
    /// another connector.
    fn can_join(&self, node: NodeId) -> bool {
        let Ok(doc) = self.session.doc() else {
            return false;
        };
        doc.node(node)
            .is_some_and(|n| !n.is_page() && !matches!(n.board, Some(BoardItem::Connector(_))))
            && doc.page_of(node) == Some(self.page)
    }

    /// Whether `frame` can take `node` in: a section or a frame of this page that isn't `node`
    /// or inside it (sections go in sections only), and `node` isn't a connector or ink, which
    /// are always on the page.
    fn can_hold(&self, frame: NodeId, node: NodeId) -> bool {
        let Ok(doc) = self.session.doc() else {
            return false;
        };
        let (Some(holder), Some(inner)) = (doc.node(frame), doc.node(node)) else {
            return false;
        };
        let holds = matches!(holder.kind, NodeKind::Section { .. })
            || (holder.kind.is_frame_like() && !holder.is_board_atom());
        let takes_it = !matches!(inner.kind, NodeKind::Section { .. }) || doc.takes_sections(frame);
        let on_the_page = !matches!(inner.board, Some(BoardItem::Connector(_) | BoardItem::Ink));
        holds
            && takes_it
            && on_the_page
            && !doc.is_ancestor(node, frame)
            && doc.page_of(frame) == Some(self.page)
    }

    /// Move `node`, made from `item`, into the section its `frameId` names, if it can go there.
    /// It stays where it was on the page.
    fn place_in_frame(&mut self, item: &Skeleton, node: Option<NodeId>) -> EngineResult<()> {
        let (Some(frame_id), Some(node)) = (item.frame_id.as_deref(), node) else {
            return Ok(());
        };
        let Some(frame) = self
            .lookup(frame_id)
            .filter(|frame| self.can_hold(*frame, node))
        else {
            return Ok(());
        };
        self.session
            .execute("node.reparent", &json!({"ids": [node], "parent": frame}))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use figcraft_color::{Color, Paint, PaintKind};
    use figcraft_doc::{
        BOARD_COLORS, BoardItem, BoardShape, Connector, End as DocEnd, NodeKind, StrokeCap,
    };
    use serde_json::json;

    use super::*;

    // A 1x1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    fn skeletons(items: Value) -> Vec<Skeleton> {
        serde_json::from_value(items).unwrap()
    }

    /// Apply `items` as they are, without an offset.
    fn add(board: &mut BoardDoc, items: Value) -> Applied {
        apply(board, &skeletons(items), (0.0, 0.0)).unwrap()
    }

    /// The layers on the page, bottom first.
    fn layers(board: &BoardDoc) -> Vec<NodeId> {
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        doc.node(page).unwrap().children.clone()
    }

    /// The node of the `index`th id in `applied`.
    fn node(applied: &Applied, index: usize) -> NodeId {
        applied.ids[index].parse().unwrap()
    }

    fn board_item(board: &BoardDoc, id: NodeId) -> BoardItem {
        let doc = board.doc().unwrap();
        doc.node(id).unwrap().board.clone().unwrap()
    }

    fn connector(board: &BoardDoc, id: NodeId) -> Connector {
        match board_item(board, id) {
            BoardItem::Connector(c) => c,
            other => panic!("not a connector: {other:?}"),
        }
    }

    fn shape(board: &BoardDoc, id: NodeId) -> BoardShape {
        match board_item(board, id) {
            BoardItem::Shape { shape } => shape,
            other => panic!("not a shape: {other:?}"),
        }
    }

    /// The text of a shape or a sticky.
    fn text_in(board: &BoardDoc, id: NodeId) -> String {
        let doc = board.doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, id).expect("an object with text");
        doc.node(text)
            .unwrap()
            .kind
            .text()
            .unwrap()
            .characters
            .clone()
    }

    /// The first solid fill of layer `id`.
    fn fill(board: &BoardDoc, id: NodeId) -> Option<Color> {
        let doc = board.doc().unwrap();
        let n = doc.node(id).unwrap();
        n.fills.first().and_then(Paint::solid_color)
    }

    fn stroke(board: &BoardDoc, id: NodeId) -> Option<Color> {
        let doc = board.doc().unwrap();
        let n = doc.node(id).unwrap();
        n.strokes.first().and_then(Paint::solid_color)
    }

    fn css(colour: &str) -> Option<Color> {
        Color::parse_css(colour)
    }

    fn palette(name: &str) -> Color {
        BOARD_COLORS.iter().find(|(n, _)| *n == name).unwrap().1
    }

    /// The outline a shape with text is drawn by: its first child.
    fn outline(board: &BoardDoc, id: NodeId) -> NodeId {
        board.doc().unwrap().node(id).unwrap().children[0]
    }

    fn bounds_of(board: &BoardDoc, id: NodeId) -> [f64; 4] {
        let r = board.doc().unwrap().world_bounds(id).unwrap();
        [r.x0, r.y0, r.x1, r.y1]
    }

    // The tests the brief asks for.

    #[test]
    fn rectangle_with_label_becomes_board_shape() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([{"type": "rectangle", "id": "a", "x": 10, "y": 20, "width": 160, "height": 60,
                    "label": {"text": "API"}}]),
        );
        let made = layers(&b);
        assert_eq!(made.len(), 1, "one layer: the label is inside it");
        assert_eq!(done.ids, vec![made[0].to_string()]);
        assert!(done.notes.is_empty(), "{:?}", done.notes);
        assert_eq!(shape(&b, made[0]), BoardShape::Rectangle);
        assert_eq!(text_in(&b, made[0]), "API");
        assert_eq!(bounds_of(&b, made[0]), [10.0, 20.0, 170.0, 80.0]);
    }

    #[test]
    fn roundness_makes_rounded_rectangle() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50, "roundness": {"type": 3}},
                {"type": "rectangle", "x": 0, "y": 100, "width": 100, "height": 50, "roundness": null},
                {"type": "rectangle", "x": 0, "y": 200, "width": 100, "height": 50},
                {"type": "rectangle", "x": 0, "y": 300, "width": 100, "height": 50, "roundness": false},
                {"type": "ellipse", "x": 0, "y": 400, "width": 100, "height": 50, "roundness": {"type": 2}},
                {"type": "diamond", "x": 0, "y": 500, "width": 100, "height": 50},
            ]),
        );
        let got: Vec<BoardShape> = (0..6).map(|i| shape(&b, node(&done, i))).collect();
        assert_eq!(
            got,
            [
                BoardShape::RoundedRectangle,
                BoardShape::Rectangle,
                BoardShape::Rectangle,
                BoardShape::Rectangle,
                BoardShape::Ellipse,
                BoardShape::Diamond,
            ]
        );
    }

    #[test]
    fn arrow_binds_to_shapes_by_agent_id() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 160, "height": 60},
                {"type": "rectangle", "id": "b", "x": 400, "y": 0, "width": 160, "height": 60},
                {"type": "arrow", "x": 160, "y": 30, "start": {"id": "a"}, "end": {"id": "b"},
                 "label": {"text": "calls"}},
            ]),
        );
        assert_eq!(
            done.ids.len(),
            3,
            "the arrow's label isn't an element of its own"
        );
        assert!(done.notes.is_empty(), "{:?}", done.notes);
        let doc = b.doc().unwrap();
        let arrow = connector(&b, node(&done, 2));
        assert_eq!(arrow.from.node(), resolve(&doc, "a"));
        assert_eq!(arrow.to.node(), resolve(&doc, "b"));
        assert_eq!(arrow.from.node(), Some(node(&done, 0)));
        assert_eq!(arrow.to.node(), Some(node(&done, 1)));
        let line = doc.node(node(&done, 2)).unwrap();
        assert_eq!(line.stroke.cap_end, StrokeCap::TriangleArrow);
        assert_eq!(line.stroke.cap_start, StrokeCap::None);
        assert!(figcraft_engine::board::label_of(&doc, node(&done, 2)).is_some());
    }

    #[test]
    fn arrow_to_missing_id_is_free() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 160, "height": 60},
                {"type": "arrow", "x": 160, "y": 30, "start": {"id": "a"},
                 "end": {"id": "zzz", "x": 400, "y": 300}},
                {"type": "arrow", "x": 10, "y": 20, "points": [[0, 0], [70, 80]],
                 "start": {"id": "nope"}, "end": {"id": "zzz"}},
            ]),
        );
        assert!(done.notes.is_empty(), "{:?}", done.notes);
        let first = connector(&b, node(&done, 1));
        assert_eq!(first.from.node(), Some(node(&done, 0)));
        assert_eq!(first.to, DocEnd::Point { x: 400.0, y: 300.0 });
        // Neither id exists and neither end has coordinates: both ends are where the arrow is drawn.
        let second = connector(&b, node(&done, 2));
        assert_eq!(second.from, DocEnd::Point { x: 10.0, y: 20.0 });
        assert_eq!(second.to, DocEnd::Point { x: 80.0, y: 100.0 });
    }

    #[test]
    fn agent_ids_win_over_node_ids() {
        let mut b = board();
        let doc = b.doc().unwrap();
        let page = doc.first_page().unwrap();
        assert_eq!(
            page,
            NodeId(1),
            "a new board's page is node 1, so \"1\" is ambiguous"
        );
        let done = add(
            &mut b,
            json!([{"type": "rectangle", "id": "1", "x": 0, "y": 0, "width": 10, "height": 10}]),
        );
        let made = node(&done, 0);
        assert_ne!(made, page);
        let doc = b.doc().unwrap();
        assert_eq!(
            resolve(&doc, "1"),
            Some(made),
            "the agent's element, not the page"
        );
        // An id no agent gave is a node id, in either spelling.
        assert_eq!(resolve(&doc, &made.to_string()), Some(made));
        assert_eq!(resolve(&doc, &format!("0:{made}")), Some(made));
        assert_eq!(resolve(&doc, "99999"), None);
        assert_eq!(resolve(&doc, "nope"), None);
    }

    #[test]
    fn frame_children_move_inside() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "frame", "id": "f", "x": 0, "y": 0, "width": 400, "height": 300, "name": "Group A"},
                {"type": "rectangle", "id": "r", "x": 50, "y": 60, "width": 100, "height": 50, "frameId": "f"},
            ]),
        );
        let (frame, child) = (node(&done, 0), node(&done, 1));
        let doc = b.doc().unwrap();
        assert!(matches!(
            doc.node(frame).unwrap().kind,
            NodeKind::Section { .. }
        ));
        assert_eq!(doc.node(frame).unwrap().name, "Group A");
        assert_eq!(doc.node(child).unwrap().parent, Some(frame));
        assert_eq!(layers(&b), vec![frame], "only the section is on the page");
        assert_eq!(
            bounds_of(&b, child),
            [50.0, 60.0, 150.0, 110.0],
            "it stays where it was"
        );
    }

    #[test]
    fn frame_children_may_come_before_their_frame() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "text", "x": 20, "y": 30, "text": "inside", "frameId": "f"},
                {"type": "sticky", "x": 40, "y": 50, "text": "too", "frameId": "f"},
                {"type": "frame", "id": "f", "x": 0, "y": 0, "width": 400, "height": 300,
                 "label": {"text": "Titled by its label"}},
            ]),
        );
        let doc = b.doc().unwrap();
        let frame = node(&done, 2);
        assert_eq!(doc.node(frame).unwrap().name, "Titled by its label");
        assert_eq!(doc.node(node(&done, 0)).unwrap().parent, Some(frame));
        assert_eq!(doc.node(node(&done, 1)).unwrap().parent, Some(frame));
        assert_eq!(layers(&b), vec![frame]);
    }

    #[test]
    fn frames_are_made_first_so_they_never_cover_what_is_drawn_on_them() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "x": 10, "y": 10, "width": 50, "height": 50},
                {"type": "frame", "x": 0, "y": 0, "width": 400, "height": 300},
            ]),
        );
        assert_eq!(
            layers(&b),
            vec![node(&done, 1), node(&done, 0)],
            "the frame is the lower layer"
        );
    }

    #[test]
    fn a_frame_id_that_cannot_hold_the_element_is_ignored() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "id": "r", "x": 0, "y": 0, "width": 50, "height": 50, "frameId": "nope"},
                {"type": "rectangle", "id": "s", "x": 0, "y": 100, "width": 50, "height": 50, "frameId": "s"},
                {"type": "rectangle", "id": "t", "x": 0, "y": 200, "width": 50, "height": 50, "frameId": "r"},
                {"type": "frame", "id": "f1", "x": 300, "y": 0, "width": 50, "height": 50, "frameId": "f2"},
                {"type": "frame", "id": "f2", "x": 300, "y": 100, "width": 50, "height": 50, "frameId": "f1"},
                {"type": "arrow", "x": 0, "y": 400, "width": 50, "frameId": "f1"},
            ]),
        );
        let doc = b.doc().unwrap();
        let page = doc.first_page().unwrap();
        for i in [0, 1, 2, 5] {
            assert_eq!(
                doc.node(node(&done, i)).unwrap().parent,
                Some(page),
                "element {i}"
            );
        }
        // Two frames naming each other: the first goes into the second, and only that.
        assert_eq!(
            doc.node(node(&done, 3)).unwrap().parent,
            Some(node(&done, 4))
        );
        assert_eq!(doc.node(node(&done, 4)).unwrap().parent, Some(page));
    }

    #[test]
    fn add_is_one_undo_step() {
        let mut b = board();
        add(
            &mut b,
            json!([
                {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50},
                {"type": "sticky", "x": 200, "y": 0, "text": "hi"},
                {"type": "text", "x": 0, "y": -40, "text": "Title"},
            ]),
        );
        assert_eq!(layers(&b).len(), 3);
        b.execute("edit.undo", &json!({})).unwrap();
        assert!(layers(&b).is_empty(), "one undo takes all three away");
        b.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(layers(&b).len(), 3, "and one redo brings them back");
    }

    #[test]
    fn line_with_many_points_is_ink() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "line", "x": 0, "y": 0, "points": [[0, 0], [50, 0], [50, 50], [100, 50]]},
                {"type": "freedraw", "x": 200, "y": 0, "points": [[0, 0], [10, 10]]},
                {"type": "line", "x": 0, "y": 200, "points": [[0, 0], [100, 50]]},
                {"type": "line", "x": 0, "y": 300, "width": 80},
            ]),
        );
        assert_eq!(board_item(&b, node(&done, 0)), BoardItem::Ink);
        assert_eq!(
            board_item(&b, node(&done, 1)),
            BoardItem::Ink,
            "a freedraw is ink however short"
        );
        for i in [2, 3] {
            let doc = b.doc().unwrap();
            let line = doc.node(node(&done, i)).unwrap();
            assert!(
                matches!(line.board, Some(BoardItem::Connector(_))),
                "line {i}"
            );
            assert_eq!(line.stroke.cap_start, StrokeCap::None);
            assert_eq!(
                line.stroke.cap_end,
                StrokeCap::None,
                "a line has no arrowhead"
            );
        }
    }

    // What the rulings and the spec add.

    #[test]
    fn points_are_relative_to_x_y_and_the_offset_moves_everything() {
        let mut b = board();
        let items = skeletons(json!([
            {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
            {"type": "freedraw", "x": 100, "y": 200, "points": [[0, 0], [10, 0], [10, 10]]},
            {"type": "arrow", "x": 10, "y": 20, "points": [[0, 0], [100, 50]]},
            {"type": "arrow", "x": 0, "y": 0, "start": {"x": 300, "y": 40}, "end": {"x": 310, "y": 90}},
        ]));
        let done = apply(&mut b, &items, (5.0, 7.0)).unwrap();
        assert_eq!(bounds_of(&b, node(&done, 0)), [15.0, 27.0, 115.0, 77.0]);
        // Ink is drawn from its points: (105, 207) to (115, 217).
        assert_eq!(bounds_of(&b, node(&done, 1)), [105.0, 207.0, 115.0, 217.0]);
        let moved = connector(&b, node(&done, 2));
        assert_eq!(moved.from, DocEnd::Point { x: 15.0, y: 27.0 });
        assert_eq!(moved.to, DocEnd::Point { x: 115.0, y: 77.0 });
        let free = connector(&b, node(&done, 3));
        assert_eq!(
            free.from,
            DocEnd::Point { x: 305.0, y: 47.0 },
            "free ends are in page coordinates"
        );
        assert_eq!(free.to, DocEnd::Point { x: 315.0, y: 97.0 });
    }

    #[test]
    fn the_offset_moves_every_kind_of_element() {
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='30'>\
                   <rect width='40' height='30'/></svg>";
        let items = skeletons(json!([
            {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
            {"type": "sticky", "x": 10, "y": 100, "text": "s"},
            {"type": "text", "x": 10, "y": 400, "text": "t"},
            {"type": "frame", "x": 10, "y": 500, "width": 200, "height": 100},
            {"type": "image", "x": 10, "y": 700, "svg": svg},
            {"type": "image", "x": 10, "y": 800, "data": PNG, "width": 20, "height": 10},
        ]));
        let (mut moved, mut still) = (board(), board());
        let moved_by = apply(&mut moved, &items, (1000.0, 2000.0)).unwrap();
        let still_at = apply(&mut still, &items, (0.0, 0.0)).unwrap();
        assert_eq!(moved_by.ids.len(), 6);
        for i in 0..6 {
            let (a, b) = (
                bounds_of(&moved, node(&moved_by, i)),
                bounds_of(&still, node(&still_at, i)),
            );
            assert_eq!([a[0] - 1000.0, a[1] - 2000.0], [b[0], b[1]], "element {i}");
            assert_eq!(
                [a[2] - a[0], a[3] - a[1]],
                [b[2] - b[0], b[3] - b[1]],
                "its size, element {i}"
            );
        }
    }

    #[test]
    fn a_free_end_without_coordinates_takes_the_position_the_arrow_is_drawn_at() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "arrow", "x": 10, "y": 20, "width": 160, "height": 0},
                {"type": "arrow", "x": 10, "y": 50},
                {"type": "line", "x": 10, "y": 80, "points": [[0, 0], [100, 50]]},
                {"type": "arrow", "x": 10, "y": 120, "points": [[0, 0], [40, 0], [40, 30]]},
                {"type": "arrow", "x": 10, "y": 200, "height": 60},
                {"type": "arrow", "x": 10, "y": 300, "width": 30, "height": 40},
            ]),
        );
        let ends = |i| {
            let c = connector(&b, node(&done, i));
            (c.from, c.to)
        };
        let point = |x, y| DocEnd::Point { x, y };
        assert_eq!(ends(0), (point(10.0, 20.0), point(170.0, 20.0)));
        assert_eq!(
            ends(1),
            (point(10.0, 50.0), point(110.0, 50.0)),
            "100 to the right by default"
        );
        assert_eq!(ends(2), (point(10.0, 80.0), point(110.0, 130.0)));
        assert_eq!(
            ends(3),
            (point(10.0, 120.0), point(50.0, 150.0)),
            "the last point; the bend is lost"
        );
        assert_eq!(
            ends(4),
            (point(10.0, 200.0), point(10.0, 260.0)),
            "straight down"
        );
        assert_eq!(ends(5), (point(10.0, 300.0), point(40.0, 340.0)));
    }

    #[test]
    fn arrows_can_name_node_ids_and_pages_are_not_joined() {
        let mut b = board();
        let first = add(
            &mut b,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "old"}]),
        );
        let old = node(&first, 0);
        let done = add(
            &mut b,
            json!([
                {"type": "sticky", "id": "new", "x": 400, "y": 0, "text": "new"},
                {"type": "arrow", "x": 0, "y": 0, "start": {"id": old.to_string()}, "end": {"id": "new"}},
                {"type": "arrow", "x": 0, "y": 100, "start": {"id": "1", "x": 5, "y": 6}, "end": {"x": 90, "y": 100}},
            ]),
        );
        assert!(done.notes.is_empty(), "{:?}", done.notes);
        let joined = connector(&b, node(&done, 1));
        assert_eq!(joined.from.node(), Some(old));
        assert_eq!(joined.to.node(), Some(node(&done, 0)));
        let page = connector(&b, node(&done, 2));
        assert_eq!(
            page.from,
            DocEnd::Point { x: 5.0, y: 6.0 },
            "the page can't be joined: the end stays free"
        );
    }

    #[test]
    fn an_arrow_cannot_join_a_connector_or_one_layer_twice() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 100, "height": 50},
                {"type": "rectangle", "id": "b", "x": 300, "y": 0, "width": 100, "height": 50},
                {"type": "arrow", "id": "c", "x": 0, "y": 0, "start": {"id": "a"}, "end": {"id": "b"}},
                {"type": "arrow", "x": 0, "y": 100, "points": [[0, 0], [60, 40]],
                 "start": {"id": "a"}, "end": {"id": "a"}},
                {"type": "arrow", "x": 0, "y": 300, "start": {"id": "c"}, "end": {"id": "b"}},
            ]),
        );
        assert_eq!(done.ids.len(), 5);
        assert_eq!(done.notes.len(), 1, "{:?}", done.notes);
        assert!(done.notes[0].contains("elements[3]"), "{:?}", done.notes);
        let looped = connector(&b, node(&done, 3));
        assert_eq!(looped.from.node(), Some(node(&done, 0)), "the start stays");
        assert_eq!(
            looped.to,
            DocEnd::Point { x: 60.0, y: 140.0 },
            "the end is left where it was drawn"
        );
        let onto_arrow = connector(&b, node(&done, 4));
        assert_eq!(
            onto_arrow.from,
            DocEnd::Point { x: 0.0, y: 300.0 },
            "an arrow can't be an end"
        );
        assert_eq!(onto_arrow.to.node(), Some(node(&done, 1)));
    }

    #[test]
    fn what_has_no_length_is_skipped_with_a_note() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "arrow", "x": 5, "y": 5, "width": 0, "height": 0},
                {"type": "line", "x": 0, "y": 0, "points": [[0, 0], [0, 0]]},
                {"type": "freedraw", "x": 0, "y": 0, "points": [[0, 0]]},
                {"type": "freedraw", "x": 0, "y": 0, "points": [[3, 3], [3, 3], [3, 3]]},
                {"type": "line", "x": 0, "y": 0, "points": [[0, 0], [0, 0], [0, 0]]},
                {"type": "freedraw", "x": 0, "y": 0},
            ]),
        );
        assert!(done.ids.is_empty());
        assert!(layers(&b).is_empty());
        assert_eq!(done.notes.len(), 6, "{:?}", done.notes);
        for (i, note) in done.notes.iter().enumerate() {
            assert!(note.contains(&format!("elements[{i}]")), "{note}");
        }
    }

    #[test]
    fn unknown_types_are_skipped_with_notes() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "laser", "x": 0, "y": 0},
                {"type": "RECTANGLE", "x": 0, "y": 0, "width": 10, "height": 10},
                {"x": 1, "y": 2},
                {"type": "embeddable", "x": 0, "y": 0},
            ]),
        );
        assert_eq!(done.ids.len(), 1, "types match in any case");
        assert_eq!(shape(&b, node(&done, 0)), BoardShape::Rectangle);
        assert_eq!(done.notes.len(), 3, "{:?}", done.notes);
        assert!(done.notes[0].contains("elements[0]") && done.notes[0].contains("laser"));
        assert!(done.notes[1].contains("elements[2]") && done.notes[1].contains("no type"));
        assert!(done.notes[2].contains("elements[3]") && done.notes[2].contains("embeddable"));
    }

    #[test]
    fn ids_come_back_in_input_order_whatever_the_order_they_are_made_in() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "arrow", "x": 0, "y": 0, "start": {"id": "a"}, "end": {"id": "b"}},
                {"type": "text", "x": 0, "y": -40, "text": "Title"},
                {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 100, "height": 50},
                {"type": "sticky", "id": "b", "x": 300, "y": 0, "text": "hi"},
                {"type": "frame", "x": -50, "y": -80, "width": 500, "height": 300},
            ]),
        );
        assert_eq!(done.ids.len(), 5);
        let doc = b.doc().unwrap();
        let kind = |i| doc.node(node(&done, i)).unwrap();
        let arrow = connector(&b, node(&done, 0));
        assert_eq!(arrow.from.node(), Some(node(&done, 2)));
        assert_eq!(arrow.to.node(), Some(node(&done, 3)));
        assert!(matches!(&kind(1).kind, NodeKind::Text(t) if t.characters == "Title"));
        assert_eq!(shape(&b, node(&done, 2)), BoardShape::Rectangle);
        assert!(matches!(kind(3).board, Some(BoardItem::Sticky { .. })));
        assert!(matches!(kind(4).kind, NodeKind::Section { .. }));
    }

    #[test]
    fn ids_of_one_call_shadow_those_already_on_the_board() {
        let mut b = board();
        add(
            &mut b,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 10, "height": 10},
                {"type": "rectangle", "id": "old", "x": 0, "y": 500, "width": 10, "height": 10},
            ]),
        );
        let before = resolve(&b.doc().unwrap(), "a").unwrap();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "id": "a", "x": 0, "y": 100, "width": 10, "height": 10},
                {"type": "rectangle", "id": "b", "x": 0, "y": 200, "width": 10, "height": 10},
                {"type": "arrow", "x": 0, "y": 0, "start": {"id": "a"}, "end": {"id": "b"}},
                {"type": "arrow", "x": 0, "y": 0, "start": {"id": "old"}, "end": {"id": "b"}},
            ]),
        );
        let (a, old) = (node(&done, 0), resolve(&b.doc().unwrap(), "old").unwrap());
        assert_ne!(a, before);
        assert_eq!(
            connector(&b, node(&done, 2)).from.node(),
            Some(a),
            "this call's `a`"
        );
        assert_eq!(
            connector(&b, node(&done, 3)).from.node(),
            Some(old),
            "an earlier call's `old`"
        );
        assert_eq!(
            resolve(&b.doc().unwrap(), "a"),
            Some(a),
            "the newest element holds an id"
        );
    }

    #[test]
    fn the_agent_id_is_kept_in_plugin_data() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "text", "id": "t", "x": 0, "y": 0, "text": "hi"},
                {"type": "text", "id": "", "x": 0, "y": 40, "text": "no id"},
            ]),
        );
        let doc = b.doc().unwrap();
        let data = &doc.node(node(&done, 0)).unwrap().plugin_data;
        assert_eq!(data.get(SAWIT_ID).map(String::as_str), Some("t"));
        assert!(
            doc.node(node(&done, 1)).unwrap().plugin_data.is_empty(),
            "an empty id is none"
        );
    }

    #[test]
    fn sticky_and_text_come_in_with_their_text() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "sticky", "x": 0, "y": 0, "text": "remember", "backgroundColor": "green"},
                {"type": "text", "x": 0, "y": -40, "text": "Title"},
                {"type": "text", "x": 0, "y": -80, "label": {"text": "from the label"}, "width": 200},
                {"type": "sticky", "x": 300, "y": 0, "label": {"text": "label wins here"}},
            ]),
        );
        assert!(matches!(
            board_item(&b, node(&done, 0)),
            BoardItem::Sticky { .. }
        ));
        assert_eq!(text_in(&b, node(&done, 0)), "remember");
        let doc = b.doc().unwrap();
        let text = |i| match &doc.node(node(&done, i)).unwrap().kind {
            NodeKind::Text(t) => t.characters.clone(),
            other => panic!("not text: {other:?}"),
        };
        assert_eq!(text(1), "Title");
        assert_eq!(text(2), "from the label");
        assert_eq!(text_in(&b, node(&done, 3)), "label wins here");
        assert_eq!(bounds_of(&b, node(&done, 1))[..2], [0.0, -40.0]);
    }

    #[test]
    fn shape_colours_follow_the_spec() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50, "backgroundColor": "#ffc9c9"},
                {"type": "rectangle", "x": 0, "y": 100, "width": 100, "height": 50, "backgroundColor": "blue"},
                {"type": "rectangle", "x": 0, "y": 200, "width": 100, "height": 50, "strokeColor": "#e03131"},
                {"type": "rectangle", "x": 0, "y": 300, "width": 100, "height": 50,
                 "backgroundColor": "transparent", "strokeColor": "#1971c2"},
                {"type": "ellipse", "x": 0, "y": 400, "width": 100, "height": 50,
                 "backgroundColor": "#ffc9c9", "strokeColor": "#e03131"},
            ]),
        );
        let drawn = |i| outline(&b, node(&done, i));
        assert_eq!(fill(&b, drawn(0)), css("#ffc9c9"));
        assert_eq!(
            fill(&b, drawn(1)),
            Some(palette("blue")),
            "board colour names work"
        );
        assert_ne!(
            stroke(&b, drawn(1)),
            Some(palette("blue")),
            "its line is the deeper shade"
        );
        assert_eq!(
            stroke(&b, drawn(2)),
            css("#e03131"),
            "the outline is stroked, not the frame"
        );
        assert!(
            b.doc()
                .unwrap()
                .node(node(&done, 2))
                .unwrap()
                .strokes
                .is_empty()
        );
        assert!(
            b.doc().unwrap().node(drawn(3)).unwrap().fills.is_empty(),
            "transparent is no fill"
        );
        assert_eq!(
            stroke(&b, drawn(3)),
            css("#1971c2"),
            "and the shape is still drawn"
        );
        assert_eq!(fill(&b, drawn(4)), css("#ffc9c9"));
        assert_eq!(
            stroke(&b, drawn(4)),
            css("#e03131"),
            "the stroke colour wins over the shade"
        );
    }

    #[test]
    fn text_is_coloured_by_stroke_color_and_has_no_background() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "text", "x": 0, "y": 0, "text": "red", "strokeColor": "#e03131",
                 "backgroundColor": "transparent"},
                {"type": "text", "x": 0, "y": 40, "text": "plain", "backgroundColor": "#ffc9c9"},
            ]),
        );
        let red = node(&done, 0);
        assert_eq!(
            fill(&b, red),
            css("#e03131"),
            "Excalidraw's text colour is its stroke"
        );
        assert!(
            b.doc().unwrap().node(red).unwrap().strokes.is_empty(),
            "the glyphs aren't outlined"
        );
        let plain = node(&done, 1);
        assert_ne!(
            fill(&b, plain),
            css("#ffc9c9"),
            "a background is never drawn behind text"
        );
        assert!(
            !b.doc().unwrap().node(plain).unwrap().fills.is_empty(),
            "and the text stays visible"
        );
    }

    #[test]
    fn connector_and_ink_colours_are_their_stroke_and_never_the_background() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "arrow", "x": 0, "y": 0, "width": 100, "strokeColor": "#e03131", "backgroundColor": "#ffc9c9"},
                {"type": "arrow", "x": 0, "y": 50, "width": 100, "backgroundColor": "#ffc9c9"},
                {"type": "freedraw", "x": 0, "y": 100, "points": [[0, 0], [30, 30]],
                 "strokeColor": "#1971c2", "backgroundColor": "#ffc9c9"},
                {"type": "line", "x": 0, "y": 150, "points": [[0, 0], [10, 0], [10, 10]],
                 "backgroundColor": "#ffc9c9"},
            ]),
        );
        let doc = b.doc().unwrap();
        assert_eq!(stroke(&b, node(&done, 0)), css("#e03131"));
        assert!(doc.node(node(&done, 0)).unwrap().fills.is_empty());
        let untouched = stroke(&b, node(&done, 1));
        assert!(
            untouched.is_some() && untouched != css("#ffc9c9"),
            "the default line stays"
        );
        assert_eq!(stroke(&b, node(&done, 2)), css("#1971c2"));
        assert!(doc.node(node(&done, 2)).unwrap().fills.is_empty());
        assert_ne!(stroke(&b, node(&done, 3)), css("#ffc9c9"));
    }

    #[test]
    fn sticky_and_frame_colours() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "sticky", "x": 0, "y": 0, "text": "a", "backgroundColor": "green"},
                {"type": "sticky", "x": 300, "y": 0, "text": "b", "backgroundColor": "transparent"},
                {"type": "sticky", "x": 600, "y": 0, "text": "c", "strokeColor": "#e03131"},
                {"type": "frame", "x": 0, "y": 400, "width": 200, "height": 100,
                 "backgroundColor": "#e7f5ff", "strokeColor": "#1971c2"},
                {"type": "frame", "x": 300, "y": 400, "width": 200, "height": 100, "backgroundColor": "transparent"},
            ]),
        );
        assert_eq!(fill(&b, node(&done, 0)), Some(palette("green")));
        let doc = b.doc().unwrap();
        assert!(doc.node(node(&done, 1)).unwrap().fills.is_empty());
        assert_eq!(stroke(&b, node(&done, 2)), css("#e03131"));
        assert_eq!(fill(&b, node(&done, 3)), css("#e7f5ff"));
        assert_eq!(stroke(&b, node(&done, 3)), css("#1971c2"));
        assert!(
            doc.node(node(&done, 4)).unwrap().fills.is_empty(),
            "transparent is no fill"
        );
    }

    #[test]
    fn set_colors_is_one_helper_for_every_kind_of_layer() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50},
                {"type": "arrow", "x": 0, "y": 100, "width": 100},
                {"type": "text", "x": 0, "y": 200, "text": "t"},
            ]),
        );
        let (shape, arrow, text) = (node(&done, 0), node(&done, 1), node(&done, 2));
        let mut paint = |id, background, stroke| {
            set_colors(&mut b.session, id, background, stroke).unwrap();
        };
        paint(shape, Some("pink"), Some("#000000"));
        paint(arrow, Some("pink"), Some("#e03131"));
        paint(text, Some("pink"), None);
        // An empty colour is none: nothing changes.
        paint(text, Some(""), Some("  "));
        assert_eq!(fill(&b, outline(&b, shape)), Some(palette("pink")));
        assert_eq!(stroke(&b, outline(&b, shape)), css("#000000"));
        assert_eq!(stroke(&b, arrow), css("#e03131"));
        assert_ne!(fill(&b, text), Some(palette("pink")));
        // A colour the engine doesn't know is its error.
        assert!(set_colors(&mut b.session, shape, Some("banana"), None).is_err());
        assert!(set_colors(&mut b.session, NodeId(99_999), Some("pink"), None).is_err());
    }

    #[test]
    fn svg_comes_in_as_an_editable_frame() {
        let mut b = board();
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='30'>\
                   <rect width='40' height='30' fill='red'/></svg>";
        let done = add(
            &mut b,
            json!([{"type": "image", "x": 10, "y": 20, "svg": svg, "name": "diagram.svg", "width": 40, "height": 30},
                   {"type": "image", "x": 100, "y": 20, "svg": svg}]),
        );
        assert!(done.notes.is_empty(), "{:?}", done.notes);
        assert_eq!(layers(&b).len(), 2);
        let first = bounds_of(&b, node(&done, 0));
        assert_eq!(first, [10.0, 20.0, 50.0, 50.0]);
        assert_eq!(
            bounds_of(&b, node(&done, 1))[..2],
            [100.0, 20.0],
            "with or without a name"
        );
        let doc = b.doc().unwrap();
        assert!(doc.node(node(&done, 0)).unwrap().kind.is_frame_like());
    }

    #[test]
    fn a_picture_takes_the_size_the_skeleton_gives_it() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "image", "x": 10, "y": 20, "data": PNG, "name": "dot.png", "width": 40, "height": 20},
                {"type": "image", "x": 10, "y": 100, "data": PNG, "width": 30},
                {"type": "image", "x": 10, "y": 200, "data": format!("data:image/png;base64,{PNG}")},
            ]),
        );
        assert!(done.notes.is_empty(), "{:?}", done.notes);
        let doc = b.doc().unwrap();
        let picture = doc.node(node(&done, 0)).unwrap();
        assert!(matches!(
            picture.fills.first().map(|p| &p.kind),
            Some(PaintKind::Image(_))
        ));
        assert_eq!(bounds_of(&b, node(&done, 0)), [10.0, 20.0, 50.0, 40.0]);
        assert_eq!(
            bounds_of(&b, node(&done, 1)),
            [10.0, 100.0, 40.0, 130.0],
            "a lone width keeps the proportion"
        );
        let natural = bounds_of(&b, node(&done, 2));
        assert_eq!(natural[..2], [10.0, 200.0], "a data URL works as it is");
        assert!(natural[2] > natural[0] && natural[3] > natural[1]);
    }

    #[test]
    fn a_picture_that_cannot_be_imported_is_noted_and_the_rest_still_comes_in() {
        let mut b = board();
        let done = add(
            &mut b,
            json!([
                {"type": "image", "x": 0, "y": 0, "data": "!!not base64!!", "name": "broken.png"},
                {"type": "image", "x": 0, "y": 0, "data": "AAAA", "name": "garbage.png"},
                {"type": "image", "x": 0, "y": 0, "svg": "<not svg>"},
                {"type": "image", "x": 0, "y": 0},
                {"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10},
            ]),
        );
        assert_eq!(done.ids.len(), 1);
        assert_eq!(layers(&b).len(), 1);
        assert_eq!(done.notes.len(), 4, "{:?}", done.notes);
        for (note, element) in done.notes.iter().zip(0..4) {
            assert!(note.contains(&format!("elements[{element}]")), "{note}");
        }
    }

    #[test]
    fn an_engine_error_fails_the_whole_call_and_changes_nothing() {
        let mut b = board();
        let items = skeletons(json!([
            {"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50},
            {"type": "sticky", "x": 200, "y": 0, "text": "fine"},
            {"type": "rectangle", "x": 0, "y": 100, "width": -5, "height": 50},
        ]));
        let before = b.doc().unwrap();
        let err = apply(&mut b, &items, (0.0, 0.0)).unwrap_err();
        assert!(
            matches!(&err, BoardError::Engine(m) if m.contains("elements[2]") && m.contains("rectangle")),
            "{err}"
        );
        assert!(layers(&b).is_empty(), "the call is rolled back");
        assert!(
            Arc::ptr_eq(&before, &b.doc().unwrap()),
            "to the very document it started from"
        );
        let items = skeletons(
            json!([{"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
                                      "backgroundColor": "banana"}]),
        );
        assert!(
            apply(&mut b, &items, (0.0, 0.0)).is_err(),
            "a colour the engine doesn't know"
        );
        assert!(layers(&b).is_empty());
        assert!(Arc::ptr_eq(&before, &b.doc().unwrap()));
    }

    #[test]
    fn applying_nothing_changes_nothing() {
        let mut b = board();
        let revision = b.revision();
        let done = apply(&mut b, &[], (10.0, 10.0)).unwrap();
        assert_eq!(done, Applied::default());
        assert_eq!(b.revision(), revision);
        b.execute("edit.undo", &json!({})).unwrap();
    }

    #[test]
    fn the_selection_is_left_as_it_was() {
        let mut b = board();
        let first = add(
            &mut b,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );
        let mine = node(&first, 0);
        b.session.set_selection(vec![mine]);
        add(
            &mut b,
            json!([
                {"type": "rectangle", "x": 300, "y": 0, "width": 50, "height": 50},
                {"type": "text", "x": 300, "y": 100, "text": "new"},
                {"type": "arrow", "x": 0, "y": 200, "width": 50},
            ]),
        );
        assert_eq!(b.session.selection(), vec![mine]);
        b.session.set_selection(Vec::new());
        add(
            &mut b,
            json!([{"type": "text", "x": 0, "y": 0, "text": "more"}]),
        );
        assert!(b.session.selection().is_empty());
    }

    #[test]
    fn a_design_file_refuses_board_elements_and_stays_unchanged() {
        let design = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
        let mut b = BoardDoc::from_bytes(&design).unwrap();
        let items =
            skeletons(json!([{"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10}]));
        assert!(matches!(
            apply(&mut b, &items, (0.0, 0.0)),
            Err(BoardError::Engine(_))
        ));
        assert!(layers(&b).is_empty());
    }

    #[test]
    fn skeleton_reads_the_mcp_json() {
        let s: Skeleton = serde_json::from_value(json!({
            "type": "arrow", "id": "e", "x": 160, "y": 30, "width": 200, "height": 10,
            "start": {"id": "a"}, "end": {"id": "b", "x": 1.5, "y": 2.5},
            "points": [[0, 0], [200, 10]],
            "strokeColor": "#e03131", "backgroundColor": "#ffc9c9", "roundness": {"type": 3},
            "frameId": "f", "label": {"text": "go", "fontSize": 16},
            "fontSize": 20, "roughness": 1, "someFutureField": [1, 2, 3]
        }))
        .unwrap();
        assert_eq!(s.kind, "arrow");
        assert_eq!((s.id.as_deref(), s.x, s.y), (Some("e"), 160.0, 30.0));
        assert_eq!((s.width, s.height), (Some(200.0), Some(10.0)));
        assert_eq!(
            s.start,
            Some(End {
                id: Some("a".into()),
                x: None,
                y: None
            })
        );
        assert_eq!(
            s.end,
            Some(End {
                id: Some("b".into()),
                x: Some(1.5),
                y: Some(2.5)
            })
        );
        assert_eq!(s.points, Some(vec![[0.0, 0.0], [200.0, 10.0]]));
        assert_eq!(s.stroke_color.as_deref(), Some("#e03131"));
        assert_eq!(s.background_color.as_deref(), Some("#ffc9c9"));
        assert_eq!(s.roundness, Some(json!({"type": 3})));
        assert_eq!(s.frame_id.as_deref(), Some("f"));
        assert_eq!(s.label, Some(Label { text: "go".into() }));
        // Anything missing is its default, and `null` is missing.
        let bare: Skeleton =
            serde_json::from_value(json!({"roundness": null, "label": {}})).unwrap();
        assert_eq!(
            bare,
            Skeleton {
                label: Some(Label::default()),
                ..Skeleton::default()
            }
        );
        // It writes the same names back.
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["type"], "arrow");
        assert_eq!(back["strokeColor"], "#e03131");
        assert_eq!(back["frameId"], "f");
        assert_eq!(serde_json::from_value::<Skeleton>(back).unwrap(), s);
    }

    #[test]
    fn bounds_cover_every_element_that_will_be_drawn() {
        assert_eq!(bounds(&[]), None);
        let items = skeletons(json!([
            {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
            {"type": "ellipse", "x": -30, "y": 40},
            {"type": "sticky", "x": 500, "y": 0, "text": "s"},
            {"type": "text", "x": 5, "y": -60, "text": "t"},
            {"type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [100, 400]]},
            {"type": "arrow", "x": 0, "y": 0, "end": {"x": -200, "y": 10}},
            {"type": "frame", "x": 0, "y": 0},
            {"type": "laser", "x": 9999, "y": 9999},
        ]));
        // Left: the free end at -200. Top: the text at -60. Right: the sticky's 200. Bottom: the arrow's 400.
        assert_eq!(bounds(&items), Some((-200.0, -60.0, 700.0, 400.0)));
        assert_eq!(
            bounds(&skeletons(json!([{"type": "laser", "x": 1, "y": 1}]))),
            None
        );
        assert_eq!(
            bounds(&skeletons(
                json!([{"type": "text", "x": 3, "y": 4, "text": "t"}])
            )),
            Some((3.0, 4.0, 3.0, 4.0))
        );
    }

    #[test]
    fn a_hundred_elements_are_one_quick_call() {
        let mut b = board();
        let mut items = vec![
            json!({"type": "rectangle", "id": "n0", "x": 0, "y": 0, "width": 50, "height": 30}),
        ];
        for i in 1..100 {
            items.push(
                json!({"type": "rectangle", "id": format!("n{i}"), "x": f64::from(i) * 60.0, "y": 0,
                              "width": 50, "height": 30, "label": {"text": format!("n{i}")}}),
            );
            items.push(json!({"type": "arrow", "x": 0, "y": 0,
                              "start": {"id": format!("n{}", i - 1)}, "end": {"id": format!("n{i}")}}));
        }
        let done = add(&mut b, Value::Array(items));
        assert_eq!(done.ids.len(), 199);
        assert!(done.notes.is_empty(), "{:?}", done.notes);
    }
}
