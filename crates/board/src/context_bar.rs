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

use figcraft_color::{Color, Paint};
use figcraft_doc::{
    BOARD_COLORS, BoardItem, BoardShape, Connector, Document, Node, NodeKind, Routing, StrokeCap,
};
use figcraft_engine::board::{TEXT_SIZES, is_board, text_of};
use figcraft_engine::tools::selection_box;
use figcraft_engine::{DocState, Session};
use serde_json::{Value, json};

/// How far above the selection the bar's bottom edge floats.
const GAP: f64 = 12.0;

/// The room the bar keeps to each edge of the canvas.
const MARGIN: f64 = 8.0;

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

/// How the view calls each of [`CAPS`], in the same order.
const CAP_LABELS: [&str; CAPS.len()] = [
    "None",
    "Line arrow",
    "Triangle arrow",
    "Reversed triangle",
    "Circle",
    "Diamond",
];

/// The ways a connector can run, in the order the bar offers them, and how the view calls each.
const ROUTINGS: [(Routing, &str); 3] = [
    (Routing::Straight, "Straight"),
    (Routing::Elbow, "Elbow"),
    (Routing::Curved, "Curved"),
];

/// The board colours as swatches: `(name, rgba8)`.
fn palette() -> Vec<(&'static str, [u8; 4])> {
    BOARD_COLORS
        .iter()
        .map(|(name, colour)| (*name, colour.to_rgba8()))
        .collect()
}

/// The name `board.setStyle` reads for `cap`: FigCraft's own camelCase ids.
fn cap_name(cap: StrokeCap) -> &'static str {
    match cap {
        StrokeCap::None => "none",
        StrokeCap::Round => "round",
        StrokeCap::Square => "square",
        StrokeCap::LineArrow => "lineArrow",
        StrokeCap::TriangleArrow => "triangleArrow",
        StrokeCap::ReversedTriangle => "reversedTriangle",
        StrokeCap::CircleFilled => "circleFilled",
        StrokeCap::DiamondFilled => "diamondFilled",
    }
}

/// FigCraft's `line_shade`, which is private there: `board.setColor` draws connectors, ink and a
/// shape's outline line in a deeper shade of the board colour (0.55 of each channel), because the
/// light tints are too faint as lines. It is how a line's colour is told from its board colour, so
/// `current_colour_is_the_board_colour_each_item_was_set_to` pins it to the engine at the pinned
/// rev.
fn line_shade(colour: Color) -> Color {
    Color::new(colour.r * 0.55, colour.g * 0.55, colour.b * 0.55, colour.a)
}

/// The name of the board colour that `paints` show, if their first solid paint is one: the colour
/// itself for a fill, its [`line_shade`] for a line. Colours are compared as 8-bit channels, so one
/// written as a hex code (by an agent, say) matches too.
fn colour_name(paints: &[Paint], line: bool) -> Option<&'static str> {
    let painted = paints.iter().find_map(Paint::solid_color)?.to_rgba8();
    BOARD_COLORS
        .iter()
        .find(|(_, colour)| {
            let drawn = if line { line_shade(*colour) } else { *colour };
            drawn.to_rgba8() == painted
        })
        .map(|(name, _)| *name)
}

/// The name of the text size `size` is, if it is one of [`TEXT_SIZES`].
fn size_name(size: f64) -> Option<&'static str> {
    TEXT_SIZES
        .iter()
        .find(|(_, named)| (named - size).abs() < 0.01)
        .map(|(name, _)| *name)
}

/// The value that every one of `values` is. `None` when they differ, or when any of them is `None`
/// (none of the named values).
fn common<T: PartialEq>(values: impl IntoIterator<Item = Option<T>>) -> Option<T> {
    let mut values = values.into_iter();
    let first = values.next()??;
    values
        .all(|value| value.as_ref() == Some(&first))
        .then_some(first)
}

/// How the control that `read` reads off an item shows for `items`. `read` gives `None` for an item
/// that doesn't have the control, else `Some` of the item's value, which is itself `None` when it
/// is none of the named values. The result is `None` unless every item has the control (the bar
/// leaves it out), else `Some` of the value the items agree on (`None` when they don't).
fn shared<T: PartialEq>(
    items: &[&Node],
    read: impl Fn(&Node) -> Option<Option<T>>,
) -> Option<Option<T>> {
    let readings = items
        .iter()
        .map(|item| read(item))
        .collect::<Option<Vec<_>>>()?;
    Some(common(readings))
}

/// Whether `node` is one of the board items that the bar has controls for. A selection's other
/// layers are left out of what the items share (see the module's docs).
fn takes_part(node: &Node) -> bool {
    matches!(
        node.board,
        Some(
            BoardItem::Sticky { .. }
                | BoardItem::Shape { .. }
                | BoardItem::Connector(_)
                | BoardItem::Ink
        )
    )
}

