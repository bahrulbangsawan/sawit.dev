//! Texts: what one says, where Pele drew it and how wide the engine sets it, as a board's text.

use figcraft_doc::TextData;
use kurbo::{Affine, Point, Rect};
use roxmltree::Node;

use super::geometry::{coordinate, length, transform_of};
use super::style::{declarations, inherited, is_turned, paint};
use super::{BASELINE, Converter, FONT_SIZE, LINE_HEIGHT};
use crate::skeleton::Skeleton;

// --- Text -----------------------------------------------------------------------------------------

impl<'a, 'input> Converter<'a, 'input> {
    /// A text of the board's. Its anchor stays where Pele put it, at the size Pele set it in, and
    /// its colour (its fill) travels in `strokeColor`, as an Excalidraw text's does. One Pele
    /// turned is a picture of itself ([`Self::turned_text`]).
    pub(super) fn text(&mut self, text: Node<'a, 'input>) -> Skeleton {
        if is_turned(text) {
            return self.turned_text(text);
        }
        self.done.insert(text.id());
        let place = text_place(text);
        let size = place.size;
        let width = place.set_width(size);
        let height = place.lines as f64 * size * LINE_HEIGHT;
        Skeleton {
            kind: "text".into(),
            x: place.at.x - place.anchor.before(width),
            y: place.at.y - height / 2.0,
            text: Some(text_of(text)),
            font_size: Some(size),
            stroke_color: Some(self.ink_or(paint(text, "fill"))),
            ..Skeleton::default()
        }
    }

    /// A text Pele turned, as a picture of itself where Pele drew it: a board's text can't turn.
    fn turned_text(&mut self, text: Node<'a, 'input>) -> Skeleton {
        self.done.insert(text.id());
        let place = text_place_in(text, Affine::IDENTITY);
        // Room for the guess of its width to be short.
        let local = place.bounds().inflate(place.size, place.size * 0.25);
        let frame = transform_of(text).transform_rect_bbox(local);
        self.cut(&[text], frame, "text.svg")
    }
}

/// What a text says: its lines (a text's spans are one line while they share a `y`), each trimmed.
pub(super) fn text_of(text: Node) -> String {
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
pub(super) fn ems(text: &str) -> f64 {
    text.chars().map(advance).sum()
}

/// About how wide the widest line of `text` is, in ems.
pub(super) fn widest_line(text: &str) -> f64 {
    text.lines().map(ems).fold(0.0, f64::max)
}

/// How wide the engine sets `line` at font `size`, in px: as its text layout measures it in the
/// face new text takes (Inter, [`crate::fonts`]), and never under the estimate ([`ems`]), so that
/// a face that isn't there yet makes room too.
pub(super) fn set_width(line: &str, size: f64) -> f64 {
    crate::fonts::register();
    let mut text = TextData::new(line);
    text.style.font_size = size;
    let measured = figcraft_text::layout(&text, None, None).width;
    (ems(line) * size).max(measured)
}

/// How wide the engine sets the widest line of `text` at font `size`, in px.
pub(super) fn set_widest_line(text: &str, size: f64) -> f64 {
    text.lines()
        .map(|line| set_width(line, size))
        .fold(0.0, f64::max)
}

/// How wide the engine sets the widest word of `text` at font `size`, in px.
pub(super) fn set_widest_word(text: &str, size: f64) -> f64 {
    text.split_whitespace()
        .map(|word| set_width(word, size))
        .fold(0.0, f64::max)
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
pub(super) struct TextPlace {
    anchor: Anchor,
    /// The anchor's x, and the middle of the text's lines, in the diagram's coordinates.
    at: Point,
    /// The font size Pele set it in.
    size: f64,
    lines: usize,
    /// About how wide its widest line is, in ems.
    ems: f64,
    /// What it says.
    said: String,
}

impl TextPlace {
    /// About how wide its widest line is at font `size`.
    fn width(&self, size: f64) -> f64 {
        self.ems * size
    }

    /// How wide the engine sets its widest line at font `size`.
    fn set_width(&self, size: f64) -> f64 {
        set_widest_line(&self.said, size)
    }

    /// About the box Pele drew it in.
    pub(super) fn bounds(&self) -> Rect {
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
pub(super) fn text_place(text: Node) -> TextPlace {
    text_place_in(text, transform_of(text))
}

/// Where `text` is drawn, in the coordinates that `to` takes its own to: the diagram's for
/// [`transform_of`] it, its own for the identity.
fn text_place_in(text: Node, to: Affine) -> TextPlace {
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
        at: to * middle,
        size,
        lines,
        ems: widest_line(&said),
        said,
    }
}

/// The font size of `node`'s text, rounded: from `font-size` or the `font` shorthand, on it or
/// the nearest group around it that sets one.
pub(super) fn font_size(node: Node) -> f64 {
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
