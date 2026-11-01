//! What a skeleton's `type` names, and what its fields say of how it is drawn.

use serde_json::Value;

use super::{LINE_LENGTH, SAME_POINT, SECTION_SIZE, SHAPE_SIZE, Skeleton};

/// What a skeleton's `type` can be.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Element {
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
pub(super) const STAGES: usize = 4;

impl Element {
    /// The element a `type` names, in any case.
    pub(super) fn of(kind: &str) -> Option<Self> {
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
    pub(super) fn stage(self) -> usize {
        match self {
            Self::Frame => 0,
            Self::Rectangle | Self::Ellipse | Self::Diamond | Self::Sticky | Self::Image => 1,
            Self::Text => 2,
            Self::Arrow | Self::Line | Self::Freedraw => 3,
        }
    }

    /// Whether it is drawn through points rather than as a box.
    pub(super) fn is_linear(self) -> bool {
        matches!(self, Self::Arrow | Self::Line | Self::Freedraw)
    }

    /// The size of its box: what the skeleton gives, else what it is drawn with.
    pub(super) fn size(self, item: &Skeleton) -> (f64, f64) {
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
pub(super) fn line_extent(item: &Skeleton) -> (f64, f64) {
    match (item.width, item.height) {
        (None, None) => (LINE_LENGTH, 0.0),
        (width, height) => (width.unwrap_or(0.0), height.unwrap_or(0.0)),
    }
}

/// Whether `points` draw a line: two or more, not all in one place.
pub(super) fn draws_a_line(points: &[[f64; 2]]) -> bool {
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
pub(super) fn is_rounded(roundness: Option<&Value>) -> bool {
    match roundness {
        None | Some(Value::Null) => false,
        Some(Value::Bool(rounded)) => *rounded,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(_) => true,
    }
}

/// The text of a shape, a sticky, an arrow or a line: its label, else its `text`.
pub(super) fn label_or_text(item: &Skeleton) -> Option<&str> {
    let label = item
        .label
        .as_ref()
        .map(|l| l.text.as_str())
        .filter(|t| !t.is_empty());
    label.or(item.text.as_deref())
}

/// The text of a text element: its `text`, else its label.
pub(super) fn text_or_label(item: &Skeleton) -> Option<&str> {
    let label = item.label.as_ref().map(|l| l.text.as_str());
    item.text.as_deref().or(label)
}