/// The colour control of `item`: the name of the board colour it is, as `board.setColor` paints
/// it. A sticky is filled with it, and a shape's outline (its first layer) too; a connector's or
/// ink's line is its [`line_shade`].
fn colour(doc: &Document, item: &Node) -> Option<Option<&'static str>> {
    Some(match item.board.as_ref()? {
        BoardItem::Sticky { .. } => colour_name(&item.fills, false),
        BoardItem::Shape { .. } => colour_name(&doc.node(*item.children.first()?)?.fills, false),
        BoardItem::Connector(_) | BoardItem::Ink => colour_name(&item.strokes, true),
        BoardItem::Label { .. } | BoardItem::Stamp { .. } | BoardItem::Table => return None,
    })
}

/// The outline control of `item`: only a shape has one.
fn shape(item: &Node) -> Option<Option<BoardShape>> {
    match &item.board {
        Some(BoardItem::Shape { shape }) => Some(Some(*shape)),
        _ => None,
    }
}

/// The text size control of `item`: a shape that has its text. A sticky fits its note to its
/// size, and a connector's label is not offered one.
fn font_size(doc: &Document, item: &Node) -> Option<Option<&'static str>> {
    if !matches!(item.board, Some(BoardItem::Shape { .. })) {
        return None;
    }
    let NodeKind::Text(text) = &doc.node(text_of(doc, item.id)?)?.kind else {
        return None;
    };
    // Parts of the text can have a size of their own: the size is one only if they all agree.
    let sizes = std::iter::once(&text.style).chain(text.runs.iter().map(|run| &run.style));
    Some(common(sizes.map(|style| size_name(style.font_size))))
}

/// `item` as a connector, if it is one.
fn connector(item: &Node) -> Option<&Connector> {
    match &item.board {
        Some(BoardItem::Connector(connector)) => Some(connector),
        _ => None,
    }
}

/// The routing control of `item`: only a connector has one.
fn routing(item: &Node) -> Option<Option<Routing>> {
    connector(item).map(|connector| Some(connector.routing))
}

/// The start cap control of `item`: only a connector has one.
fn start_cap(item: &Node) -> Option<Option<&'static str>> {
    connector(item).map(|_| Some(cap_name(item.stroke.cap_start)))
}

/// The end cap control of `item`: only a connector has one.
fn end_cap(item: &Node) -> Option<Option<&'static str>> {
    connector(item).map(|_| Some(cap_name(item.stroke.cap_end)))
}

/// Where the bar's bottom-centre goes: the middle of the top of the selection's box on screen,
/// [`GAP`] above it. `None` when the selection has no box, or the view makes no point of it.
fn anchor(ds: &DocState) -> Option<(f64, f64)> {
    let corners = selection_box(&ds.doc, &ds.selection)?;
    // The box turns with a rotated layer: its top is its highest corner, and its middle is the
    // middle of the span of its corners.
    let top = corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let (left, right) = corners
        .iter()
        .map(|p| p.x)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(left, right), x| {
            (left.min(x), right.max(x))
        });
    let (x, y) = ds.view.page_to_screen((left + right) / 2.0, top);
    let anchor = (x, y - GAP);
    (anchor.0.is_finite() && anchor.1.is_finite()).then_some(anchor)
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

/// An option that runs `board.setStyle` with `key` set to `value`.
fn style_option(key: &str, value: Value, label: &str, selected: bool) -> ControlOption {
    ControlOption {
        command: "board.setStyle",
        params: json!({ key: value }),
        label: label.to_owned(),
        swatch: None,
        selected,
    }
}

/// The options of a cap control: `key` (`startCap` or `endCap`) set to each of [`CAPS`].
fn cap_options(key: &str, current: Option<&str>) -> Vec<ControlOption> {
    CAPS.iter()
        .zip(CAP_LABELS)
        .map(|(cap, label)| style_option(key, json!(cap), label, Some(*cap) == current))
        .collect()
}

