//! Element skeletons: the JSON that agents send to `canvas_add_elements`, that the Mermaid
//! converter makes from a diagram and that the Excalidraw importer makes from a scene. [`apply`]
//! turns a list of them into board objects (shapes, stickies, text, sections, images, connectors
//! and ink) in one undo step, and [`resolve`] finds an element by the id an agent gave it.
//!
//! A [`Skeleton`] has Excalidraw's shape, so that agents' existing calls keep working. Its
//! coordinates do too: `x`, `y` and the `x`, `y` of a free [`End`] are page coordinates, and
//! `points` are relative to the element's own `x`, `y`. The `offset` of [`apply`] moves all of
//! them.

mod colors;
mod element;
mod layers;
mod maker;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use figcraft_doc::{Document, Node, NodeId};
use figcraft_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::doc::{BoardDoc, BoardError};
use element::{Element, line_extent};

pub use colors::set_colors;

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
    /// The font size of a `text`, in px (Excalidraw's `fontSize`); the engine's default without
    /// one.
    #[serde(rename = "fontSize", skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
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
    /// Whether an SVG `image` comes in as a group of its layers rather than a frame: the pictures
    /// the Mermaid converter cuts from a diagram (an icon, a turned text, the picture under its
    /// shapes), on which a frame's title, the file's name, would label every one. Not on the wire.
    #[serde(skip)]
    pub as_group: bool,
}

/// The text inside a shape, or on an arrow or a line.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Label {
    pub text: String,
    /// The font size of a shape's label, in px (Excalidraw's `label.fontSize`); the engine's
    /// default, 16, without one.
    #[serde(rename = "fontSize", skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
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
///
/// What a skeleton doesn't name looks the same on every board, whatever the user last picked in
/// the toolbar or set in Preferences, so one call draws the same in the app as with no app: shapes
/// and stickies in the engine's default sticky colour (yellow), stickies unsigned, ink in the
/// engine's default marker colour, connectors straight. What it does name is applied on top of
/// that (see [`set_colors`]).
pub fn apply(
    board: &mut BoardDoc,
    items: &[Skeleton],
    offset: (f64, f64),
) -> Result<Applied, BoardError> {
    let selection = board.session.selection();
    let applied = crate::doc::grouped(&mut board.session, "Add elements", |session| {
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

/// The id an agent gave `node`.
fn agent_id(node: &Node) -> Option<&str> {
    node.plugin_data.get(SAWIT_ID).map(String::as_str)
}

/// The node `id` names, if the document has it: `12` or Figma's `0:12`.
fn node_id(doc: &Document, id: &str) -> Option<NodeId> {
    id.parse::<NodeId>().ok().filter(|node| doc.contains(*node))
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
