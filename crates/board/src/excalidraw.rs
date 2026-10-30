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

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use figcraft_color::Color;
use figcraft_engine::Session;
use serde_json::{Map, Value, json};

use crate::doc::{BoardDoc, BoardError, DocKind};
use crate::skeleton::{self, End, Label, Skeleton};

/// A JSON object: an element of a scene, or its `files`.
type Fields = Map<String, Value>;

/// The sides `board.shape` takes (from 1 to 100000). Excalidraw keeps elements outside of that, a
/// rectangle of no width that looks like a line, say, and a side the engine refuses would fail the
/// whole import, so every box is held to it.
const MIN_SIDE: f64 = 1.0;
const MAX_SIDE: f64 = 100_000.0;

/// What some editors write at the start of a text file.
const BYTE_ORDER_MARK: char = '\u{feff}';

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
    // before the elements are read: the sections, and then what the lines can join.
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
        drawn[at] = item.skeleton(at, &labels, &frames, &HashSet::new());
    }
    let joinable: HashSet<&str> = items
        .iter()
        .zip(&drawn)
        .filter(|(_, drawn)| drawn.is_some())
        .filter_map(|(item, _)| item.id)
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
    let board = BoardDoc {
        session: Session::with_document(doc),
    };
    Ok((board, notes))
}

/// The scene in the `.excalidraw` file at `path`. A file that can't be read, or isn't JSON, is an
/// error that names it. A file with nothing in it isn't JSON, so it is one too: a load that
/// somebody asked for (the `replace` op) has no use for it. Opening a canvas takes it as a blank
/// one ([`read_file_or_blank`]).
pub fn read_file(path: &Path) -> Result<Value, BoardError> {
    parse(path, &read_text(path)?)
}

/// The scene in the `.excalidraw` file at `path`, as [`read_file`] reads it, except that a file
/// with nothing in it is a scene with no elements: a blank canvas. Nothing in it is empty, or only
/// white space, or only a byte order mark (which some editors write). Today's canvas opens such a
/// file blank, and there is nothing in it to protect.
///
/// It is for opening a canvas, as the migration of the old project canvas does
/// ([`store::open_with_issues`](crate::store::open_with_issues)). Everything else is as
/// [`read_file`]'s: a file with anything in it that isn't a scene is an error that names it.
pub fn read_file_or_blank(path: &Path) -> Result<Value, BoardError> {
    let text = read_text(path)?;
    if text
        .chars()
        .all(|c| c.is_whitespace() || c == BYTE_ORDER_MARK)
    {
        return Ok(json!({"type": "excalidraw", "elements": []}));
    }
    parse(path, &text)
}

/// The text of the file at `path`.
fn read_text(path: &Path) -> Result<String, BoardError> {
    std::fs::read_to_string(path)
        .map_err(|e| BoardError::Io(format!("couldn't read {}: {e}", path.display())))
}

/// The JSON in `text`, the text of the file at `path`, without the byte order mark in front.
fn parse(path: &Path, text: &str) -> Result<Value, BoardError> {
    serde_json::from_str(text.trim_start_matches(BYTE_ORDER_MARK))
        .map_err(|e| BoardError::Format(format!("{} isn't valid JSON: {e}", path.display())))
}

/// Draws `items` one at a time, in the order [`skeleton::apply`] draws a list in, so that an
/// element the engine refuses is left out with a note and the others stay. `refused` is why the
/// list as a whole wasn't drawn, and is the error when not one element could be drawn.
///
/// An element finds what it joins, or the section it goes in, by the agent id of what was drawn
/// before it, so one at a time draws the same as all at once.
fn draw_apart(
    board: &mut BoardDoc,
    items: &[Skeleton],
    refused: BoardError,
    notes: &mut Vec<String>,
) -> Result<(), BoardError> {
    let mut in_order: Vec<&Skeleton> = items.iter().collect();
    in_order.sort_by_key(|item| stage(&item.kind));
    let mut later = Vec::new();
    let mut drawn = 0;
    for item in in_order {
        let only = std::slice::from_ref(item);
        match skeleton::apply(board, only, (0.0, 0.0)) {
            Ok(applied) => {
                drawn += 1;
                later.extend(applied.notes.iter().map(|note| named(note, only)));
            }
            Err(error) => later.push(format!("skipped {}: {}", describe(item), reason(&error))),
        }
    }
    if drawn == 0 && !items.is_empty() {
        return Err(refused);
    }
    notes.extend(later);
    Ok(())
}

/// When [`skeleton::apply`] draws a skeleton of `kind`: sections, then shapes and pictures, then
/// text, then what joins them.
fn stage(kind: &str) -> usize {
    match kind {
        "frame" => 0,
        "rectangle" | "ellipse" | "diamond" | "image" => 1,
        "text" => 2,
        _ => 3,
    }
}

/// `note`, as [`skeleton::apply`] words it about "elements[3]", with each of those named by the
/// element it is about, which is how a person knows it: "arrow `Ab3x`". `items` are the skeletons
/// it was given.
fn named(note: &str, items: &[Skeleton]) -> String {
    const OPEN: &str = "elements[";
    let mut out = String::with_capacity(note.len());
    let mut rest = note;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        rest = &rest[at + OPEN.len()..];
        let item = rest.split_once(']').and_then(|(index, tail)| {
            let item = items.get(index.parse::<usize>().ok()?)?;
            Some((item, tail))
        });
        match item {
            Some((item, tail)) => {
                out.push_str(&describe(item));
                rest = tail;
            }
            None => out.push_str(OPEN),
        }
    }
    out.push_str(rest);
    out
}

/// What a skeleton is, for a note: its type, and the id it came with.
fn describe(item: &Skeleton) -> String {
    match item.id.as_deref() {
        Some(id) => format!("{} `{id}`", item.kind),
        None => item.kind.clone(),
    }
}

/// Why `apply` refused an element it was given alone: the engine's words, without the
/// "elements[0] (arrow): " in front.
fn reason(error: &BoardError) -> String {
    let message = error.to_string();
    match message.split_once("): ") {
        Some((_, reason)) => reason.to_string(),
        None => message,
    }
}

/// What a scene's element can be that the canvas draws.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rectangle,
    Ellipse,
    Diamond,
    Text,
    Arrow,
    Line,
    Freedraw,
    Frame,
    Image,
}

impl Kind {
    /// The kind that the `type` of an element names.
    fn of(name: &str) -> Option<Self> {
        Some(match name {
            "rectangle" => Self::Rectangle,
            "ellipse" => Self::Ellipse,
            "diamond" => Self::Diamond,
            "text" => Self::Text,
            "arrow" => Self::Arrow,
            "line" => Self::Line,
            "freedraw" => Self::Freedraw,
            "frame" => Self::Frame,
            "image" => Self::Image,
            _ => return None,
        })
    }

    /// The type of the skeleton it becomes. It is the same word.
    fn skeleton(self) -> &'static str {
        match self {
            Self::Rectangle => "rectangle",
            Self::Ellipse => "ellipse",
            Self::Diamond => "diamond",
            Self::Text => "text",
            Self::Arrow => "arrow",
            Self::Line => "line",
            Self::Freedraw => "freedraw",
            Self::Frame => "frame",
            Self::Image => "image",
        }
    }

    /// Whether it is drawn through points rather than as a box.
    fn is_linear(self) -> bool {
        matches!(self, Self::Arrow | Self::Line | Self::Freedraw)
    }

    /// Whether a text can be bound to it, as its label.
    fn holds_a_label(self) -> bool {
        matches!(
            self,
            Self::Rectangle | Self::Ellipse | Self::Diamond | Self::Arrow
        )
    }
}

/// A live element of a scene that the canvas draws.
struct Item<'a> {
    kind: Kind,
    /// The element's id. Other elements name it in `containerId`, `startBinding`, `endBinding` and
    /// `frameId`.
    id: Option<&'a str>,
    fields: &'a Fields,
    /// What an image shows.
    picture: Option<Picture<'a>>,
}

impl<'a> Item<'a> {
    /// The element in `element`, or `None` when it isn't drawn: it is deleted, an embed, of a type
    /// the canvas doesn't know, or an image with no picture. What isn't a deleted element is
    /// counted in `skipped`.
    fn read(element: &'a Value, files: Option<&'a Fields>, skipped: &mut Skipped) -> Option<Self> {
        let Some(fields) = element.as_object() else {
            skipped.not_objects += 1;
            return None;
        };
        if flag(fields, "isDeleted") == Some(true) {
            return None;
        }
        let kind = match string(fields, "type") {
            Some("embeddable" | "iframe" | "magicframe") => {
                skipped.embeds += 1;
                return None;
            }
            Some(name) => {
                let Some(kind) = Kind::of(name) else {
                    skipped.unknown(name);
                    return None;
                };
                kind
            }
            None => {
                skipped.untyped += 1;
                return None;
            }
        };
        let mut picture = None;
        if kind == Kind::Image {
            picture = Picture::of(fields, files);
            if picture.is_none() {
                skipped.pictures += 1;
                return None;
            }
        }
        Some(Self {
            kind,
            id: string(fields, "id").filter(|id| !id.is_empty()),
            fields,
            picture,
        })
    }

