//! Excalidraw scenes, the JSON of an `.excalidraw` file, as board objects. The old project canvas
//! (`canvas.excalidraw`) is imported once when the native canvas first opens
//! ([`store::open_with_issues`](crate::store::open_with_issues)), and an agent can replace the
//! canvas with a scene or a file (the `replace` op).
//!
//! [`to_skeletons`] reads a scene as the element [`Skeleton`]s the canvas draws, [`import`] draws
//! them on a board, [`to_board`] makes a board of them, and [`read_file`] reads a scene from a
//! file ([`read_file_or_blank`] for opening a canvas: a file with nothing in it is a blank one).
//! An Excalidraw element has the shape of a skeleton (today's agent schema is Excalidraw's), so
//! the mapping is direct:
//! - A rectangle, an ellipse or a diamond is the shape of that name, and a rectangle with
//!   `roundness` has rounded corners. A text bound to it (`containerId`) is its `label`, and so is
//!   one bound to an arrow. A text whose container isn't in the scene, or can't hold one, is a
//!   text of its own.
//! - A text is coloured by its `strokeColor`, as in Excalidraw: its `backgroundColor` is never
//!   drawn. It is as wide as its longest line, unless Excalidraw fixed its width.
//! - An arrow or a line keeps its `points`, relative to its `x`, `y` as in Excalidraw. An end bound
//!   to an element the canvas draws (`startBinding`, `endBinding`) joins it, and any other end is
//!   free, at the first or the last point. A line through more than two points, and a freedraw,
//!   are ink ([`skeleton::apply`]).
//! - Two bindings are kept as Excalidraw draws them, where they are, since a connector joined there
//!   would be drawn otherwise: an end bound to an element it runs beside, as the old canvas's
//!   sequence messages are to the note under them, is free; and so are both ends of a line bound
//!   to one element at both ends, a loop back to it. Such a loop, if it is an arrow with no label,
//!   is ink along its route with its head drawn in it. A line runs beside an element when
//!   Excalidraw says so (a `focus` outside -1 to 1) and the drawing agrees (carried on past that
//!   end, the line misses the element's box). Excalidraw's word alone isn't enough: it puts some
//!   lines straight into a wide diamond past 1.
//! - An arrow points where its head is (`startArrowhead`, `endArrowhead`). With a head at its end,
//!   or at both ends, it is an arrow. With a head at its start only it is drawn the other way
//!   round, from its end to its start, so the head stays where it was. With no head it is a line,
//!   which is what the links of Mermaid diagrams (`A --- B`) are on the old canvas; if it bends and
//!   joins something, or has a label, it keeps only its first and last point, since ink joins
//!   nothing and has no text. A connector has one head, so the kind of head and a second one at
//!   the start aren't carried.
//! - A frame is a section, and the elements that name it in `frameId` are inside it.
//! - An image shows the picture in `files`, in the entry its `fileId` names.
//!
//! Elements marked as deleted are skipped. Embeds (`embeddable`, `iframe`, `magicframe`), and any
//! type the canvas doesn't know, are skipped with a note, never an error. What an element says of
//! how it is drawn and a skeleton has no place for isn't carried over: rotation, opacity, stroke
//! width and style, fill style, the kind of arrowhead, fonts, text alignment, groups and links.
//!
//! What goes to the engine is made safe first, because one element it refuses would fail the
//! whole import: a colour it can't read is left out, and a side it can't take is brought into its
//! range. An element that is refused anyway is skipped with a note ([`import`]).

mod arrows;
mod draw;
mod fields;
mod file;
mod items;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use figcraft_engine::Session;
use serde_json::{Map, Value};

use crate::doc::{BoardDoc, BoardError, DocKind};
use crate::skeleton::{self, Skeleton};
use draw::{draw_apart, named};
use items::{Item, Kind, Labels, Skipped};

pub use file::{MAX_SCENE, read_file, read_file_or_blank};

/// A JSON object: an element of a scene, or its `files`.
type Fields = Map<String, Value>;

/// The sides `board.shape` takes (from 1 to 100000). Excalidraw keeps elements outside of that, a
/// rectangle of no width that looks like a line, say, and a side the engine refuses would fail the
/// whole import, so every box is held to it.
const MIN_SIDE: f64 = 1.0;
const MAX_SIDE: f64 = 100_000.0;

/// What some editors write at the start of a text file.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// The longest side of the head that Excalidraw draws for a `triangle` arrowhead, and how far each
/// side turns off the arrow (`getArrowheadSize` and `getArrowheadAngle` in Excalidraw).
const HEAD_LENGTH: f64 = 15.0;
const HEAD_ANGLE: f64 = 25.0;

