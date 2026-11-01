//! Pele's colours as the board's: its neutral ones become the canvas's own, a tint as on white.

use roxmltree::Node;
use sawit_core::mermaid::Palette;

use super::geometry::length;
use super::style::{hex, inherited, paint, tint};
use super::{Converter, INK, SURFACE_ALT_FILL, TRANSPARENT};

/// What the colours of Pele's palette mean: its neutral ones carry no colour of their own.
pub(super) struct Neutrals {
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
    pub(super) fn of(palette: &Palette) -> Self {
        let colours = |colours: &[&String]| colours.iter().filter_map(|c| hex(c)).collect();
        Self {
            strokes: colours(&[&palette.foreground, &palette.line, &palette.border]),
            fills: colours(&[&palette.surface, &palette.background]),
            surface_alt: hex(&palette.surface_alt),
            background: hex(&palette.background),
        }
    }
}

impl<'a, 'input> Converter<'a, 'input> {
    // --- Colours --------------------------------------------------------------------------------

    /// A shape's colours as a skeleton has them: its fill as `backgroundColor`, its stroke as
    /// `strokeColor`. Pele's neutral colours become the canvas's own: no fill, ink lines.
    pub(super) fn shape_colours(&self, shape: Node) -> (String, String) {
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

    pub(super) fn stroke_colour(&self, shape: Node) -> String {
        match paint(shape, "stroke") {
            None => TRANSPARENT.to_string(),
            stroke => self.ink_or(stroke),
        }
    }

    /// `colour`, or ink for none or a neutral one.
    pub(super) fn ink_or(&self, colour: Option<String>) -> String {
        colour
            .filter(|c| !self.neutrals.strokes.contains(c))
            .unwrap_or_else(|| INK.to_string())
    }

    /// Whether `shape` only hides what is behind a label: a rectangle of the page's colour with no
    /// outline.
    pub(super) fn is_mask(&self, shape: Node) -> bool {
        shape.has_tag_name("rect")
            && paint(shape, "fill") == self.neutrals.background
            && paint(shape, "stroke").is_none()
    }
}
