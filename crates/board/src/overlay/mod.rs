//! What the canvas draws over the rendered page (spec D4): the selection's outlines, box, handles
//! and size, a selected ellipse's arc handles and a selected shape's corner radius handles with
//! their readouts, the hover outline, the text being edited (its box, its selected characters, an
//! input method's composition and the caret), the names of frames and sections, the marquee, snap
//! guides with their distances, where a drag drops, the previews of the pen, pencil, marker,
//! highlighter, connector and eraser, and vector edit mode's points and handles. [`overlay`] lists
//! them back to front as [`Shape`]s, in logical px from the canvas's top-left, from the engine's
//! state; the app paints the list as it is (`crates/app/src/board/overlay_paint.rs`) and only
//! measures the labels' text.
//!
//! A port of FigCraft's `ui-egui/src/canvas/overlays.rs`, with the frame names of
//! `frame_labels.rs` and the snap guides `measure_overlay.rs` draws from the tool's overlay,
//! limited to what the native canvas shows. Not drawn: layout grids, the pixel grid, constraint
//! guides, rulers and their guides, prototype connections, slot badges, the paint bucket's area,
//! shape builder regions, the lasso, the box measured to, the Scale tool's readout, auto layout
//! drop cells, swap targets, comments and the box around several selected points; names are not
//! turned with a rotated frame, and the caret doesn't blink. Every handle the Move tool answers a
//! press on is drawn.
//!
//! Every colour is a fixed canvas colour, the same on any page and in either theme ([`SELECTION`],
//! [`COMPONENT`], [`GUIDE`], white), except the names of frames and sections, whose grey reads on
//! the page they are on, and a stroke being drawn, which shows in the colour it will have. The
//! caret and the composition's underline, which FigCraft draws in its theme's text colour, are
//! [`SELECTION`], as the text's box is. So the caret is painted wider than FigCraft's, beside its
//! line ([`caret_band`]), to show where it meets the box's outline.

use figcraft_doc::NodeId;
use figcraft_engine::{Session, ViewState};
use figcraft_geom::Point;

mod geometry;
mod gestures;
mod names;
mod selection;
mod text_edit;
mod vector_edit;

pub use names::name_room;
pub use text_edit::{Composition, caret_at};

/// An sRGB colour with straight (not premultiplied) alpha.
pub type Rgba = [u8; 4];

/// The selection: its outlines, box, handles and size, the hover outline, the marquee, where a drag
/// drops and the pen's previews.
pub const SELECTION: Rgba = [0x0D, 0x99, 0xFF, 0xFF];
/// Components, component sets and instances, and every layer in them, in place of [`SELECTION`].
pub const COMPONENT: Rgba = [0x97, 0x47, 0xFF, 0xFF];
/// Snap guides and measurements, and their distances.
pub const GUIDE: Rgba = [0xF2, 0x48, 0x22, 0xFF];

/// The size of the labels' text, and the height of their line (logical px).
pub const LABEL_TEXT: f64 = 11.0;
pub const LABEL_LINE: f64 = 14.0;

/// How wide the caret is painted (logical px): wider than the 1 px outline of the text's box,
/// which it meets at the start of a line ([`caret_band`]).
pub const CARET_WIDTH: f64 = 2.0;

const WHITE: Rgba = [0xFF; 4];

/// The room around a section's name inside its pill, and around a readout inside its pill (left
/// and right, top and bottom).
const NAME_PAD: (f64, f64) = (8.0, 4.0);
const READOUT_PAD: (f64, f64) = (4.0, 2.0);
/// How far above a frame its name's line ends.
const NAME_GAP: f64 = 4.0;
/// The least width a name has, however narrow its frame (an ellipsis at least), and the most a
/// section's has (ui-egui `frame_labels.rs`).
const NAME_MIN: f64 = 20.0;
const SECTION_NAME_MAX: f64 = 480.0;
/// How far around the canvas a frame may be and still have its name drawn: names sit outside.
const NAME_REACH: f64 = 20.0;
/// How far below the selection's box its size label starts.
const SIZE_GAP: f64 = 9.0;