/// What is said of a scene that isn't one.
const NO_ELEMENTS: &str =
    r#"an Excalidraw scene needs an `elements` array, like {"type": "excalidraw", "elements": []}"#;

/// A scene read as skeletons.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    /// What the scene draws, in the order of its elements, ready for [`skeleton::apply`].
    pub skeletons: Vec<Skeleton>,
    /// What was left out, one sentence each.
    pub notes: Vec<String>,
}

/// The element skeletons of `scene`, an Excalidraw scene (`{elements, files?, ...}`).
///
/// An error only when `scene` has no `elements` array. Everything else that can't be drawn is
/// left out with a note: elements of a type the canvas doesn't know, embeds, images whose picture
/// isn't in `files`. Nothing is read from disk, and nothing is drawn.
pub fn to_skeletons(scene: &Value) -> Result<Imported, String> {
    let elements = scene
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| NO_ELEMENTS.to_string())?;
    let files = scene.get("files").and_then(Value::as_object);
    let mut skipped = Skipped::default();
    let items: Vec<Item> = elements
        .iter()
        .filter_map(|element| Item::read(element, files, &mut skipped))
        .collect();

    let labels = Labels::find(&items);
    // An element names the section it is in, and a line names what it joins, so both are known
    // before the elements are read: the sections, and then what the lines can join, and where
    // each of those is drawn.
    let frames: HashSet<&str> = items
        .iter()
        .filter(|item| item.kind == Kind::Frame)
        .filter_map(|item| item.id)
        .collect();
    let mut drawn: Vec<Option<Skeleton>> = vec![None; items.len()];
    for (at, item) in items
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.kind.is_linear())
    {
        drawn[at] = item.skeleton(at, &labels, &frames, &HashMap::new());
    }
    let joinable: HashMap<&str, &Fields> = items
        .iter()
        .zip(&drawn)
        .filter(|(_, drawn)| drawn.is_some())
        .filter_map(|(item, _)| Some((item.id?, item.fields)))
        .collect();
    for (at, item) in items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.kind.is_linear())
    {
        drawn[at] = item.skeleton(at, &labels, &frames, &joinable);
    }
    Ok(Imported {
        skeletons: drawn.into_iter().flatten().collect(),
        notes: skipped.notes(),
    })
}

/// Draws the elements of `scene` on `board`'s page on show, at the place the scene has them. The
/// notes say what was left out: what [`to_skeletons`] skipped, and what [`skeleton::apply`]
/// couldn't draw.
///
/// It is one undo step ("Add elements"), unless the engine refuses an element: then the others are
/// drawn without it, one step each, and it is a note ([`draw_apart`]). That is no error, because
/// what is left is worth keeping.
///
/// The ids of the scene's elements are the agent ids of what is drawn ([`skeleton::SAWIT_ID`]), so
/// an agent can still name an element by the id it had in Excalidraw.
///
/// An error when `scene` has no `elements` array, when `board` isn't a board (a design file has
/// no board objects), or when not one element could be drawn.
pub fn import(board: &mut BoardDoc, scene: &Value) -> Result<Vec<String>, BoardError> {
    let Imported {
        skeletons,
        mut notes,
    } = to_skeletons(scene).map_err(BoardError::Format)?;
    match skeleton::apply(board, &skeletons, (0.0, 0.0)) {
        Ok(applied) => notes.extend(applied.notes.iter().map(|note| named(note, &skeletons))),
        // Drawn apart, a design file would take the sections and pictures and refuse the rest.
        Err(error) if board.kind() != DocKind::Board => return Err(error),
        Err(error) => draw_apart(board, &skeletons, error, &mut notes)?,
    }
    Ok(notes)
}

/// A new board called `name` that holds the scene, and the notes on what the import left out
/// ([`import`]).
///
/// The board is as one that was loaded from a file: its history has no step, so undoing never
/// takes away what the person opened. That is what opening an Excalidraw canvas is, and not an
/// edit of the new board. An error as [`import`]'s.
pub fn to_board(name: &str, scene: &Value) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let mut drawn = BoardDoc::new_board(name)?;
    let notes = import(&mut drawn, scene)?;
    let doc = drawn.doc()?;
    drop(drawn);
    let doc = Arc::try_unwrap(doc).unwrap_or_else(|shared| (*shared).clone());
    // As a file is opened: its state made inside a guard against a panic (CANVAS-CORE-9).
    let session = figcraft_engine::guard::run("file.open", || Ok(Session::with_document(doc)))?;
    Ok((BoardDoc { session }, notes))
}