    /// The skeleton of the element at index `at` of the items, or `None` when there is nothing to
    /// draw: a text that is another element's label, or has no characters.
    ///
    /// Of what is named in the element, `frames` are the sections that are drawn, and `joinable`
    /// the elements that a line can join. A name that isn't one of those is left out, so that it
    /// can't be taken for something else on the board: a numeric id is a node id too.
    fn skeleton(
        &self,
        at: usize,
        labels: &Labels,
        frames: &HashSet<&str>,
        joinable: &HashSet<&str>,
    ) -> Option<Skeleton> {
        let fields = self.fields;
        let mut item = Skeleton {
            kind: self.kind.skeleton().to_string(),
            id: self.id.map(str::to_string),
            x: number(fields, "x").unwrap_or(0.0),
            y: number(fields, "y").unwrap_or(0.0),
            frame_id: string(fields, "frameId")
                .filter(|frame| frames.contains(frame))
                .map(str::to_string),
            ..Skeleton::default()
        };
        match self.kind {
            Kind::Rectangle | Kind::Ellipse | Kind::Diamond => {
                item.width = side(fields, "width");
                item.height = side(fields, "height");
                item.label = self.label(labels);
                if self.kind == Kind::Rectangle {
                    item.roundness = fields.get("roundness").filter(|r| !r.is_null()).cloned();
                }
                item.stroke_color = colour(fields, "strokeColor");
                item.background_color = colour(fields, "backgroundColor");
            }
            Kind::Text => {
                if labels.used.contains(&at) {
                    return None;
                }
                item.text = Some(text_of(fields)?.to_string());
                // A text that grows with what is typed is as wide as it is long. Excalidraw's
                // measure of it isn't this canvas's, so its width would wrap it early.
                if flag(fields, "autoResize") == Some(false) {
                    item.width = number(fields, "width").filter(|width| *width > 0.0);
                }
                // A text's colour is its stroke in Excalidraw. Its background is never drawn.
                item.stroke_color = colour(fields, "strokeColor");
            }
            Kind::Frame => {
                // Excalidraw frames don't nest, and draw in a style of their own: the colours
                // saved with one aren't its look.
                item.frame_id = None;
                item.width = side(fields, "width");
                item.height = side(fields, "height");
                item.name = string(fields, "name")
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string);
            }
            Kind::Image => {
                let picture = self.picture.as_ref()?;
                item.width = side(fields, "width");
                item.height = side(fields, "height");
                item.data = Some(picture.data.to_string());
                item.name = Some(picture.name.clone());
            }
            Kind::Arrow | Kind::Line | Kind::Freedraw => {
                item.width = number(fields, "width");
                item.height = number(fields, "height");
                item.points = points_of(fields);
                if self.kind != Kind::Freedraw {
                    self.run_between(&mut item, labels, joinable);
                }
                item.stroke_color = colour(fields, "strokeColor");
            }
        }
        Some(item)
    }

    /// Sets what an arrow or a line runs between on `item`, its skeleton, which has its place and
    /// its points: the two ends and the label. An arrow is then drawn as its heads say ([`Heads`]).
    fn run_between(&self, item: &mut Skeleton, labels: &Labels, joinable: &HashSet<&str>) {
        let origin = (item.x, item.y);
        let points = item.points.as_deref().unwrap_or_default();
        let start = end_of(
            self.fields,
            "startBinding",
            points.first(),
            origin,
            joinable,
        );
        let end = end_of(self.fields, "endBinding", points.last(), origin, joinable);
        item.start = Some(start);
        item.end = Some(end);
        item.label = self.label(labels);
        if self.kind == Kind::Arrow {
            match Heads::of(self.fields) {
                Heads::End => {}
                Heads::Start => turn_around(item),
                Heads::Neither => without_heads(item),
            }
        }
    }

    /// The text bound to this element.
    fn label(&self, labels: &Labels) -> Option<Label> {
        let text = labels.of.get(self.id?)?;
        Some(Label {
            text: (*text).to_string(),
        })
    }
}

/// The texts that are bound to another element as its label.
struct Labels<'a> {
    /// The text of each element that has one, by the element's id.
    of: HashMap<&'a str, &'a str>,
    /// The indexes of the items that are such texts, and so aren't drawn as texts.
    used: HashSet<usize>,
}

impl<'a> Labels<'a> {
    /// The labels among `items`: a text that names, in `containerId`, an item that can hold one. A
    /// text with no characters has no label to give, and when two texts name one element the
    /// first is its label and the other stays a text.
    fn find(items: &[Item<'a>]) -> Self {
        let holders: HashSet<&str> = items
            .iter()
            .filter(|item| item.kind.holds_a_label())
            .filter_map(|item| item.id)
            .collect();
        let mut labels = Self {
            of: HashMap::new(),
            used: HashSet::new(),
        };
        for (at, item) in items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.kind == Kind::Text)
        {
            let holder = string(item.fields, "containerId").filter(|id| holders.contains(id));
            let (Some(holder), Some(text)) = (holder, text_of(item.fields)) else {
                continue;
            };
            if let Entry::Vacant(slot) = labels.of.entry(holder) {
                slot.insert(text);
                labels.used.insert(at);
            }
        }
        labels
    }
}

/// The picture an image shows: the base64 of its file, and a file name whose extension tells the
/// importer what format it is in.
struct Picture<'a> {
    data: &'a str,
    name: String,
}

impl<'a> Picture<'a> {
    /// The picture of the image `element`: in the entry of `files` that its `fileId` names, a
    /// base64 data URL. `None` when there is no such entry, or its data isn't base64.
    fn of(element: &Fields, files: Option<&'a Fields>) -> Option<Self> {
        let file = files?.get(string(element, "fileId")?)?.as_object()?;
        let (head, data) = string(file, "dataURL")?
            .strip_prefix("data:")?
            .split_once(',')?;
        let mut parts = head.split(';');
        let media = parts.next().unwrap_or_default();
        let data = data.trim();
        if data.is_empty() || !parts.any(|part| part.eq_ignore_ascii_case("base64")) {
            return None;
        }
        let media = string(file, "mimeType").unwrap_or(media);
        Some(Self {
            data,
            name: format!("Image.{}", extension(media)),
        })
    }
}

/// The extension of a file of media type `media`. The importer reads the bytes of anything but an
/// SVG, which it takes by its name, so a type that isn't known is a `.png`.
fn extension(media: &str) -> &'static str {
    match media.trim().to_ascii_lowercase().as_str() {
        "image/svg+xml" => "svg",
        "image/jpeg" | "image/jpg" | "image/pjpeg" | "image/jfif" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        _ => "png",
    }
}

/// What a scene holds that the canvas can't draw, counted.
#[derive(Default)]
struct Skipped {
    /// Entries of `elements` that aren't objects.
    not_objects: usize,
    /// Elements with no type.
    untyped: usize,
    /// Elements of a type the canvas doesn't know, by type, in the order they were met.
    unknown: Vec<(String, usize)>,
    embeds: usize,
    /// Images whose picture isn't in the scene.
    pictures: usize,
}

impl Skipped {
    fn unknown(&mut self, name: &str) {
        match self.unknown.iter_mut().find(|(known, _)| known == name) {
            Some((_, count)) => *count += 1,
            None => self.unknown.push((name.to_string(), 1)),
        }
    }

    /// One sentence for each kind of thing that was skipped.
    fn notes(self) -> Vec<String> {
        let mut notes = Vec::new();
        match self.not_objects {
            0 => {}
            1 => notes.push("skipped 1 entry of `elements` that is not an object".into()),
            n => notes.push(format!(
                "skipped {n} entries of `elements` that are not objects"
            )),
        }
        if self.untyped > 0 {
            let what = count(self.untyped, "element", "elements");
            notes.push(format!("skipped {what} without a type"));
        }
        for (name, n) in self.unknown {
            let what = count(n, "element", "elements");
            notes.push(format!(
                "skipped {what} of the unknown type `{}`",
                shown(&name)
            ));
        }
        if self.embeds > 0 {
            let what = count(self.embeds, "embed", "embeds");
            notes.push(format!("skipped {what}"));
        }
        if self.pictures > 0 {
            let what = count(self.pictures, "image", "images");
            notes.push(format!("skipped {what} with no picture in the scene"));
        }
        notes
    }
}

/// "1 embed", "2 embeds".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A type from a scene as far as a note shows it: the file chose it, so it can be anything.
fn shown(name: &str) -> String {
    const LONGEST: usize = 40;
    match name.char_indices().nth(LONGEST) {
        Some((end, _)) => format!("{}…", &name[..end]),
        None => name.to_string(),
    }
}

fn string<'a>(fields: &'a Fields, key: &str) -> Option<&'a str> {
    fields.get(key)?.as_str()
}

fn flag(fields: &Fields, key: &str) -> Option<bool> {
    fields.get(key)?.as_bool()
}

/// A number that can be drawn: JSON has no other.
fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}

fn number(fields: &Fields, key: &str) -> Option<f64> {
    finite(fields.get(key)?)
}

/// A width or a height, held to the sides the engine takes ([`MIN_SIDE`]).
fn side(fields: &Fields, key: &str) -> Option<f64> {
    number(fields, key).map(|side| side.clamp(MIN_SIDE, MAX_SIDE))
}

/// The colour in `key`, if the engine can read it: a CSS colour (hex, `rgb()`, a name), or
/// transparent. One it can't is left out, and the element keeps the colour it would have had.
fn colour(fields: &Fields, key: &str) -> Option<String> {
    let colour = string(fields, key)?.trim();
    let readable = Color::parse_css(colour).is_some() || colour.eq_ignore_ascii_case("none");
    readable.then(|| colour.to_string())
}