/// The side of the selection box's corner handles, and the size across of a connector's end
/// handle and of a pen point or vector point (larger under the pointer).
const CORNER: f64 = 8.0;
const END: f64 = 10.0;
const POINT: f64 = 7.0;
const POINT_HOVERED: f64 = 9.0;
/// The size across of an ellipse's arc handles, larger under the pointer or while dragged, and of
/// the dot on its Start handle (ui-egui `arc_overlay`: radius 4, 5 and 1.5).
const ARC: f64 = 8.0;
const ARC_HOVERED: f64 = 10.0;
const ARC_DOT: f64 = 3.0;
/// The size across of a corner radius handle and a star's ratio handle, larger under the pointer
/// or while dragged (ui-egui `shape_overlay`: radius 3.5 and 4.5).
const RADIUS: f64 = 7.0;
const RADIUS_HOVERED: f64 = 9.0;
/// How far right of and below a dragged handle its readout starts.
const READOUT_OFFSET: f64 = 12.0;

/// The colours of names on a light page or a dark one: a frame's name, and an unselected section's
/// name and pill (FigCraft's secondary text, text and hover colours in its light and dark themes).
struct Names {
    frame: Rgba,
    section: Rgba,
    pill: Rgba,
}

const ON_LIGHT: Names = Names {
    frame: [0x6E, 0x6E, 0x6E, 0xFF],
    section: [0x1E, 0x1E, 0x1E, 0xFF],
    pill: [0xF5, 0xF5, 0xF5, 0xFF],
};
const ON_DARK: Names = Names {
    frame: [0xB3, 0xB3, 0xB3, 0xFF],
    section: WHITE,
    pill: [0x38, 0x38, 0x38, 0xFF],
};

/// One thing drawn over the canvas. Positions are logical px from the canvas's top-left.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A line through `points`, back to the first when `closed`, `width` px wide.
    Outline {
        points: Vec<(f64, f64)>,
        closed: bool,
        color: Rgba,
        width: f64,
        dashed: bool,
    },
    /// The polygon through `points`, filled.
    Fill {
        points: Vec<(f64, f64)>,
        color: Rgba,
    },
    /// A handle `size` px across at `center`: a circle when `round`, else a square, with `fill`
    /// inside a 1 px `stroke`.
    Handle {
        center: (f64, f64),
        size: f64,
        round: bool,
        fill: Rgba,
        stroke: Rgba,
    },
    /// One line of text, [`LABEL_TEXT`] px on a [`LABEL_LINE`] px line: the name of the frame or
    /// section `node`, or a readout (the selection's size, a guide's distance, a dragged handle's
    /// value) when `node` is `None`. With a `background` the text sits on a pill of that colour.
    /// `at` is the point of its box that `anchor` says. [`Shape::label_box`] says where it goes
    /// once its text is measured, and [`name_room`] how wide a name may be.
    Label {
        at: (f64, f64),
        anchor: Anchor,
        text: String,
        color: Rgba,
        background: Option<Rgba>,
        node: Option<NodeId>,
    },
    /// The text caret: its line, from `top` to `bottom`, before the character it is at. It is
    /// painted as the band [`caret_band`] gives, beside that line.
    Caret {
        top: (f64, f64),
        bottom: (f64, f64),
        color: Rgba,
    },
}

/// Which point of a label's box its `at` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The box's top-left corner: a frame's or section's name, and the readout beside a dragged
    /// handle.
    TopLeft,
    /// The box's middle: the selection's size and a guide's distance, centred on what they
    /// measure, however wide their text.
    Middle,
}

impl Shape {
    /// Where a label goes once its text is measured `text_width` px wide: the top-left corner and
    /// the size of its box. The box is the text's line, padded into a pill when the label has a
    /// background (more around a name than around a readout). Its top-left or its middle is at
    /// `at`, as its [`Anchor`] says. The text is centred in the box. `None` for every other shape.
    pub fn label_box(&self, text_width: f64) -> Option<((f64, f64), (f64, f64))> {
        let Shape::Label {
            at,
            anchor,
            background,
            node,
            ..
        } = self
        else {
            return None;
        };
        let (pad_x, pad_y) = match (background, node) {
            (None, _) => (0.0, 0.0),
            (Some(_), Some(_)) => NAME_PAD,
            (Some(_), None) => READOUT_PAD,
        };
        let size = (text_width.max(0.0) + 2.0 * pad_x, LABEL_LINE + 2.0 * pad_y);
        let top_left = match anchor {
            Anchor::TopLeft => *at,
            Anchor::Middle => (at.0 - size.0 / 2.0, at.1 - size.1 / 2.0),
        };
        Some((top_left, size))
    }
}

