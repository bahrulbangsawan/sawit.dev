//! A scene's elements as the canvas draws them: their kinds, labels and pictures, and what is skipped.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::Fields;
use super::arrows::{Heads, drawn_along_its_route, end_of, free_at, turn_around, without_heads};
use super::fields::{colour, flag, number, points_of, side, string, text_of};
use crate::skeleton::{Label, Skeleton};

/// What a scene's element can be that the canvas draws.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
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
    pub(super) fn skeleton(self) -> &'static str {
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
    pub(super) fn is_linear(self) -> bool {
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
pub(super) struct Item<'a> {
    pub(super) kind: Kind,
    /// The element's id. Other elements name it in `containerId`, `startBinding`, `endBinding` and
    /// `frameId`.
    pub(super) id: Option<&'a str>,
    pub(super) fields: &'a Fields,
    /// What an image shows.
    picture: Option<Picture<'a>>,
}

impl<'a> Item<'a> {
    /// The element in `element`, or `None` when it isn't drawn: it is deleted, an embed, of a type
    /// the canvas doesn't know, or an image with no picture. What isn't a deleted element is
    /// counted in `skipped`.
    pub(super) fn read(
        element: &'a Value,
        files: Option<&'a Fields>,
        skipped: &mut Skipped,
    ) -> Option<Self> {
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
    /// the elements that a line can join, by id, each with its fields. A name that isn't one of
    /// those is left out, so that it can't be taken for something else on the board: a numeric id
    /// is a node id too.
    pub(super) fn skeleton(
        &self,
        at: usize,
        labels: &Labels,
        frames: &HashSet<&str>,
        joinable: &HashMap<&str, &Fields>,
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
    ///
    /// A connector can't join one element at both ends: the engine would draw it from the element
    /// to where the route ends, a stub. What Excalidraw draws there is the route, a loop back to
    /// the element (a state's transition to itself, say), so such a line joins neither and keeps
    /// its route. An arrow of that kind is drawn along it ([`drawn_along_its_route`]).
    fn run_between(&self, item: &mut Skeleton, labels: &Labels, joinable: &HashMap<&str, &Fields>) {
        let origin = (item.x, item.y);
        let points = item.points.as_deref().unwrap_or_default();
        let mut start = end_of(self.fields, "startBinding", points.iter(), origin, joinable);
        let mut end = end_of(
            self.fields,
            "endBinding",
            points.iter().rev(),
            origin,
            joinable,
        );
        let looped = start.id.is_some() && start.id == end.id;
        if looped {
            start = free_at(points.first(), origin);
            end = free_at(points.last(), origin);
        }
        item.start = Some(start);
        item.end = Some(end);
        item.label = self.label(labels);
        if self.kind != Kind::Arrow {
            return;
        }
        match Heads::of(self.fields) {
            Heads::End => {}
            Heads::Start => turn_around(item),
            // With no head it is a line, and a line that joins nothing keeps its bends.
            Heads::Neither => {
                without_heads(item);
                return;
            }
        }
        if looped && item.label.is_none() {
            drawn_along_its_route(item);
        }
    }

    /// The text bound to this element.
    fn label(&self, labels: &Labels) -> Option<Label> {
        let text = labels.of.get(self.id?)?;
        Some(Label {
            text: (*text).to_string(),
            font_size: None,
        })
    }
}

/// The texts that are bound to another element as its label.
pub(super) struct Labels<'a> {
    /// The text of each element that has one, by the element's id.
    of: HashMap<&'a str, &'a str>,
    /// The indexes of the items that are such texts, and so aren't drawn as texts.
    used: HashSet<usize>,
}

impl<'a> Labels<'a> {
    /// The labels among `items`: a text that names, in `containerId`, an item that can hold one. A
    /// text with no characters has no label to give, and when two texts name one element the
    /// first is its label and the other stays a text.
    pub(super) fn find(items: &[Item<'a>]) -> Self {
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
pub(super) struct Skipped {
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
    pub(super) fn notes(self) -> Vec<String> {
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
