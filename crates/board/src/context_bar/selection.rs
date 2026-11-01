//! The selection's board items: which of them take part in the bar, what each has for each
//! control, what they share, and where the bar floats over them.

use figcraft_color::{Color, Paint};
use figcraft_doc::{
    BOARD_COLORS, BoardItem, BoardShape, Connector, Document, Node, NodeKind, Routing, StrokeCap,
};
use figcraft_engine::DocState;
use figcraft_engine::board::{TEXT_SIZES, text_of};
use figcraft_engine::tools::selection_box;

use super::GAP;

/// The board colours as swatches: `(name, rgba8)`.
pub(super) fn palette() -> Vec<(&'static str, [u8; 4])> {
    BOARD_COLORS
        .iter()
        .map(|(name, colour)| (*name, colour.to_rgba8()))
        .collect()
}

/// The name `board.setStyle` reads for `cap`: FigCraft's own camelCase ids.
pub(super) fn cap_name(cap: StrokeCap) -> &'static str {
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
pub(super) fn shared<T: PartialEq>(
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
pub(super) fn takes_part(node: &Node) -> bool {
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
pub(super) fn colour(doc: &Document, item: &Node) -> Option<Option<&'static str>> {
    Some(match item.board.as_ref()? {
        BoardItem::Sticky { .. } => colour_name(&item.fills, false),
        BoardItem::Shape { .. } => colour_name(&doc.node(*item.children.first()?)?.fills, false),
        BoardItem::Connector(_) | BoardItem::Ink => colour_name(&item.strokes, true),
        BoardItem::Label { .. } | BoardItem::Stamp { .. } | BoardItem::Table => return None,
    })
}

/// The outline control of `item`: only a shape has one.
pub(super) fn shape(item: &Node) -> Option<Option<BoardShape>> {
    match &item.board {
        Some(BoardItem::Shape { shape }) => Some(Some(*shape)),
        _ => None,
    }
}

/// The text size control of `item`: a shape that has its text. A sticky fits its note to its
/// size, and a connector's label is not offered one.
pub(super) fn font_size(doc: &Document, item: &Node) -> Option<Option<&'static str>> {
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
pub(super) fn routing(item: &Node) -> Option<Option<Routing>> {
    connector(item).map(|connector| Some(connector.routing))
}

/// The start cap control of `item`: only a connector has one.
pub(super) fn start_cap(item: &Node) -> Option<Option<&'static str>> {
    connector(item).map(|_| Some(cap_name(item.stroke.cap_start)))
}

/// The end cap control of `item`: only a connector has one.
pub(super) fn end_cap(item: &Node) -> Option<Option<&'static str>> {
    connector(item).map(|_| Some(cap_name(item.stroke.cap_end)))
}

/// Where the bar's bottom-centre goes: the middle of the top of the selection's box on screen,
/// [`GAP`] above it. `None` when the selection has no box, or the view makes no point of it.
pub(super) fn anchor(ds: &DocState) -> Option<(f64, f64)> {
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
