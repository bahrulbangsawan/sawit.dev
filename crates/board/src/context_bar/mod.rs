//! The context bar's model: the small bar that floats over a board's selection with the controls
//! the selected items share (colour, outline, text size, connector style and caps). It ports
//! FigCraft's `ui-egui/src/canvas/board_bar.rs`, which decides the same things for its own UI. The
//! view in `crates/app/src/board/context_bar.rs` draws what [`context_bar`] returns, a button for
//! each of [`options`] of each control, and runs the one picked through `BoardView::choose`, with
//! the command and params its [`ControlOption`] has: they are the ones the tests here run. The one
//! option the view doesn't run is a selected shape, which the engine refuses because the selection
//! has that shape already.
//!
//! What each kind of item takes, as `board.setColor` and `board.setStyle` take it:
//!
//! | Item | Controls |
//! |---|---|
//! | sticky | colours |
//! | shape | colours, outline, text size |
//! | connector | colours, routing, start cap, end cap |
//! | ink | colours |
//!
//! A sticky has no size control: FigCraft sizes a sticky's note to fit it, and `board.setStyle`
//! leaves it alone. A mixed selection gets only the controls all of its items have. Other selected
//! layers (stamps, connector labels, tables, sections, layers that are not board items) are left
//! out of that: both commands skip the layers that take nothing, so a control the items share works
//! for the whole selection.
//!
//! Lengths are logical px from the canvas's top-left.

use figcraft_doc::{BoardShape, Document, Node, Routing};
use figcraft_engine::Session;
use figcraft_engine::board::is_board;
use serde_json::Value;

use selection::{
    anchor, colour, end_cap, font_size, palette, routing, shape, shared, start_cap, takes_part,
};

mod options;
mod selection;

pub use options::options;

/// How far above the selection the bar's bottom edge floats.
const GAP: f64 = 12.0;

/// The room the bar keeps to each edge of the canvas. The view draws the bar no wider than the
/// canvas less this room at each side, so that [`clamp_anchor`] keeps all of it inside.
pub const MARGIN: f64 = 8.0;

/// One control of the bar, with the value the selection has for it now: `None` when the selected
/// items differ, or have none of the named values. [`options`] lists what it offers, the buttons
/// the view draws for it, each with the command that picking it runs (which each variant names).
#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    /// The board colours, as `(name, rgba8)` in FigCraft's order. `board.setColor {color}` with the
    /// name.
    Colors {
        items: Vec<(&'static str, [u8; 4])>,
        current: Option<&'static str>,
    },
    /// A shape's outline, one of [`BoardShape::ALL`]. `board.setStyle {shape}` with
    /// [`BoardShape::id`]. The engine refuses a shape that every selected shape already has, so a
    /// view treats picking the current one as a no-op rather than a click that fails.
    Shape { current: Option<BoardShape> },
    /// A shape's text size, by its name in `figcraft_engine::board::TEXT_SIZES` (S, M, L, XL).
    /// `board.setStyle {fontSize}` with the size that name has there.
    FontSize { current: Option<&'static str> },
    /// How a connector runs. `board.setStyle {style}` with [`Routing::id`].
    Routing { current: Option<Routing> },
    /// A connector's start cap, one of [`CAPS`]. `board.setStyle {startCap}` with the name.
    StartCap { current: Option<String> },
    /// A connector's end cap, one of [`CAPS`]. `board.setStyle {endCap}` with the name.
    EndCap { current: Option<String> },
}

/// The caps a connector's start and end can be set to, by the name `board.setStyle` reads: none, a
/// line arrow, a triangle, a reversed triangle, a dot and a diamond. FigCraft's plain round and
/// square caps are not offered.
pub const CAPS: &[&str] = &[
    "none",
    "lineArrow",
    "triangleArrow",
    "reversedTriangle",
    "circleFilled",
    "diamondFilled",
];

/// The bar to float over the selection. The view draws each of its controls as the buttons of
/// [`options`] and runs the one picked through `BoardView::choose`, except a selected shape option:
/// the engine refuses the shape the selection has already, so that one runs nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextBar {
    /// Where the bar's bottom-centre goes: the middle of the selection's top on screen, 12 px above
    /// it. It can lie outside the canvas (a selection near an edge, or off-screen); [`clamp_anchor`]
    /// brings the bar in.
    pub anchor: (f64, f64),
    /// What the selected items share, in the order of [`Control`].
    pub controls: Vec<Control>,
}

/// One button of a [`Control`]: what the view draws for an option and the command that picking it
/// runs on the selection. It has the fields of `toolbar::FlyoutItem`, so the view can draw the two
/// alike.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlOption {
    /// The command picking the option runs: `board.setColor` or `board.setStyle`.
    pub command: &'static str,
    /// The command's params: the one value the option sets. The command acts on the selection.
    pub params: Value,
    /// What the view calls the option: the colour's name, the shape's label, the text size's name
    /// (S, M, L or XL), the routing's or the cap's label.
    pub label: String,
    /// The colour to draw for a colour option, `None` for the others.
    pub swatch: Option<[u8; 4]>,
    /// Whether the option is the value the control shows now.
    pub selected: bool,
}

/// The bar for the selection of `s`: what its sticky notes, shapes, connectors and ink share, and
/// where it goes. `None` when there is nothing to show: the file is not a board, none of those is
/// selected, or the selection has no box on screen.
///
/// Hiding the bar while a gesture is on or text is being edited is up to the view: the model only
/// follows the selection.
pub fn context_bar(s: &Session) -> Option<ContextBar> {
    let ds = s.ds().ok()?;
    if !is_board(&ds.doc) {
        return None;
    }
    let doc: &Document = &ds.doc;
    let items: Vec<&Node> = ds
        .selection
        .iter()
        .filter_map(|id| doc.node(*id))
        .filter(|node| takes_part(node))
        .collect();
    if items.is_empty() {
        return None;
    }
    let controls: Vec<Control> = [
        shared(&items, |item| colour(doc, item)).map(|current| Control::Colors {
            items: palette(),
            current,
        }),
        shared(&items, shape).map(|current| Control::Shape { current }),
        shared(&items, |item| font_size(doc, item)).map(|current| Control::FontSize { current }),
        shared(&items, routing).map(|current| Control::Routing { current }),
        shared(&items, start_cap).map(|current| Control::StartCap {
            current: current.map(str::to_owned),
        }),
        shared(&items, end_cap).map(|current| Control::EndCap {
            current: current.map(str::to_owned),
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    if controls.is_empty() {
        return None;
    }
    Some(ContextBar {
        anchor: anchor(ds)?,
        controls,
    })
}

/// Moves a bar of size `bar` so that it lies inside a canvas of size `canvas`, at least 8 px from
/// each edge. `anchor` is the bar's top-centre, and so is the result. [`ContextBar::anchor`] is
/// where the bar's bottom-centre goes, so a bar above it has its top-centre at
/// `(anchor.0, anchor.1 - bar.1)`.
///
/// A canvas too small for the bar and its margins has no good place for it: the bar then sits 8 px
/// from the canvas's left and top and runs out at the right and the bottom. This never panics.
pub fn clamp_anchor(anchor: (f64, f64), bar: (f64, f64), canvas: (f64, f64)) -> (f64, f64) {
    let half = bar.0 / 2.0;
    // `min` then `max`, not `clamp`, which panics when the bounds cross: here the left and top
    // margins win.
    let x = anchor.0.min(canvas.0 - MARGIN - half).max(MARGIN + half);
    let y = anchor.1.min(canvas.1 - MARGIN - bar.1).max(MARGIN);
    (x, y)
}

#[cfg(test)]
mod tests;