/// What a text says: the text as it was typed (`originalText`, which Excalidraw has already
/// wrapped to a width in `text`), else `text`. `None` for no characters.
fn text_of(fields: &Fields) -> Option<&str> {
    ["originalText", "text"]
        .into_iter()
        .filter_map(|key| string(fields, key))
        .find(|text| !text.is_empty())
}

/// The `points` of a line or a freedraw, relative to its `x`, `y`. A point that isn't a pair of
/// numbers is dropped. `None` for no points.
fn points_of(fields: &Fields) -> Option<Vec<[f64; 2]>> {
    let points: Vec<[f64; 2]> = fields
        .get("points")?
        .as_array()?
        .iter()
        .filter_map(|point| {
            let pair = point.as_array()?;
            Some([finite(pair.first()?)?, finite(pair.get(1)?)?])
        })
        .collect();
    (!points.is_empty()).then_some(points)
}

/// One end of an arrow or a line: the element named in its `binding` when `joinable` has it, else
/// the end is free, at `point` (relative to `origin`, the line's own place).
fn end_of(
    fields: &Fields,
    binding: &str,
    point: Option<&[f64; 2]>,
    origin: (f64, f64),
    joinable: &HashSet<&str>,
) -> End {
    let bound = fields
        .get(binding)
        .and_then(|binding| binding.get("elementId"))
        .and_then(Value::as_str);
    if let Some(id) = bound.filter(|id| joinable.contains(id)) {
        return End {
            id: Some(id.to_string()),
            ..End::default()
        };
    }
    End {
        id: None,
        x: point.map(|p| origin.0 + p[0]),
        y: point.map(|p| origin.1 + p[1]),
    }
}