/// What `control` offers, a [`ControlOption`] for each button of the view, in order: the control's
/// own colours, [`BoardShape::ALL`], [`TEXT_SIZES`], straight, elbow and curved, or [`CAPS`].
///
/// The view draws these, and runs the command of the one picked with its params on the selection,
/// through `BoardView::choose`. Every option works on the whole selection, whether or not it is
/// the one selected, except a selected shape option: the engine refuses a shape that every
/// selected shape already has, so the view runs nothing for that one.
///
/// `selected` marks the option that is the control's current value. No option is selected when
/// the selected items differ, or have a value that the control has no option for.
pub fn options(control: &Control) -> Vec<ControlOption> {
    match control {
        Control::Colors { items, current } => items
            .iter()
            .map(|(name, rgba)| ControlOption {
                command: "board.setColor",
                params: json!({ "color": name }),
                label: (*name).to_owned(),
                swatch: Some(*rgba),
                selected: Some(*name) == *current,
            })
            .collect(),
        Control::Shape { current } => BoardShape::ALL
            .iter()
            .map(|shape| {
                let selected = Some(*shape) == *current;
                style_option("shape", json!(shape.id()), shape.label(), selected)
            })
            .collect(),
        // The engine takes the size, not its name.
        Control::FontSize { current } => TEXT_SIZES
            .iter()
            .map(|(name, size)| {
                style_option("fontSize", json!(size), name, Some(*name) == *current)
            })
            .collect(),
        Control::Routing { current } => ROUTINGS
            .iter()
            .map(|(routing, label)| {
                let selected = Some(*routing) == *current;
                style_option("style", json!(routing.id()), label, selected)
            })
            .collect(),
        Control::StartCap { current } => cap_options("startCap", current.as_deref()),
        Control::EndCap { current } => cap_options("endCap", current.as_deref()),
    }
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
mod tests {
    use figcraft_doc::{FileKind, NodeId, StyleRun};

    use super::*;
    use crate::doc::{BoardDoc, DocKind};

    /// A new board seen through an 800 × 600 canvas, centred on the page point (100, 100) at 100 %:
    /// the page origin is at (300, 200) on screen.
    fn board() -> BoardDoc {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let view = json!({"cx": 100, "cy": 100, "zoom": 1, "vw": 800, "vh": 600});
        board.execute("view.set", &view).unwrap();
        board
    }

    /// Runs `command` on `board`, which selects what it makes, and returns that layer.
    fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
        let made = board.execute(command, &params).unwrap();
        NodeId(made["id"].as_u64().unwrap())
    }

    /// A sticky at (0, 0), 200 px wide, in the default yellow.
    fn sticky(board: &mut BoardDoc) -> NodeId {
        make(board, "board.sticky", json!({"x": 0, "y": 0}))
    }

    /// A rectangle with text at (300, 0), 160 px square, in the default yellow and size M.
    fn shape(board: &mut BoardDoc) -> NodeId {
        let params = json!({"shape": "rectangle", "x": 300, "y": 0});
        make(board, "board.shape", params)
    }

    /// An elbow connector with an arrow at its end, between two free points.
    fn connector(board: &mut BoardDoc) -> NodeId {
        let params = json!({"from": {"x": 0, "y": 400}, "to": {"x": 300, "y": 400}});
        make(board, "board.connector", params)
    }

    /// A marker stroke.
    fn ink(board: &mut BoardDoc) -> NodeId {
        make(
            board,
            "board.ink",
            json!({"points": [[0, 600], [100, 650]]}),
        )
    }

    fn select(board: &mut BoardDoc, ids: &[NodeId]) {
        board.session.set_selection(ids.to_vec());
    }

    /// The colours control showing `current`.
    fn colors(current: Option<&'static str>) -> Control {
        Control::Colors {
            items: palette(),
            current,
        }
    }

    /// The bar for the selection now.
    fn bar(board: &BoardDoc) -> ContextBar {
        context_bar(&board.session).unwrap()
    }

    /// The controls' names, in order.
    fn kinds(bar: &ContextBar) -> Vec<&'static str> {
        bar.controls
            .iter()
            .map(|control| match control {
                Control::Colors { .. } => "colors",
                Control::Shape { .. } => "shape",
                Control::FontSize { .. } => "font size",
                Control::Routing { .. } => "routing",
                Control::StartCap { .. } => "start cap",
                Control::EndCap { .. } => "end cap",
            })
            .collect()
    }

    #[test]
    fn sticky_selection_shows_colours() {
        let mut b = board();
        sticky(&mut b);
        assert_eq!(bar(&b).controls, [colors(Some("yellow"))]);
    }

    #[test]
    fn shape_selection_shows_colours_outline_and_size() {
        let mut b = board();
        shape(&mut b);
        assert_eq!(
            bar(&b).controls,
            [
                colors(Some("yellow")),
                Control::Shape {
                    current: Some(BoardShape::Rectangle)
                },
                Control::FontSize { current: Some("M") },
            ]
        );
    }

    #[test]
    fn connector_selection_shows_routing_and_caps() {
        let mut b = board();
        connector(&mut b);
        assert_eq!(
            bar(&b).controls,
            [
                // The line is drawn in a dark grey, which is no board colour's shade.
                colors(None),
                Control::Routing {
                    current: Some(Routing::Elbow)
                },
                Control::StartCap {
                    current: Some("none".into())
                },
                Control::EndCap {
                    current: Some("triangleArrow".into())
                },
            ]
        );
    }

    #[test]
    fn ink_selection_shows_colours() {
        let mut b = board();
        ink(&mut b);
        // The marker's ink is no board colour either.
        assert_eq!(bar(&b).controls, [colors(None)]);
    }

    #[test]
    fn swatches_are_the_board_colours() {
        let mut b = board();
        sticky(&mut b);
        let bar = bar(&b);
        let Control::Colors { items, .. } = &bar.controls[0] else {
            panic!("{:?}", bar.controls);
        };
        assert_eq!(
            items,
            &[
                ("yellow", [0xFF, 0xE7, 0x8A, 0xFF]),
                ("orange", [0xFF, 0xC9, 0x94, 0xFF]),
                ("red", [0xFF, 0xAE, 0xA6, 0xFF]),
                ("pink", [0xF7, 0xB9, 0xDA, 0xFF]),
                ("violet", [0xD4, 0xC2, 0xFC, 0xFF]),
                ("blue", [0xAC, 0xD3, 0xFA, 0xFF]),
                ("green", [0xC2, 0xE8, 0xA6, 0xFF]),
                ("gray", [0xE3, 0xE1, 0xDD, 0xFF]),
            ]
        );
    }

    #[test]
    fn mixed_selection_shows_shared_controls() {
        let mut b = board();
        let sticky = sticky(&mut b);
        let connector = connector(&mut b);
        select(&mut b, &[sticky, connector]);
        // The sticky is yellow and the connector's line is not a board colour: no swatch is on.
        assert_eq!(bar(&b).controls, [colors(None)]);
        // The order of the selection makes no difference.
        select(&mut b, &[connector, sticky]);
        assert_eq!(bar(&b).controls, [colors(None)]);

        // A sticky and a shape share the colour, and have the same one.
        let shape = shape(&mut b);
        select(&mut b, &[sticky, shape]);
        assert_eq!(bar(&b).controls, [colors(Some("yellow"))]);

        // A shape and a connector share the colour only.
        select(&mut b, &[shape, connector]);
        assert_eq!(kinds(&bar(&b)), ["colors"]);
    }

    #[test]
    fn items_that_differ_show_no_current_value() {
        let mut b = board();
        let first = shape(&mut b);
        let second = make(
            &mut b,
            "board.shape",
            json!({"shape": "ellipse", "x": 600, "y": 0}),
        );
        select(&mut b, &[first, second]);
        // The same colour and size, a different outline.
        assert_eq!(
            bar(&b).controls,
            [
                colors(Some("yellow")),
                Control::Shape { current: None },
                Control::FontSize { current: Some("M") },
            ]
        );
        // Now the size differs too, and the colour.
        b.execute("board.setStyle", &json!({"ids": [second], "fontSize": 24}))
            .unwrap();
        b.execute("board.setColor", &json!({"ids": [second], "color": "blue"}))
            .unwrap();
        assert_eq!(
            bar(&b).controls,
            [
                colors(None),
                Control::Shape { current: None },
                Control::FontSize { current: None },
            ]
        );

        // Two connectors: the same caps, a different routing.
        let first = connector(&mut b);
        let second = make(
            &mut b,
            "board.connector",
            json!({"from": {"x": 0, "y": 800}, "to": {"x": 300, "y": 900}, "style": "straight"}),
        );
        select(&mut b, &[first, second]);
        assert_eq!(
            bar(&b).controls,
            [
                colors(None),
                Control::Routing { current: None },
                Control::StartCap {
                    current: Some("none".into())
                },
                Control::EndCap {
                    current: Some("triangleArrow".into())
                },
            ]
        );
    }

    #[test]
    fn other_layers_are_left_out_of_what_is_shared() {
        let mut b = board();
        let sticky = sticky(&mut b);
        let connector = connector(&mut b);
        let others = [
            make(
                &mut b,
                "board.stamp",
                json!({"stamp": "heart", "x": 700, "y": 700}),
            ),
            make(
                &mut b,
                "board.table",
                json!({"rows": 2, "columns": 2, "x": 800, "y": 0}),
            ),
            make(
                &mut b,
                "node.create",
                json!({"type": "section", "x": -500, "y": 0, "width": 300, "height": 300}),
            ),
            make(
                &mut b,
                "shape.rectangle",
                json!({"x": -500, "y": 400, "width": 100, "height": 100}),
            ),
            // The label on the connector.
            NodeId(
                b.execute("board.label", &json!({"id": connector, "text": "yes"}))
                    .unwrap()["label"]
                    .as_u64()
                    .unwrap(),
            ),
        ];
        for other in others {
            // On their own they take nothing from the bar.
            select(&mut b, &[other]);
            assert_eq!(context_bar(&b.session), None, "{other:?}");
            // Beside a sticky they don't take its controls away, or put theirs in.
            select(&mut b, &[sticky, other]);
            assert_eq!(bar(&b).controls, [colors(Some("yellow"))], "{other:?}");
            // And beside a connector the connector's own controls stay.
            select(&mut b, &[connector, other]);
            assert_eq!(
                kinds(&bar(&b)),
                ["colors", "routing", "start cap", "end cap"],
                "{other:?}"
            );
        }
    }

    #[test]
    fn nothing_selected_has_no_context_bar() {
        let mut b = board();
        assert_eq!(context_bar(&b.session), None);
        let sticky = sticky(&mut b);
        assert!(context_bar(&b.session).is_some());
        select(&mut b, &[]);
        assert_eq!(context_bar(&b.session), None);
        select(&mut b, &[sticky]);
        assert!(context_bar(&b.session).is_some());
    }

    #[test]
    fn design_doc_has_no_context_bar() {
        // A design file, with a sticky selected in it (pasted from a board, say).
        let mut b = board();
        sticky(&mut b);
        assert!(context_bar(&b.session).is_some());
        b.session.doc_mut().unwrap().kind = FileKind::Design;
        assert_eq!(b.kind(), DocKind::Design);
        assert_eq!(context_bar(&b.session), None);

        // A design file with nothing in it, and a session with no file.
        let design = figcraft_format::save_bytes(&Document::new("Design"), None).unwrap();
        let design = BoardDoc::from_bytes(&design).unwrap();
        assert_eq!(context_bar(&design.session), None);
        assert_eq!(context_bar(&Session::empty()), None);
    }

    #[test]
    fn a_shape_without_its_text_or_outline_offers_only_what_still_works() {
        let mut b = board();
        let shape = shape(&mut b);
        let layers = b.doc().unwrap().node(shape).unwrap().children.clone();
        let [outline, text] = layers[..] else {
            panic!("a shape holds an outline and a text: {layers:?}");
        };

        // Without its text there is no size to set, which the engine refuses.
        b.execute("edit.delete", &json!({"ids": [text]})).unwrap();
        select(&mut b, &[shape]);
        assert_eq!(kinds(&bar(&b)), ["colors", "shape"]);
        assert!(
            b.execute("board.setStyle", &json!({"fontSize": 12}))
                .is_err()
        );

        // Without its outline it has nothing to colour either, and that is refused too.
        b.execute("edit.delete", &json!({"ids": [outline]}))
            .unwrap();
        select(&mut b, &[shape]);
        assert_eq!(kinds(&bar(&b)), ["shape"]);
        assert!(
            b.execute("board.setColor", &json!({"color": "blue"}))
                .is_err()
        );
        // What is left, the outline, still works: the first outline the shape doesn't have.
        let choices = options(&bar(&b).controls[0]);
        let choice = choices.iter().find(|option| !option.selected).unwrap();
        b.execute(choice.command, &choice.params).unwrap();
    }

    #[test]
    fn a_text_size_is_current_only_when_all_of_the_text_has_it() {
        let mut b = board();
        let shape = make(&mut b, "board.shape", json!({"x": 0, "y": 0, "text": "ab"}));
        let text = text_of(&b.doc().unwrap(), shape).unwrap();
        // The second letter in the size XL, the rest in M.
        let doc = b.session.doc_mut().unwrap();
        let Some(NodeKind::Text(data)) = doc.node_mut(text).map(|n| &mut n.kind) else {
            panic!("{text:?} is not text");
        };
        let mut style = data.style.clone();
        style.font_size = 36.0;
        data.runs.push(StyleRun {
            start: 1,
            end: 2,
            style,
        });
        assert_eq!(bar(&b).controls[2], Control::FontSize { current: None });

        // Picking a size sizes all of the text, and then that is the size.
        b.execute("board.setStyle", &json!({"fontSize": 12}))
            .unwrap();
        assert_eq!(
            bar(&b).controls[2],
            Control::FontSize { current: Some("S") }
        );
    }

    #[test]
    fn current_colour_is_the_board_colour_each_item_was_set_to() {
        let mut b = board();
        let items = [
            sticky(&mut b),
            shape(&mut b),
            connector(&mut b),
            ink(&mut b),
        ];
        for item in items {
            select(&mut b, &[item]);
            for (name, _) in BOARD_COLORS {
                b.execute("board.setColor", &json!({"color": name}))
                    .unwrap();
                assert_eq!(bar(&b).controls[0], colors(Some(name)), "{item:?}");
            }
            // A colour that is not one of the board's is no swatch's.
            b.execute("board.setColor", &json!({"color": "#123456"}))
                .unwrap();
            assert_eq!(bar(&b).controls[0], colors(None), "{item:?}");
        }
    }

    #[test]
    fn cap_names_are_the_ones_the_engine_reads() {
        let mut b = board();
        let id = connector(&mut b);
        for cap in StrokeCap::ALL {
            b.execute("board.setStyle", &json!({"endCap": cap_name(cap)}))
                .unwrap();
            assert_eq!(b.doc().unwrap().node(id).unwrap().stroke.cap_end, cap);
        }
        // The bar offers some of them, and reads them back.
        for name in CAPS {
            assert!(StrokeCap::ALL.iter().any(|cap| cap_name(*cap) == *name));
            let params = json!({"startCap": name, "endCap": name});
            b.execute("board.setStyle", &params).unwrap();
            let controls = bar(&b).controls;
            let (start, end) = (&controls[2], &controls[3]);
            assert_eq!(
                start,
                &Control::StartCap {
                    current: Some(name.to_string())
                }
            );
            assert_eq!(
                end,
                &Control::EndCap {
                    current: Some(name.to_string())
                }
            );
        }
    }

    #[test]
    fn controls_run() {
        let mut b = board();
        let items = [
            sticky(&mut b),
            shape(&mut b),
            connector(&mut b),
            ink(&mut b),
        ];
        let mut run = 0;
        for item in items {
            select(&mut b, &[item]);
            let before = bar(&b);
            for (at, control) in before.controls.iter().enumerate() {
                // The first option that is not the item's value now: the engine refuses to give a
                // shape the outline it has.
                let choices = options(control);
                let picked = choices.iter().position(|option| !option.selected).unwrap();
                let ControlOption {
                    command, params, ..
                } = &choices[picked];
                b.execute(command, params)
                    .unwrap_or_else(|e| panic!("{item:?} {control:?} {command} {params}: {e}"));
                // And the bar shows what was picked, and nothing else, as the value now.
                let after = bar(&b);
                assert_eq!(kinds(&after), kinds(&before));
                let on: Vec<usize> = options(&after.controls[at])
                    .iter()
                    .enumerate()
                    .filter(|(_, option)| option.selected)
                    .map(|(i, _)| i)
                    .collect();
                assert_eq!(on, [picked], "{item:?} {command} {params}");
                run += 1;
            }
        }
        // 1 for the sticky, 3 for the shape, 4 for the connector and 1 for the ink.
        assert_eq!(run, 9);
    }

    /// What picking each option of `control` runs, and what the option is called: `(command,
    /// params, label)`.
    fn picks(control: &Control) -> Vec<(&'static str, Value, String)> {
        let options = options(control).into_iter();
        options.map(|o| (o.command, o.params, o.label)).collect()
    }

    /// The labels of the options of `control` that show as selected.
    fn selected(control: &Control) -> Vec<String> {
        let options = options(control).into_iter();
        options.filter(|o| o.selected).map(|o| o.label).collect()
    }

    #[test]
    fn options_are_the_commands_the_controls_run() {
        // Written out here, not read from the lists the engine keeps them in, so that a change to
        // one of those has to be seen.
        let colour = |name: &str| ("board.setColor", json!({ "color": name }), name.to_owned());
        let style = |key: &str, value: Value, label: &str| {
            ("board.setStyle", json!({ key: value }), label.to_owned())
        };

        let colours = [
            "yellow", "orange", "red", "pink", "violet", "blue", "green", "gray",
        ];
        assert_eq!(picks(&colors(None)), colours.map(colour));

        let shapes = [
            ("rectangle", "Rectangle"),
            ("roundedRectangle", "Rounded rectangle"),
            ("ellipse", "Ellipse"),
            ("diamond", "Diamond"),
            ("triangle", "Triangle"),
            ("speechBubble", "Speech bubble"),
            ("chevron", "Chevron"),
        ];
        assert_eq!(
            picks(&Control::Shape { current: None }),
            shapes.map(|(id, label)| style("shape", json!(id), label))
        );

        // The size is the number, never its name.
        let sizes = [("S", 12.0), ("M", 16.0), ("L", 24.0), ("XL", 36.0)];
        assert_eq!(
            picks(&Control::FontSize { current: None }),
            sizes.map(|(name, size)| style("fontSize", json!(size), name))
        );

        let routings = [
            ("straight", "Straight"),
            ("elbow", "Elbow"),
            ("curved", "Curved"),
        ];
        assert_eq!(
            picks(&Control::Routing { current: None }),
            routings.map(|(id, label)| style("style", json!(id), label))
        );

        let caps = [
            ("none", "None"),
            ("lineArrow", "Line arrow"),
            ("triangleArrow", "Triangle arrow"),
            ("reversedTriangle", "Reversed triangle"),
            ("circleFilled", "Circle"),
            ("diamondFilled", "Diamond"),
        ];
        assert_eq!(
            picks(&Control::StartCap { current: None }),
            caps.map(|(name, label)| style("startCap", json!(name), label))
        );
        assert_eq!(
            picks(&Control::EndCap { current: None }),
            caps.map(|(name, label)| style("endCap", json!(name), label))
        );

        // The colours are the only options with a swatch, and it is the control's own.
        let swatches: Vec<_> = options(&colors(None)).iter().map(|o| o.swatch).collect();
        let palette: Vec<_> = palette().iter().map(|(_, rgba)| Some(*rgba)).collect();
        assert_eq!(swatches, palette);
        for control in [
            Control::Shape { current: None },
            Control::FontSize { current: None },
            Control::Routing { current: None },
            Control::StartCap { current: None },
            Control::EndCap { current: None },
        ] {
            assert!(options(&control).iter().all(|o| o.swatch.is_none()));
        }

        // The colours are the control's own, whole: all five fields of an option.
        let own = Control::Colors {
            items: vec![("mauve", [1, 2, 3, 255])],
            current: Some("mauve"),
        };
        assert_eq!(
            options(&own),
            [ControlOption {
                command: "board.setColor",
                params: json!({ "color": "mauve" }),
                label: "mauve".to_owned(),
                swatch: Some([1, 2, 3, 255]),
                selected: true,
            }]
        );
    }

    #[test]
    fn the_selected_option_is_the_value_the_control_shows() {
        assert_eq!(selected(&colors(Some("blue"))), ["blue"]);
        assert_eq!(
            selected(&Control::Shape {
                current: Some(BoardShape::Ellipse)
            }),
            ["Ellipse"]
        );
        assert_eq!(selected(&Control::FontSize { current: Some("L") }), ["L"]);
        assert_eq!(
            selected(&Control::Routing {
                current: Some(Routing::Curved)
            }),
            ["Curved"]
        );
        let cap = |name: &str| Some(name.to_owned());
        assert_eq!(
            selected(&Control::StartCap {
                current: cap("lineArrow")
            }),
            ["Line arrow"]
        );
        assert_eq!(
            selected(&Control::EndCap {
                current: cap("diamondFilled")
            }),
            ["Diamond"]
        );

        // No value (the items differ), or one the bar has no option for (a plain round cap, which
        // FigCraft has but the bar doesn't offer): nothing shows as selected.
        for control in [
            colors(None),
            Control::Shape { current: None },
            Control::FontSize { current: None },
            Control::Routing { current: None },
            Control::StartCap { current: None },
            Control::EndCap {
                current: cap("round"),
            },
        ] {
            assert_eq!(selected(&control), Vec::<String>::new(), "{control:?}");
        }

        // A real selection: a connector is elbow, has no cap at its start and a triangle at its
        // end, and its line is in no board colour.
        let mut b = board();
        connector(&mut b);
        let on: Vec<_> = bar(&b).controls.iter().map(selected).collect();
        assert_eq!(
            on,
            [vec![], vec!["Elbow"], vec!["None"], vec!["Triangle arrow"]]
        );
    }

    #[test]
    fn every_option_runs_but_the_shape_the_item_has() {
        let mut b = board();
        let items = [
            sticky(&mut b),
            shape(&mut b),
            connector(&mut b),
            ink(&mut b),
        ];
        let mut run = 0;
        for item in items {
            select(&mut b, &[item]);
            for at in 0..bar(&b).controls.len() {
                for pick in 0..options(&bar(&b).controls[at]).len() {
                    // What is selected moves with each pick, so ask the bar again every time.
                    let control = bar(&b).controls[at].clone();
                    let option = options(&control).swap_remove(pick);
                    let result = b.execute(option.command, &option.params);
                    // The engine refuses a shape the item has already. Everything else works,
                    // whether or not it is what the item has now.
                    let refused = matches!(control, Control::Shape { .. }) && option.selected;
                    assert_eq!(
                        result.is_err(),
                        refused,
                        "{item:?} {} {}: {result:?}",
                        option.command,
                        option.params
                    );
                    run += 1;
                }
            }
        }
        // The 8 colours for each of the 4 items, 7 shapes, 4 sizes, 3 routings, and 6 caps twice.
        assert_eq!(run, 4 * 8 + 7 + 4 + 3 + 2 * 6);
    }

    #[test]
    fn anchor_is_clamped_inside_canvas() {
        let (bar, canvas) = ((300.0, 36.0), (800.0, 600.0));
        // Past the top-left corner: the bar's centre is held 8 px plus half its width from the
        // left, and its top 8 px from the top.
        assert_eq!(clamp_anchor((5.0, -40.0), bar, canvas), (158.0, 8.0));
        // Past the bottom-right corner: its right edge and its bottom are 8 px in.
        assert_eq!(clamp_anchor((790.0, 590.0), bar, canvas), (642.0, 556.0));
        // A bar that fits is where it was put.
        assert_eq!(clamp_anchor((400.0, 300.0), bar, canvas), (400.0, 300.0));
        assert_eq!(clamp_anchor((158.0, 8.0), bar, canvas), (158.0, 8.0));
        assert_eq!(clamp_anchor((642.0, 556.0), bar, canvas), (642.0, 556.0));

        // However far out the anchor is, the bar ends up inside.
        let far = [
            -1e9,
            -300.0,
            0.0,
            5.0,
            400.0,
            799.0,
            1e9,
            f64::NEG_INFINITY,
            f64::INFINITY,
        ];
        for x in far {
            for y in far {
                let (cx, top) = clamp_anchor((x, y), bar, canvas);
                let (left, right, bottom) = (cx - bar.0 / 2.0, cx + bar.0 / 2.0, top + bar.1);
                assert!(left >= 8.0 && right <= canvas.0 - 8.0, "({x}, {y}) -> {cx}");
                assert!(
                    top >= 8.0 && bottom <= canvas.1 - 8.0,
                    "({x}, {y}) -> {top}"
                );
            }
        }

        // A canvas too small for the bar and its margins has no good place for it. It must not
        // panic; the margins at the left and the top win.
        assert_eq!(clamp_anchor((50.0, 50.0), bar, (200.0, 30.0)), (158.0, 8.0));
        assert_eq!(
            clamp_anchor((900.0, 900.0), bar, (200.0, 30.0)),
            (158.0, 8.0)
        );
        assert_eq!(clamp_anchor((0.0, 0.0), bar, (0.0, 0.0)), (158.0, 8.0));
        // Wide enough but not tall enough, and the other way round.
        assert_eq!(
            clamp_anchor((400.0, 300.0), bar, (800.0, 30.0)),
            (400.0, 8.0)
        );
        assert_eq!(
            clamp_anchor((400.0, 300.0), bar, (200.0, 600.0)),
            (158.0, 300.0)
        );
    }

    #[test]
    fn anchor_floats_above_the_middle_of_the_selections_top() {
        let mut b = board();
        let sticky = sticky(&mut b);
        // The sticky is 200 wide at the page origin, which is at (300, 200): its top-centre is
        // (400, 200), and the bar's bottom-centre 12 px above it.
        assert_eq!(bar(&b).anchor, (400.0, 188.0));

        // Zoomed to 200 % about the middle of the canvas, the page origin is at (200, 100).
        b.execute("view.set", &json!({"zoom": 2})).unwrap();
        assert_eq!(bar(&b).anchor, (400.0, 88.0));
        b.execute("view.set", &json!({"zoom": 1})).unwrap();

        // Several layers: the top of the box around all of them. The shape is 160 wide at x 300.
        let shape = shape(&mut b);
        select(&mut b, &[sticky, shape]);
        assert_eq!(bar(&b).anchor, (530.0, 188.0));
    }

    #[test]
    fn a_turned_selection_floats_above_its_highest_corner() {
        let mut b = board();
        let shape = shape(&mut b);
        // Turned 45 degrees about its middle, (380, 80), the square's corners are 80 * sqrt(2) from
        // it: one points straight up.
        b.execute("transform.rotate", &json!({"ids": [shape], "degrees": 45}))
            .unwrap();
        let (x, y) = bar(&b).anchor;
        let top = 200.0 + 80.0 - 80.0 * 2f64.sqrt();
        assert!((x - 680.0).abs() < 1e-6, "{x}");
        assert!((y - (top - 12.0)).abs() < 1e-6, "{y}");
    }

    #[test]
    fn an_off_screen_selection_still_has_a_bar_that_lands_inside_the_canvas() {
        let mut b = board();
        make(&mut b, "board.sticky", json!({"x": -5000, "y": -5000}));
        let bar = bar(&b);
        assert_eq!(bar.anchor, (-4600.0, -4812.0));
        // A 300 × 36 bar sits above its anchor; the canvas is 800 × 600.
        let (x, y) = bar.anchor;
        assert_eq!(
            clamp_anchor((x, y - 36.0), (300.0, 36.0), (800.0, 600.0)),
            (158.0, 8.0)
        );
    }
}