/// Where the caret whose line runs from `top` to `bottom` (a [`Shape::Caret`]) is painted, as a
/// polygon: the band [`CARET_WIDTH`] px wide from that line toward the text's right, as a text view
/// draws its insertion point. Painted on its line, a caret at the start of a line would vanish into
/// the outline of the text's box, which is the same colour on the same edge (ui-egui draws the
/// caret in its theme's text colour instead, which the canvas's fixed colours leave out). `None`
/// for a caret with no length.
pub fn caret_band(top: (f64, f64), bottom: (f64, f64)) -> Option<[(f64, f64); 4]> {
    let (dx, dy) = (bottom.0 - top.0, bottom.1 - top.1);
    let length = dx.hypot(dy);
    if !length.is_normal() {
        return None;
    }
    // A quarter turn from down the text's lines is the text's right, however it is turned.
    let (rx, ry) = (dy / length * CARET_WIDTH, -dx / length * CARET_WIDTH);
    Some([
        top,
        (top.0 + rx, top.1 + ry),
        (bottom.0 + rx, bottom.1 + ry),
        bottom,
    ])
}

/// What to draw over the canvas of `s`'s open document, back to front:
///
/// 1. the names of frames and sections (labels with a `node`);
/// 2. the outline of the layer under the pointer, when it isn't selected;
/// 3. the outline of each selected layer;
/// 4. the selection's box, outlined around several layers, and its 4 corner handles (a
///    connector's 2 round end handles instead);
/// 5. the selection's size, `"{w} × {h}"` under its box, then a selected ellipse's arc handles and
///    a selected shape's corner radius handles, with the value of the one dragged;
/// 6. while text is edited, in place of 3 to 5: the edited text's box, its selected characters,
///    the input method's composition, underlined, and the caret;
/// 7. the marquee;
/// 8. snap guides and measurements, with their distances;
/// 9. the frame a drag drops into, and the line where it goes in an auto layout;
/// 10. the pen's path, the stroke the pencil, marker or highlighter is drawing, the connector
///     being drawn and the eraser;
/// 11. in vector edit mode, the edited layers' segments, points and handles.
///
/// Item 6 lays out the text as it shows, with the composition under way in it
/// ([`Composition::of`]); [`overlay_with`] takes one already made.
pub fn overlay(s: &Session) -> Vec<Shape> {
    overlay_with(s, Composition::of(s).as_ref())
}

/// [`overlay`], with `composition` the composition under way in the text `s` edits, as the
/// canvas keeps it ([`Composition::refresh`]) and renders it.
pub fn overlay_with(s: &Session, composition: Option<&Composition>) -> Vec<Shape> {
    let Ok(ds) = s.ds() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    names::frame_names(&mut out, ds);
    selection::hover(&mut out, s, ds);
    selection::selection(&mut out, s, ds);
    text_edit::text_edit(&mut out, s, ds, composition);
    let (ov, view) = (&s.tool.overlay, &ds.view);
    gestures::marquee(&mut out, ov, view);
    gestures::guides(&mut out, ov, view);
    gestures::drops(&mut out, ov, &ds.doc, view);
    gestures::previews(&mut out, s, ds);
    vector_edit::vector_edit(&mut out, s, ds);
    out
}

/// The page quad (top-left, top-right, bottom-right, bottom-left) under the box on screen whose
/// top-left corner is `top_left` and whose size is `size`: how a frame's name, measured where it
/// was painted, goes into the engine's `ToolState::labels`, which takes a press or hover there as
/// one on the frame.
pub fn label_quad(top_left: (f64, f64), size: (f64, f64), view: &ViewState) -> [Point; 4] {
    let (x, y) = top_left;
    let (w, h) = size;
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(sx, sy)| {
        let (px, py) = view.screen_to_page(sx, sy);
        Point::new(px, py)
    })
}

#[cfg(test)]
mod tests;