/// Where an arrow has a head, which is which way it points. A skeleton's `arrow` has one, at its
/// end, and its `line` has none; so the kind of head, and a second head at the start, are not
/// carried.
enum Heads {
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
    fn of(fields: &Fields) -> Self {
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
fn turn_around(item: &mut Skeleton) {
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
fn without_heads(item: &mut Skeleton) {
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

#[cfg(test)]
mod tests {
    use figcraft_doc::{BoardItem, End as DocEnd, NodeId, NodeKind, StrokeCap};
    use serde_json::json;

    use super::*;
    use crate::ops;

    /// The scene of the `basic.excalidraw` fixture.
    fn basic() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/excalidraw/basic.excalidraw"
        ))
        .unwrap()
    }

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// What `get` lists on the board.
    fn elements(board: &mut BoardDoc) -> Vec<Value> {
        let reply = ops::run(board, &json!({"op": "get"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        reply["elements"].as_array().unwrap().clone()
    }

    /// The elements of `kind` among `elements`.
    fn of_type<'a>(elements: &'a [Value], kind: &str) -> Vec<&'a Value> {
        let is = |e: &&Value| e["type"] == json!(kind);
        elements.iter().filter(is).collect()
    }

    #[test]
    fn basic_scene_imports_everything_supported() {
        let mut board = board();
        let notes = import(&mut board, &basic()).unwrap();
        assert_eq!(notes, ["skipped 1 embed"]);

        let elements = elements(&mut board);
        // One of each kind the fixture holds, and nothing else: no layer for the label, the
        // embed or the deleted rectangle, and the frame's child is inside it.
        assert_eq!(elements.len(), 9, "{elements:?}");
        assert_eq!(of_type(&elements, "rectangle").len(), 1);
        assert_eq!(of_type(&elements, "ellipse").len(), 1);
        assert_eq!(of_type(&elements, "diamond").len(), 1);
        assert_eq!(of_type(&elements, "connector").len(), 2);
        assert_eq!(of_type(&elements, "ink").len(), 1);
        assert_eq!(of_type(&elements, "text").len(), 1);
        assert_eq!(of_type(&elements, "section").len(), 1);
        assert_eq!(of_type(&elements, "image").len(), 1);

        // The rectangle carries its label.
        let rectangle = of_type(&elements, "rectangle")[0];
        assert_eq!(rectangle["text"], json!("Service"));
        let ellipse = of_type(&elements, "ellipse")[0];

        // One connector is bound at both ends, to the rectangle and the ellipse; the other is a
        // free line, with no caps.
        let doc = board.doc().unwrap();
        let (bound, free): (Vec<&Value>, Vec<&Value>) = of_type(&elements, "connector")
            .into_iter()
            .partition(|c| c.get("from").is_some());
        assert_eq!((bound.len(), free.len()), (1, 1));
        assert_eq!(bound[0]["from"], rectangle["id"]);
        assert_eq!(bound[0]["to"], ellipse["id"]);
        assert!(free[0].get("to").is_none());
        let line = doc
            .node(free[0]["id"].as_str().unwrap().parse().unwrap())
            .unwrap();
        assert_eq!(line.stroke.cap_start, StrokeCap::None);
        assert_eq!(line.stroke.cap_end, StrokeCap::None);
        assert!(matches!(line.board, Some(BoardItem::Connector(_))));

        // The section is named for the frame, and holds the rectangle that was in it.
        let section = of_type(&elements, "section")[0];
        assert_eq!(section["name"], json!("Group A"));
        let section = doc
            .node(section["id"].as_str().unwrap().parse().unwrap())
            .unwrap();
        assert!(matches!(section.kind, NodeKind::Section { .. }));
        assert_eq!(section.children.len(), 1);
        let child = doc.node(section.children[0]).unwrap();
        assert!(matches!(child.board, Some(BoardItem::Shape { .. })));
    }

    #[test]
    fn bad_scene_is_reported() {
        for scene in [
            json!({}),
            json!([]),
            json!(null),
            json!("scene"),
            json!({"elements": {}}),
            json!({"elements": "none"}),
            json!({"elements": null}),
        ] {
            let error = to_skeletons(&scene).err().unwrap_or_default();
            assert!(error.contains("`elements`"), "{scene}: {error:?}");
            // Importing it is an error too, and draws nothing.
            let mut board = board();
            let before = board.doc().unwrap();
            assert!(matches!(
                import(&mut board, &scene),
                Err(BoardError::Format(_))
            ));
            assert!(std::sync::Arc::ptr_eq(&before, &board.doc().unwrap()));
        }
    }

    #[test]
    fn unknown_types_are_noted() {
        let scene = json!({"elements": [
            {"id": "a", "type": "laser", "x": 0, "y": 0},
            {"id": "b", "type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50},
        ]});
        let imported = to_skeletons(&scene).unwrap();
        assert_eq!(imported.skeletons.len(), 1);
        assert_eq!(imported.skeletons[0].kind, "rectangle");
        assert_eq!(imported.notes.len(), 1, "{:?}", imported.notes);
        assert!(imported.notes[0].contains("laser"), "{:?}", imported.notes);

        // It is a note when imported, never an error, and the rest is drawn.
        let mut board = board();
        let notes = import(&mut board, &scene).unwrap();
        assert_eq!(notes, imported.notes);
        assert_eq!(elements(&mut board).len(), 1);
    }

    // What the brief doesn't name.

    /// A scene of `elements`, with the other keys a saved one has.
    fn scene(elements: Value) -> Value {
        json!({"type": "excalidraw", "version": 2, "elements": elements, "files": {}})
    }

    /// The skeletons of a scene of `elements`, and the notes it gave.
    fn read(elements: Value) -> Imported {
        to_skeletons(&scene(elements)).unwrap()
    }

    /// The skeleton with agent id `id`.
    fn with_id<'a>(imported: &'a Imported, id: &str) -> &'a Skeleton {
        let found = imported
            .skeletons
            .iter()
            .find(|s| s.id.as_deref() == Some(id));
        found.unwrap_or_else(|| panic!("no skeleton `{id}` in {:?}", imported.skeletons))
    }

    /// A rectangle, as little of it as the importer reads.
    fn rectangle(id: &str, x: f64, y: f64) -> Value {
        json!({"id": id, "type": "rectangle", "x": x, "y": y, "width": 100, "height": 50})
    }

    /// A text bound to the element `container`.
    fn label(id: &str, container: &str, text: &str) -> Value {
        json!({"id": id, "type": "text", "x": 0, "y": 0, "text": text, "originalText": text,
               "containerId": container})
    }

    /// The first solid fill and stroke of layer `id`, as `#RRGGBB`.
    fn paints(board: &BoardDoc, id: &str) -> (Option<String>, Option<String>) {
        let doc = board.doc().unwrap();
        let node = doc.node(id.parse().unwrap()).unwrap();
        let first = |paints: &[figcraft_color::Paint]| {
            paints
                .iter()
                .find_map(figcraft_color::Paint::solid_color)
                .map(figcraft_color::Color::to_css_hex)
        };
        (first(&node.fills), first(&node.strokes))
    }

    #[test]
    fn deleted_elements_are_skipped_without_a_word() {
        let mut deleted = rectangle("gone", 0.0, 0.0);
        deleted["isDeleted"] = json!(true);
        let mut deleted_text = label("gone-label", "gone", "bye");
        deleted_text["isDeleted"] = json!(true);
        let imported = read(json!([deleted, deleted_text, rectangle("kept", 0.0, 0.0)]));
        assert_eq!(imported.notes, Vec::<String>::new());
        assert_eq!(imported.skeletons.len(), 1);
        assert_eq!(imported.skeletons[0].id.as_deref(), Some("kept"));
    }

    #[test]
    fn a_bound_text_is_its_containers_label() {
        let mut wrapped = label("t1", "box", "wrapped\nby\nexcalidraw");
        // What was typed is `originalText`; `text` has been wrapped to the container's width.
        wrapped["originalText"] = json!("wrapped by excalidraw");
        let imported = read(json!([
            rectangle("box", 0.0, 0.0),
            wrapped,
            {"id": "oval", "type": "ellipse", "x": 200, "y": 0, "width": 80, "height": 80},
            label("t2", "oval", "Oval"),
            {"id": "diamond", "type": "diamond", "x": 400, "y": 0, "width": 80, "height": 80},
            label("t3", "diamond", "Diamond"),
            {"id": "arrow", "type": "arrow", "x": 0, "y": 200, "points": [[0, 0], [100, 0]]},
            label("t4", "arrow", "calls"),
        ]));
        assert_eq!(imported.notes, Vec::<String>::new());
        // The labels are not elements of their own.
        let ids: Vec<&str> = imported
            .skeletons
            .iter()
            .filter_map(|s| s.id.as_deref())
            .collect();
        assert_eq!(ids, ["box", "oval", "diamond", "arrow"]);
        let text = |id: &str| {
            with_id(&imported, id)
                .label
                .as_ref()
                .map(|l| l.text.clone())
        };
        assert_eq!(text("box").as_deref(), Some("wrapped by excalidraw"));
        assert_eq!(text("oval").as_deref(), Some("Oval"));
        assert_eq!(text("diamond").as_deref(), Some("Diamond"));
        assert_eq!(text("arrow").as_deref(), Some("calls"));
    }

    #[test]
    fn a_text_that_cannot_be_a_label_stays_a_text() {
        let line =
            json!({"id": "line", "type": "line", "x": 0, "y": 0, "points": [[0, 0], [9, 9]]});
        let mut gone = rectangle("gone", 0.0, 0.0);
        gone["isDeleted"] = json!(true);
        let imported = read(json!([
            rectangle("box", 0.0, 0.0),
            label("first", "box", "first"),
            // A second text for the same container stays a text of its own.
            label("second", "box", "second"),
            // A container that isn't in the scene, is deleted, or is a line, can't hold one.
            label("orphan", "nowhere", "orphan"),
            gone,
            label("ghost", "gone", "ghost"),
            line,
            label("on-line", "line", "on a line"),
            // No characters: nothing to draw.
            label("empty", "box", ""),
        ]));
        let texts: Vec<(&str, &str)> = imported
            .skeletons
            .iter()
            .filter(|s| s.kind == "text")
            .map(|s| (s.id.as_deref().unwrap(), s.text.as_deref().unwrap()))
            .collect();
        assert_eq!(
            texts,
            [
                ("second", "second"),
                ("orphan", "orphan"),
                ("ghost", "ghost"),
                ("on-line", "on a line"),
            ]
        );
        let first = with_id(&imported, "box").label.as_ref().unwrap();
        assert_eq!(first.text, "first");
    }

    #[test]
    fn arrows_join_what_the_scene_draws_and_are_free_otherwise() {
        let bound = |id: &str| json!({"elementId": id, "focus": 0, "gap": 1});
        let mut deleted = rectangle("gone", 400.0, 0.0);
        deleted["isDeleted"] = json!(true);
        let imported = read(json!([
            rectangle("a", 0.0, 0.0),
            deleted,
            {"id": "embed", "type": "embeddable", "x": 600, "y": 0, "width": 50, "height": 50},
            // Bound at the start to a drawn element, at the end to an embed, which isn't drawn.
            {"id": "one", "type": "arrow", "x": 100, "y": 25, "points": [[0, 0], [300, 40]],
             "startBinding": bound("a"), "endBinding": bound("embed")},
            // Both ends name what isn't there, or was deleted; one is a number, which is a node id
            // on the board too.
            {"id": "two", "type": "arrow", "x": 10, "y": 20, "points": [[0, 0], [70, 80]],
             "startBinding": bound("gone"), "endBinding": bound("3")},
            // Not bound at all, with the points relative to the arrow's place.
            {"id": "three", "type": "line", "x": 5, "y": 6, "points": [[0, 0], [20, 30]]},
        ]));
        assert_eq!(imported.notes, ["skipped 1 embed"]);
        let end = |id: &str, which: &str| {
            let item = with_id(&imported, id);
            let end = if which == "start" {
                &item.start
            } else {
                &item.end
            };
            end.clone().unwrap()
        };
        let free = |x: f64, y: f64| End {
            id: None,
            x: Some(x),
            y: Some(y),
        };
        // A joined end names the element and nothing else.
        assert_eq!(
            end("one", "start"),
            End {
                id: Some("a".into()),
                x: None,
                y: None
            }
        );
        assert_eq!(end("one", "end"), free(400.0, 65.0));
        assert_eq!(end("two", "start"), free(10.0, 20.0));
        assert_eq!(end("two", "end"), free(80.0, 100.0));
        assert_eq!(end("three", "start"), free(5.0, 6.0));
        assert_eq!(end("three", "end"), free(25.0, 36.0));
        // The points go through as they are, relative to `x`, `y`.
        assert_eq!(
            with_id(&imported, "one").points,
            Some(vec![[0.0, 0.0], [300.0, 40.0]])
        );
        assert_eq!(with_id(&imported, "one").x, 100.0);
    }

    // Arrowheads: where an arrow's heads are is which way it points, and an arrow with none is a
    // line. The scenes here are written inline, so that the fixture's counts stay what they are.

    /// An arrow from `at` along `points`, as Excalidraw writes it: `heads` are its
    /// `startArrowhead` and `endArrowhead` (`null` is none), and `joins` are the elements it is
    /// bound to at its start and at its end.
    fn arrow(
        id: &str,
        at: (f64, f64),
        points: Value,
        heads: (Value, Value),
        joins: (Option<&str>, Option<&str>),
    ) -> Value {
        let bound = |to: Option<&str>| match to {
            Some(id) => json!({"elementId": id, "focus": 0, "gap": 1}),
            None => json!(null),
        };
        json!({"id": id, "type": "arrow", "x": at.0, "y": at.1, "points": points,
               "startArrowhead": heads.0, "endArrowhead": heads.1,
               "startBinding": bound(joins.0), "endBinding": bound(joins.1)})
    }

    /// The rectangles `a` and `b`, side by side, and `link` after them.
    fn between(link: Value) -> Value {
        json!([rectangle("a", 0.0, 0.0), rectangle("b", 300.0, 0.0), link])
    }

    /// The elements of a scene drawn on a new board, with nothing left out.
    fn drawn(elements: Value) -> BoardDoc {
        let mut board = board();
        let notes = import(&mut board, &scene(elements)).unwrap();
        assert_eq!(notes, Vec::<String>::new());
        board
    }

    /// The node on `board` that the element `id` of the scene is.
    fn node_of(board: &BoardDoc, id: &str) -> Option<NodeId> {
        skeleton::resolve(&board.doc().unwrap(), id)
    }

    /// The connector that the element `id` of the scene is on `board`: its two ends, and the caps
    /// of its line (its arrowheads).
    fn connector(board: &BoardDoc, id: &str) -> (DocEnd, DocEnd, StrokeCap, StrokeCap) {
        let doc = board.doc().unwrap();
        let node = doc.node(skeleton::resolve(&doc, id).unwrap()).unwrap();
        let Some(BoardItem::Connector(connector)) = node.board.clone() else {
            panic!("`{id}` is {:?}, not a connector", node.board);
        };
        (
            connector.from,
            connector.to,
            node.stroke.cap_start,
            node.stroke.cap_end,
        )
    }

    /// An end of a skeleton that joins the element `id`.
    fn joined(id: &str) -> End {
        End {
            id: Some(id.into()),
            x: None,
            y: None,
        }
    }

    /// A free end of a skeleton, at the point `(x, y)` of the page.
    fn loose(x: f64, y: f64) -> End {
        End {
            id: None,
            x: Some(x),
            y: Some(y),
        }
    }

    #[test]
    fn an_arrow_with_no_head_is_a_line() {
        // A link with no arrowhead (Mermaid's `A --- B`, say) is written as an arrow whose heads
        // are both `null`. It is drawn as a plain line, so it comes in as one that joins what it
        // joined, and not as an arrow with a head at its end.
        let heads = (json!(null), json!(null));
        let link = arrow(
            "link",
            (100.0, 25.0),
            json!([[0, 0], [200, 0]]),
            heads,
            (Some("a"), Some("b")),
        );
        let imported = read(between(link.clone()));
        assert_eq!(imported.notes, Vec::<String>::new());
        let item = with_id(&imported, "link");
        assert_eq!(item.kind, "line");
        assert_eq!(item.start, Some(joined("a")));
        assert_eq!(item.end, Some(joined("b")));
        assert_eq!(item.points, Some(vec![[0.0, 0.0], [200.0, 0.0]]));

        let board = drawn(between(link));
        let (from, to, cap_start, cap_end) = connector(&board, "link");
        assert_eq!(from.node(), node_of(&board, "a"));
        assert_eq!(to.node(), node_of(&board, "b"));
        assert_eq!((cap_start, cap_end), (StrokeCap::None, StrokeCap::None));
    }

    #[test]
    fn an_arrow_with_a_head_at_its_start_only_is_turned_around() {
        // The head is where an arrow points, and a connector has one head, at its end. So the
        // arrow is drawn from its end to its start: the same line, with the head where it was.
        let heads = (json!("arrow"), json!(null));
        let link = arrow(
            "link",
            (100.0, 25.0),
            json!([[0, 0], [200, 0]]),
            heads,
            (Some("a"), Some("b")),
        );
        let imported = read(between(link.clone()));
        let item = with_id(&imported, "link");
        assert_eq!(item.kind, "arrow");
        // It starts where it ended, and what it joined at its start is what it points at now.
        assert_eq!(item.start, Some(joined("b")));
        assert_eq!(item.end, Some(joined("a")));
        assert_eq!((item.x, item.y), (300.0, 25.0));
        assert_eq!(item.points, Some(vec![[0.0, 0.0], [-200.0, 0.0]]));

        let board = drawn(between(link));
        let (from, to, cap_start, cap_end) = connector(&board, "link");
        assert_eq!(from.node(), node_of(&board, "b"));
        assert_eq!(to.node(), node_of(&board, "a"));
        assert_eq!(
            (cap_start, cap_end),
            (StrokeCap::None, StrokeCap::TriangleArrow)
        );
    }

    #[test]
    fn a_turned_around_arrow_keeps_its_route_its_free_ends_and_its_label() {
        // Free at both ends, through a bend, coloured and labelled: all of it runs the other way
        // round. The first point is still the arrow's own place (`x`, `y`), so the points are
        // given relative to the place where the arrow now starts.
        let mut bent = arrow(
            "bent",
            (10.0, 20.0),
            json!([[0, 0], [60, -30], [200, 40]]),
            (json!("triangle"), json!(null)),
            (None, None),
        );
        bent["strokeColor"] = json!("#ff0000");
        let imported = read(json!([bent.clone(), label("bent-label", "bent", "back")]));
        assert_eq!(imported.notes, Vec::<String>::new());
        let item = with_id(&imported, "bent");
        assert_eq!(item.kind, "arrow");
        assert_eq!((item.x, item.y), (210.0, 60.0));
        assert_eq!(
            item.points,
            Some(vec![[0.0, 0.0], [-140.0, -70.0], [-200.0, -40.0]])
        );
        assert_eq!(item.start, Some(loose(210.0, 60.0)));
        assert_eq!(item.end, Some(loose(10.0, 20.0)));
        assert_eq!(item.stroke_color.as_deref(), Some("#ff0000"));
        assert_eq!(item.label.as_ref().map(|l| l.text.as_str()), Some("back"));

        // On the board it runs from where it ended to where it started, with its head there.
        let board = drawn(json!([bent, label("bent-label", "bent", "back")]));
        let (from, to, cap_start, cap_end) = connector(&board, "bent");
        assert_eq!(from, DocEnd::Point { x: 210.0, y: 60.0 });
        assert_eq!(to, DocEnd::Point { x: 10.0, y: 20.0 });
        assert_eq!(
            (cap_start, cap_end),
            (StrokeCap::None, StrokeCap::TriangleArrow)
        );
    }

    #[test]
    fn an_arrow_with_a_head_at_its_end_or_at_both_ends_is_as_it_was() {
        // Whatever the kind of head is. A connector has one head, at its end, so a second one at
        // the start isn't carried, and the arrow keeps pointing the way it was drawn.
        for (id, start, end) in [
            ("arrow", json!(null), json!("arrow")),
            ("triangle", json!(null), json!("triangle_outline")),
            ("crow", json!(null), json!("crowfoot_many")),
            ("both", json!("arrow"), json!("arrow")),
            ("kinds", json!("dot"), json!("bar")),
        ] {
            let link = arrow(
                id,
                (100.0, 25.0),
                json!([[0, 0], [200, 0]]),
                (start, end),
                (Some("a"), Some("b")),
            );
            let imported = read(between(link.clone()));
            let item = with_id(&imported, id);
            assert_eq!(item.kind, "arrow", "{id}");
            assert_eq!(item.start, Some(joined("a")), "{id}");
            assert_eq!(item.end, Some(joined("b")), "{id}");
            assert_eq!((item.x, item.y), (100.0, 25.0), "{id}");
            assert_eq!(item.points, Some(vec![[0.0, 0.0], [200.0, 0.0]]), "{id}");

            let board = drawn(between(link));
            let (from, to, cap_start, cap_end) = connector(&board, id);
            assert_eq!(from.node(), node_of(&board, "a"), "{id}");
            assert_eq!(to.node(), node_of(&board, "b"), "{id}");
            assert_eq!(
                (cap_start, cap_end),
                (StrokeCap::None, StrokeCap::TriangleArrow),
                "{id}"
            );
        }
    }

    #[test]
    fn a_head_the_scene_does_not_mention_is_read_as_excalidraw_reads_it() {
        // Excalidraw draws an arrow with no `endArrowhead` with a head at its end (what
        // `restoreElements` does, which the canvas runs on every scene it loads), and one with no
        // `startArrowhead` without. A line has no heads, whatever it says of them.
        let plain = |id: &str| json!({"id": id, "type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [50, 0]]});
        let mut start_only = plain("start-only");
        start_only["startArrowhead"] = json!("arrow");
        let mut no_end = plain("no-end");
        no_end["endArrowhead"] = json!(null);
        let line = json!({"id": "line", "type": "line", "x": 0, "y": 0, "points": [[0, 0], [50, 0]],
                          "startArrowhead": "arrow", "endArrowhead": null});
        let imported = read(json!([plain("plain"), start_only, no_end, line]));
        let kind = |id: &str| with_id(&imported, id).kind.as_str();
        assert_eq!(kind("plain"), "arrow");
        // Both heads (the start's is written, the end's is the default): not turned around.
        assert_eq!(kind("start-only"), "arrow");
        assert_eq!(
            with_id(&imported, "start-only").points,
            with_id(&imported, "plain").points
        );
        assert_eq!(kind("no-end"), "line");
        // A line stays a line as it is, whatever heads it names, and is never turned around.
        let line = with_id(&imported, "line");
        assert_eq!(line.kind, "line");
        assert_eq!((line.x, line.y), (0.0, 0.0));
        assert_eq!(line.points, Some(vec![[0.0, 0.0], [50.0, 0.0]]));
    }

    #[test]
    fn a_headless_arrow_that_bends_and_joins_something_is_a_connector() {
        // The Mermaid links on an old canvas are written with the 10 to 19 points that were
        // sampled along their route. A line through more than two points is ink, which joins
        // nothing, so a headless arrow keeps its first and its last point: it runs between what
        // it joined, like an arrow does.
        let route = json!([[0, 0], [60, -30], [140, -30], [200, 0]]);
        let heads = (json!(null), json!(null));
        for (id, joins) in [("both", (Some("a"), Some("b"))), ("end", (None, Some("b")))] {
            let link = arrow(id, (100.0, 25.0), route.clone(), heads.clone(), joins);
            let imported = read(between(link.clone()));
            let item = with_id(&imported, id);
            assert_eq!(item.kind, "line", "{id}");
            assert_eq!(item.points, Some(vec![[0.0, 0.0], [200.0, 0.0]]), "{id}");

            let mut board = drawn(between(link));
            let (from, to, cap_start, cap_end) = connector(&board, id);
            match joins.0 {
                Some(start) => assert_eq!(from.node(), node_of(&board, start), "{id}"),
                None => assert_eq!(from, DocEnd::Point { x: 100.0, y: 25.0 }, "{id}"),
            }
            assert_eq!(to.node(), node_of(&board, "b"), "{id}");
            assert_eq!((cap_start, cap_end), (StrokeCap::None, StrokeCap::None));
            // Not ink: the board has the two rectangles and the one connector.
            let listed = elements(&mut board);
            assert_eq!(listed.len(), 3, "{id}: {listed:?}");
            assert_eq!(of_type(&listed, "connector").len(), 1, "{id}");
        }
    }

    #[test]
    fn a_headless_arrow_that_joins_nothing_keeps_its_bends_unless_it_has_a_label() {
        // Free at both ends, it is as an Excalidraw line is: a line through more than two points
        // is ink, and one through two is a connector with no heads.
        let heads = (json!(null), json!(null));
        let free =
            |id: &str, points: Value| arrow(id, (10.0, 10.0), points, heads.clone(), (None, None));
        let bent = free("bent", json!([[0, 0], [30, 5], [60, 0]]));
        let straight = free("straight", json!([[0, 0], [60, 0]]));
        let imported = read(json!([bent.clone(), straight.clone()]));
        assert_eq!(with_id(&imported, "bent").kind, "line");
        assert_eq!(
            with_id(&imported, "bent").points.as_ref().map(Vec::len),
            Some(3)
        );
        let mut board = drawn(json!([bent.clone(), straight]));
        let listed = elements(&mut board);
        assert_eq!(of_type(&listed, "ink").len(), 1, "{listed:?}");
        assert_eq!(of_type(&listed, "connector").len(), 1);
        let (_, _, cap_start, cap_end) = connector(&board, "straight");
        assert_eq!((cap_start, cap_end), (StrokeCap::None, StrokeCap::None));

        // Ink has no text, and a label is not to be lost: one that has a label is a connector
        // through its first and last point.
        let labelled = json!([bent, label("bent-label", "bent", "maybe")]);
        let imported = read(labelled.clone());
        let item = with_id(&imported, "bent");
        assert_eq!(item.kind, "line");
        assert_eq!(item.points, Some(vec![[0.0, 0.0], [60.0, 0.0]]));
        assert_eq!(item.label.as_ref().map(|l| l.text.as_str()), Some("maybe"));
        let mut board = drawn(labelled);
        let listed = elements(&mut board);
        assert_eq!(of_type(&listed, "ink").len(), 0, "{listed:?}");
        let connectors = of_type(&listed, "connector");
        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0]["text"], json!("maybe"));
    }

    #[test]
    fn an_id_the_scene_does_not_have_is_not_taken_for_a_node_of_the_board() {
        // Excalidraw ids are strings, but a scene of numbers is as good, and a node id is a number
        // too: an element that names "5" when the scene has no "5" must not get node 5 of the board.
        let mut board = board();
        let made = |board: &mut BoardDoc, command: &str, params: Value| {
            board.execute(command, &params).unwrap()["id"].to_string()
        };
        let sticky = made(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
        let section = made(
            &mut board,
            "node.create",
            json!({"type": "section", "x": 500, "y": 500}),
        );
        let mut outside = rectangle("box", 600.0, 600.0);
        outside["frameId"] = json!(section);
        let notes = import(
            &mut board,
            &scene(json!([
                outside,
                {"id": "arrow", "type": "arrow", "x": 100, "y": 300, "points": [[0, 0], [200, 50]],
                 "startBinding": {"elementId": sticky}, "endBinding": {"elementId": sticky}},
            ])),
        )
        .unwrap();
        assert_eq!(notes, Vec::<String>::new());

        let doc = board.doc().unwrap();
        let arrow = skeleton::resolve(&doc, "arrow").unwrap();
        let Some(BoardItem::Connector(connector)) = doc.node(arrow).unwrap().board.clone() else {
            panic!("not a connector");
        };
        assert_eq!((connector.from.node(), connector.to.node()), (None, None));
        // Nor did the rectangle go into the section because its `frameId` said so.
        let rectangle = skeleton::resolve(&doc, "box").unwrap();
        assert_eq!(doc.node(rectangle).unwrap().parent, doc.first_page());
    }

    #[test]
    fn lines_and_freedraws_keep_their_points() {
        let imported = read(json!([
            {"id": "bend", "type": "line", "x": 10, "y": 10, "points": [[0, 0], [30, 5], [60, 0]]},
            {"id": "pen", "type": "freedraw", "x": 100, "y": 100,
             "points": [[0, 0], [1.5, 2.5], [4, 4], "junk", [1], [null, 2]],
             "pressures": [0.5, 0.5, 0.5]},
        ]));
        assert_eq!(
            with_id(&imported, "pen").points,
            Some(vec![[0.0, 0.0], [1.5, 2.5], [4.0, 4.0]]),
            "a point that isn't a pair of numbers is dropped"
        );

        // A line through more than two points is ink, as is a freedraw; neither is a connector.
        let mut board = board();
        import(&mut board, &scene(json!([
            {"id": "bend", "type": "line", "x": 10, "y": 10, "points": [[0, 0], [30, 5], [60, 0]]},
            {"id": "pen", "type": "freedraw", "x": 100, "y": 100, "points": [[0, 0], [20, 30]]},
        ])))
        .unwrap();
        let elements = elements(&mut board);
        assert_eq!(of_type(&elements, "ink").len(), 2, "{elements:?}");
        assert_eq!(of_type(&elements, "connector").len(), 0);
        let bend = of_type(&elements, "ink")
            .into_iter()
            .find(|e| e["x"] == json!(10.0))
            .unwrap();
        assert_eq!(bend["width"], json!(60.0));
    }

    #[test]
    fn frames_hold_the_elements_that_name_them() {
        let mut gone =
            json!({"id": "old", "type": "frame", "x": 0, "y": 0, "width": 50, "height": 50});
        gone["isDeleted"] = json!(true);
        let mut inside = rectangle("inside", 30.0, 40.0);
        inside["frameId"] = json!("frame");
        let mut orphan = rectangle("orphan", 0.0, 0.0);
        orphan["frameId"] = json!("old");
        let mut magic = rectangle("magic", 0.0, 0.0);
        magic["frameId"] = json!("ai");
        let imported = read(json!([
            inside,
            orphan,
            magic,
            gone,
            {"id": "ai", "type": "magicframe", "x": 0, "y": 0, "width": 50, "height": 50},
            {"id": "frame", "type": "frame", "x": 10, "y": 20, "width": 300, "height": 200,
             "name": "  Plan  ", "strokeColor": "#bbb", "backgroundColor": "#ff0000",
             "frameId": "frame"},
        ]));
        assert_eq!(imported.notes, ["skipped 1 embed"]);
        assert_eq!(
            with_id(&imported, "inside").frame_id.as_deref(),
            Some("frame")
        );
        // A frame that isn't drawn can't hold anything.
        assert_eq!(with_id(&imported, "orphan").frame_id, None);
        assert_eq!(with_id(&imported, "magic").frame_id, None);
        // Frames draw in a style of their own: the colours saved with one are not its look. They
        // don't nest, either.
        let frame = with_id(&imported, "frame");
        assert_eq!(frame.name.as_deref(), Some("Plan"));
        assert_eq!(
            (
                &frame.stroke_color,
                &frame.background_color,
                &frame.frame_id
            ),
            (&None, &None, &None)
        );

        // On the board the section holds the rectangle, which keeps its place on the page.
        let mut board = board();
        import(
            &mut board,
            &scene(json!([
                {"id": "inside", "type": "rectangle", "x": 30, "y": 40, "width": 100, "height": 50,
                 "frameId": "frame"},
                {"id": "frame", "type": "frame", "x": 10, "y": 20, "width": 300, "height": 200},
            ])),
        )
        .unwrap();
        let elements = elements(&mut board);
        assert_eq!(elements.len(), 1, "{elements:?}");
        let doc = board.doc().unwrap();
        let section = doc
            .node(elements[0]["id"].as_str().unwrap().parse().unwrap())
            .unwrap();
        let rectangle = section.children[0];
        let at = doc.world_bounds(rectangle).unwrap();
        assert_eq!((at.x0, at.y0), (30.0, 40.0));
    }

    #[test]
    fn an_image_shows_the_picture_in_the_files_of_the_scene() {
        const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let image = |id: &str, file: &str| {
            json!({"id": id, "type": "image", "x": 10, "y": 20, "width": 60, "height": 30,
                   "fileId": file, "status": "saved", "scale": [1, 1]})
        };
        let mut scene = scene(json!([
            image("png", "f-png"),
            image("jpg", "f-jpg"),
            image("svg", "f-svg"),
            image("odd", "f-odd"),
            image("lost", "f-nowhere"),
            image("plain", "f-plain"),
            image("blank", "f-blank"),
            {"id": "no-file-id", "type": "image", "x": 0, "y": 0},
            {"id": "arrow", "type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [100, 100]],
             "startBinding": {"elementId": "lost", "focus": 0, "gap": 1}},
        ]));
        let url = |media: &str| format!("data:{media};base64,{PNG}");
        scene["files"] = json!({
            "f-png": {"mimeType": "image/png", "id": "f-png", "dataURL": url("image/png")},
            "f-jpg": {"mimeType": "image/jpeg", "dataURL": url("image/jpeg")},
            "f-svg": {"mimeType": "image/svg+xml", "dataURL": url("image/svg+xml")},
            // The type is in the URL only, and unknown: the importer reads the bytes.
            "f-odd": {"dataURL": url("application/octet-stream")},
            // Not base64, or nothing in it.
            "f-plain": {"mimeType": "image/svg+xml", "dataURL": "data:image/svg+xml;utf8,<svg/>"},
            "f-blank": {"mimeType": "image/png", "dataURL": "data:image/png;base64,"},
        });

        let imported = to_skeletons(&scene).unwrap();
        let picture = |id: &str| {
            let image = with_id(&imported, id);
            (image.data.as_deref(), image.name.as_deref())
        };
        assert_eq!(picture("png"), (Some(PNG), Some("Image.png")));
        assert_eq!(picture("jpg"), (Some(PNG), Some("Image.jpg")));
        assert_eq!(picture("svg"), (Some(PNG), Some("Image.svg")));
        assert_eq!(picture("odd"), (Some(PNG), Some("Image.png")));
        assert_eq!(
            (
                with_id(&imported, "png").width,
                with_id(&imported, "png").height
            ),
            (Some(60.0), Some(30.0))
        );
        // The ones with no picture are skipped, one note for them all.
        assert_eq!(
            imported.notes,
            ["skipped 4 images with no picture in the scene"]
        );
        let kept: Vec<&str> = imported
            .skeletons
            .iter()
            .filter_map(|s| s.id.as_deref())
            .collect();
        assert_eq!(kept, ["png", "jpg", "svg", "odd", "arrow"]);
        // An arrow bound to an image that isn't drawn is free at its own start.
        let arrow = with_id(&imported, "arrow");
        assert_eq!(arrow.start.as_ref().unwrap().id, None);

        // A picture comes on the board as an image layer of the size the scene gives it.
        let mut board = board();
        let mut one = scene.clone();
        one["elements"] = json!([image("png", "f-png")]);
        assert_eq!(import(&mut board, &one).unwrap(), Vec::<String>::new());
        let elements = elements(&mut board);
        assert_eq!(of_type(&elements, "image").len(), 1, "{elements:?}");
        assert_eq!(elements[0]["width"], json!(60.0));
        assert_eq!(elements[0]["height"], json!(30.0));

        // With no `files` at all there is no picture either.
        let none = json!({"elements": [image("png", "f-png")]});
        let imported = to_skeletons(&none).unwrap();
        assert!(imported.skeletons.is_empty());
        assert_eq!(
            imported.notes,
            ["skipped 1 image with no picture in the scene"]
        );
    }

    #[test]
    fn colours_come_across_when_the_engine_can_read_them() {
        let imported = read(json!([
            {"id": "ok", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
             "strokeColor": "#1E1E1E", "backgroundColor": "transparent"},
            {"id": "css", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
             "strokeColor": "rgba(255, 0, 0, 0.5)", "backgroundColor": "teal"},
            {"id": "bad", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
             "strokeColor": "currentColor", "backgroundColor": 12},
            {"id": "blank", "type": "ellipse", "x": 0, "y": 0, "width": 10, "height": 10,
             "strokeColor": "", "backgroundColor": null},
        ]));
        let colours = |id: &str| {
            let item = with_id(&imported, id);
            (
                item.stroke_color.as_deref(),
                item.background_color.as_deref(),
            )
        };
        assert_eq!(colours("ok"), (Some("#1E1E1E"), Some("transparent")));
        assert_eq!(colours("css"), (Some("rgba(255, 0, 0, 0.5)"), Some("teal")));
        // One the engine would refuse is left out, so that it can't fail the import.
        assert_eq!(colours("bad"), (None, None));
        assert_eq!(colours("blank"), (None, None));

        let mut board = board();
        let notes = import(
            &mut board,
            &scene(json!([
                {"id": "bad", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10,
                 "strokeColor": "currentColor", "backgroundColor": "nonsense"},
            ])),
        )
        .unwrap();
        assert_eq!(notes, Vec::<String>::new());
        assert_eq!(elements(&mut board).len(), 1);
    }

    #[test]
    fn a_text_is_coloured_by_its_stroke_color_and_has_no_background() {
        let mut text = json!({"id": "t", "type": "text", "x": 5, "y": 6, "text": "Hi",
                              "originalText": "Hi", "strokeColor": "#00FF00",
                              "backgroundColor": "#FF0000"});
        text["containerId"] = json!(null);
        let imported = read(json!([text.clone()]));
        let item = with_id(&imported, "t");
        assert_eq!(item.stroke_color.as_deref(), Some("#00FF00"));
        assert_eq!(item.background_color, None);

        let mut board = board();
        import(&mut board, &scene(json!([text]))).unwrap();
        let elements = elements(&mut board);
        let id = elements[0]["id"].as_str().unwrap();
        // The glyphs are green, and nothing is drawn behind or around them.
        assert_eq!(paints(&board, id), (Some("#00FF00".into()), None));
    }

    #[test]
    fn a_text_wraps_at_its_width_only_when_excalidraw_fixed_it() {
        let text = |id: &str, auto: Value| {
            let mut text = json!({"id": id, "type": "text", "x": 0, "y": 0, "width": 120,
                                  "height": 25, "text": "Hello"});
            if !auto.is_null() {
                text["autoResize"] = auto;
            }
            text
        };
        let imported = read(json!([
            text("grows", json!(true)),
            text("older", json!(null)),
            text("fixed", json!(false)),
        ]));
        // A text that grows with what is typed is as wide as it is long.
        assert_eq!(with_id(&imported, "grows").width, None);
        assert_eq!(with_id(&imported, "older").width, None);
        assert_eq!(with_id(&imported, "fixed").width, Some(120.0));
        // `text` is the text when there is no `originalText`; none at all is nothing to draw.
        assert_eq!(with_id(&imported, "grows").text.as_deref(), Some("Hello"));
        let none = read(json!([{"id": "x", "type": "text", "x": 0, "y": 0}]));
        assert!(none.skeletons.is_empty());
        assert_eq!(none.notes, Vec::<String>::new());
    }

    #[test]
    fn a_box_the_engine_would_refuse_is_held_to_the_sides_it_takes() {
        // A rectangle of no width is a line to the eye, and `board.shape` takes none under 1.
        let scene = scene(json!([
            {"id": "thin", "type": "rectangle", "x": 0, "y": 0, "width": 0, "height": 80},
            {"id": "huge", "type": "ellipse", "x": 0, "y": 0, "width": 1.0e9, "height": 5},
            {"id": "frame", "type": "frame", "x": 0, "y": 0, "width": 0, "height": 0},
            {"id": "none", "type": "diamond", "x": 0, "y": 0},
        ]));
        let imported = to_skeletons(&scene).unwrap();
        let sides = |id: &str| (with_id(&imported, id).width, with_id(&imported, id).height);
        assert_eq!(sides("thin"), (Some(1.0), Some(80.0)));
        assert_eq!(sides("huge"), (Some(100_000.0), Some(5.0)));
        assert_eq!(sides("frame"), (Some(1.0), Some(1.0)));
        // A side that isn't given stays open: the engine draws its own size.
        assert_eq!(sides("none"), (None, None));

        let mut board = board();
        let notes = import(&mut board, &scene).unwrap();
        assert_eq!(notes, Vec::<String>::new());
        assert_eq!(elements(&mut board).len(), 4);
    }

    #[test]
    fn rounded_corners_come_across() {
        let rounded = |id: &str, roundness: Value| {
            let mut item = rectangle(id, 0.0, 0.0);
            item["roundness"] = roundness;
            item
        };
        let mut board = board();
        import(
            &mut board,
            &scene(json!([
                rounded("round", json!({"type": 3})),
                rounded("sharp", json!(null)),
                {"id": "oval", "type": "ellipse", "x": 0, "y": 0, "width": 50, "height": 50,
                 "roundness": {"type": 2}},
            ])),
        )
        .unwrap();
        let elements = elements(&mut board);
        assert_eq!(of_type(&elements, "roundedRectangle").len(), 1);
        assert_eq!(of_type(&elements, "rectangle").len(), 1);
        assert_eq!(of_type(&elements, "ellipse").len(), 1);
    }

    #[test]
    fn what_is_not_an_element_is_noted_by_kind() {
        let imported = read(json!([
            1, "two", null,
            {"id": "x"},
            {"id": "e1", "type": "embeddable"},
            {"id": "e2", "type": "iframe"},
            {"id": "e3", "type": "magicframe"},
            {"id": "l1", "type": "laser"},
            {"id": "l2", "type": "laser"},
            {"id": "s", "type": "selection"},
            {"id": "long", "type": "a-type-name-that-is-much-too-long-to-show-in-a-note-in-full"},
        ]));
        assert!(imported.skeletons.is_empty());
        assert_eq!(
            imported.notes,
            [
                "skipped 3 entries of `elements` that are not objects",
                "skipped 1 element without a type",
                "skipped 2 elements of the unknown type `laser`",
                "skipped 1 element of the unknown type `selection`",
                "skipped 1 element of the unknown type `a-type-name-that-is-much-too-long-to-sho…`",
                "skipped 3 embeds",
            ]
        );
        let one = read(json!([1]));
        assert_eq!(
            one.notes,
            ["skipped 1 entry of `elements` that is not an object"]
        );
    }

    #[test]
    fn the_ids_of_a_scene_are_agent_ids_on_the_board() {
        let mut board = board();
        import(&mut board, &basic()).unwrap();
        let doc = board.doc().unwrap();
        let ink = skeleton::resolve(&doc, "scribble").unwrap();
        assert!(matches!(doc.node(ink).unwrap().board, Some(BoardItem::Ink)));
        let link = skeleton::resolve(&doc, "link").unwrap();
        assert!(matches!(
            doc.node(link).unwrap().board,
            Some(BoardItem::Connector(_))
        ));
        // And an agent can change an element by the id it had in Excalidraw.
        let reply = ops::run(
            &mut board,
            &json!({"op": "update", "elements": [{"id": "box", "text": "Renamed"}]}),
        );
        assert_eq!(reply["updated"], json!(1), "{reply}");
    }

    #[test]
    fn a_board_of_a_scene_opens_as_a_file_does() {
        let (board, notes) = to_board("Sketch", &basic()).unwrap();
        assert_eq!(notes, ["skipped 1 embed"]);
        assert_eq!(board.kind(), DocKind::Board);
        assert_eq!(board.doc().unwrap().name, "Sketch");
        // Nothing to undo: the scene is what was opened, not something done to the board.
        assert_eq!(board.session.undo_depth(), (0, 0));
        assert!(board.session.selection().is_empty());
        let mut board = board;
        assert_eq!(elements(&mut board).len(), 9);
        // What a save writes of it opens as the same board.
        let again = BoardDoc::from_bytes(&board.to_bytes().unwrap()).unwrap();
        assert_eq!(
            again.doc().unwrap().nodes().count(),
            board.doc().unwrap().nodes().count()
        );

        // A scene that is none is the error of `import`.
        let error = to_board("Sketch", &json!({})).err().unwrap();
        assert!(matches!(error, BoardError::Format(_)), "{error:?}");
    }

    #[test]
    fn an_import_is_one_undo_step_and_leaves_the_selection_alone() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        let (selection, steps) = (board.session.selection(), board.session.undo_depth());
        import(&mut board, &basic()).unwrap();
        assert_eq!(board.session.selection(), selection);
        assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
        assert_eq!(board.session.undo_labels(1), ["Add elements"]);
    }

    #[test]
    fn an_element_the_engine_refuses_is_left_out_with_a_note() {
        // Skeletons made by hand, one of which the engine refuses: its colour is nonsense, which
        // `to_skeletons` would never give. The list as a whole can't be drawn.
        let rectangle = |id: &str, colour: Option<&str>| Skeleton {
            kind: "rectangle".into(),
            id: Some(id.into()),
            width: Some(40.0),
            height: Some(40.0),
            stroke_color: colour.map(str::to_string),
            ..Skeleton::default()
        };
        let arrow = Skeleton {
            kind: "arrow".into(),
            id: Some("link".into()),
            start: Some(End {
                id: Some("good".into()),
                ..End::default()
            }),
            end: Some(End {
                id: Some("also".into()),
                ..End::default()
            }),
            ..Skeleton::default()
        };
        let items = [
            // The arrow comes first on purpose: it still finds what it joins.
            arrow,
            rectangle("good", None),
            rectangle("bad", Some("nonsense")),
            Skeleton {
                x: 200.0,
                ..rectangle("also", None)
            },
        ];
        let mut all_at_once = board();
        assert!(skeleton::apply(&mut all_at_once, &items, (0.0, 0.0)).is_err());

        let mut board = board();
        let mut notes = Vec::new();
        let refused = BoardError::Engine("the list".into());
        draw_apart(&mut board, &items, refused, &mut notes).unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].starts_with("skipped rectangle `bad`: "),
            "{notes:?}"
        );
        assert!(!notes[0].contains("elements["), "{notes:?}");
        // The others are drawn, and the arrow joins the two rectangles.
        let elements = elements(&mut board);
        assert_eq!(of_type(&elements, "rectangle").len(), 2, "{elements:?}");
        let link = of_type(&elements, "connector");
        assert_eq!(link.len(), 1);
        assert!(link[0].get("from").is_some() && link[0].get("to").is_some());
    }

    #[test]
    fn when_nothing_can_be_drawn_it_is_the_lists_error() {
        // A design file has no board objects: that is the error, not a note for each element, and
        // not the sections and pictures that it would take.
        let bytes =
            figcraft_format::save_bytes(&figcraft_doc::Document::new("Design"), None).unwrap();
        let mut design = BoardDoc::from_bytes(&bytes).unwrap();
        let before = design.doc().unwrap();
        let error = import(&mut design, &basic()).unwrap_err();
        assert!(matches!(error, BoardError::Engine(_)), "{error:?}");
        assert!(error.to_string().contains("elements["), "{error}");
        assert!(std::sync::Arc::ptr_eq(&before, &design.doc().unwrap()));
        // An empty scene draws nothing, and that is no error.
        assert_eq!(
            import(&mut design, &scene(json!([]))).unwrap(),
            Vec::<String>::new()
        );

        // On a board, when the engine refuses every element, that is the list's error as well.
        let nonsense = |id: &str| Skeleton {
            kind: "rectangle".into(),
            id: Some(id.into()),
            stroke_color: Some("nonsense".into()),
            ..Skeleton::default()
        };
        let mut board = board();
        let before = board.doc().unwrap();
        let mut notes = vec!["kept".to_string()];
        let refused = BoardError::Engine("the list".into());
        let error = draw_apart(
            &mut board,
            &[nonsense("a"), nonsense("b")],
            refused,
            &mut notes,
        )
        .unwrap_err();
        assert_eq!(error, BoardError::Engine("the list".into()));
        assert_eq!(notes, ["kept"]);
        assert!(std::sync::Arc::ptr_eq(&before, &board.doc().unwrap()));
    }

    #[test]
    fn notes_name_the_element_they_are_about() {
        // An arrow that starts and ends in one place can't be drawn, which `apply` says of
        // "elements[2]": here that is the arrow's name.
        let mut board = board();
        let notes = import(
            &mut board,
            &scene(json!([
                rectangle("a", 0.0, 0.0),
                rectangle("b", 300.0, 0.0),
                {"id": "dot", "type": "arrow", "x": 50, "y": 60, "points": [[0, 0], [0, 0]]},
            ])),
        )
        .unwrap();
        assert_eq!(
            notes,
            ["skipped arrow `dot`: it starts and ends at the same point"]
        );
        assert_eq!(elements(&mut board).len(), 2);

        // Only what looks like an index is replaced.
        let items = [Skeleton {
            kind: "text".into(),
            ..Skeleton::default()
        }];
        assert_eq!(named("elements[0]: x", &items), "text: x");
        assert_eq!(
            named("elements[9]: elements[x] elements[", &items),
            "elements[9]: elements[x] elements["
        );
    }

    #[test]
    fn read_file_reads_a_scene_or_says_why_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.excalidraw");

        let error = read_file(&path).unwrap_err();
        assert!(matches!(error, BoardError::Io(_)), "{error:?}");
        assert!(error.to_string().contains("board.excalidraw"), "{error}");

        std::fs::write(&path, "{ not json").unwrap();
        let error = read_file(&path).unwrap_err();
        assert!(matches!(error, BoardError::Format(_)), "{error:?}");
        let message = error.to_string();
        assert!(
            message.contains("board.excalidraw") && message.contains("isn't valid JSON"),
            "{message}"
        );

        // A byte order mark, which some editors write, is not part of the JSON.
        std::fs::write(&path, "\u{feff}{\"elements\": []}").unwrap();
        assert_eq!(read_file(&path).unwrap(), json!({"elements": []}));
        std::fs::write(
            &path,
            include_str!("../tests/fixtures/excalidraw/basic.excalidraw"),
        )
        .unwrap();
        assert_eq!(read_file(&path).unwrap(), basic());
    }

    #[test]
    fn a_file_with_nothing_in_it_is_a_blank_scene_only_when_asked_to_be() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("canvas.excalidraw");
        // Today's canvas opens a file like these blank: empty, white space, a byte order mark
        // (which some editors write), or any mix of them.
        for blank in [
            "",
            " ",
            " \n",
            "\t\r\n",
            "\u{feff}",
            "\u{feff}\n  ",
            " \u{feff} ",
            "\u{a0}",
        ] {
            std::fs::write(&path, blank).unwrap();
            // `read_file` is for a file somebody asked to load, and takes none of them.
            let error = read_file(&path).unwrap_err();
            assert!(
                matches!(error, BoardError::Format(_)),
                "{blank:?}: {error:?}"
            );
            assert!(
                error.to_string().contains("isn't valid JSON"),
                "{blank:?}: {error}"
            );

            let scene = read_file_or_blank(&path).unwrap();
            let imported = to_skeletons(&scene).unwrap();
            assert_eq!(imported, Imported::default(), "{blank:?}");
            // And it opens as the blank board it is, with nothing to say.
            let (mut board, notes) = to_board("Canvas", &scene).unwrap();
            assert_eq!(notes, Vec::<String>::new());
            assert!(elements(&mut board).is_empty());
        }

        // Anything with something in it is read as `read_file` reads it: what isn't there, what
        // isn't text, and what isn't JSON are errors that name the file. So is a scene with no
        // `elements`, once it is imported.
        let missing = dir.path().join("missing.excalidraw");
        assert_eq!(
            read_file_or_blank(&missing).unwrap_err(),
            read_file(&missing).unwrap_err()
        );
        for text in [&b"{ not json"[..], b"\0", b"[", &[0xFF, 0xFE, 0x00]] {
            std::fs::write(&path, text).unwrap();
            let error = read_file_or_blank(&path).unwrap_err();
            assert_eq!(error, read_file(&path).unwrap_err(), "{text:?}");
            assert!(error.to_string().contains("canvas.excalidraw"), "{error}");
        }
        for text in ["{}", "null", r#"{"type": "excalidraw"}"#] {
            std::fs::write(&path, text).unwrap();
            let scene = read_file_or_blank(&path).unwrap();
            assert_eq!(scene, read_file(&path).unwrap(), "{text}");
            assert!(to_skeletons(&scene).is_err(), "{text}");
        }
        std::fs::write(
            &path,
            include_str!("../tests/fixtures/excalidraw/basic.excalidraw"),
        )
        .unwrap();
        assert_eq!(read_file_or_blank(&path).unwrap(), basic());
    }

    #[test]
    fn a_connector_the_engine_cannot_place_does_not_fail_the_import() {
        // Two shapes drawn on top of each other and an arrow between them: the engine can't
        // place its ends apart. The shapes are worth keeping, so that is a note, not an error.
        let bound = |id: &str| json!({"elementId": id, "focus": 0, "gap": 1});
        let mut board = board();
        let notes = import(
            &mut board,
            &scene(json!([
                rectangle("a", 0.0, 0.0),
                rectangle("b", 0.0, 0.0),
                {"id": "ab", "type": "arrow", "x": 0, "y": 0, "points": [[0, 0], [10, 0]],
                 "startBinding": bound("a"), "endBinding": bound("b")},
                {"id": "after", "type": "text", "x": 0, "y": 200, "text": "still here"},
            ])),
        )
        .unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("skipped arrow `ab`: "), "{notes:?}");
        assert!(
            notes[0].ends_with("the ends are in the same place"),
            "{notes:?}"
        );
        let elements = elements(&mut board);
        assert_eq!(of_type(&elements, "rectangle").len(), 2, "{elements:?}");
        assert_eq!(of_type(&elements, "text").len(), 1);
        assert_eq!(of_type(&elements, "connector").len(), 0);
    }
}
