//! Styles, resolved down the tree as a browser resolves them, and what an element's markup says.

use figcraft_color::Color;
use roxmltree::Node;

use super::geometry::{length, transform_of};
use super::{NOT_DRAWN, SHAPES};

// --- Styles ---------------------------------------------------------------------------------------

/// The `property: value` declarations of a `style` attribute.
pub(super) fn declarations(style: &str) -> impl Iterator<Item = (&str, &str)> {
    style
        .split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .map(|(property, value)| (property.trim(), value.trim()))
}

/// What `node`'s own markup sets `property` to: its last declaration in the `style` attribute,
/// which wins over the attribute of that name.
fn declared<'a>(node: Node<'a, '_>, property: &str) -> Option<&'a str> {
    let styled = node.attribute("style").and_then(|style| {
        declarations(style)
            .filter(|(name, _)| name.eq_ignore_ascii_case(property))
            .map(|(_, value)| value)
            .last()
    });
    styled
        .or_else(|| node.attribute(property).map(str::trim))
        .filter(|value| *value != "inherit")
}

/// An inherited property (fill, stroke, text-anchor...): what `node` or the nearest group around
/// it sets it to.
pub(super) fn inherited<'a>(node: Node<'a, '_>, property: &str) -> Option<&'a str> {
    node.ancestors()
        .filter(Node::is_element)
        .find_map(|n| declared(n, property))
}

/// `node`'s fill or stroke as `#rrggbb`, or `None` for no paint: `none`, a transparent colour, or a
/// paint that is no colour (a gradient). Without a fill anywhere, the fill is SVG's black.
pub(super) fn paint(node: Node, property: &str) -> Option<String> {
    let value = match inherited(node, property) {
        Some(value) => value,
        None if property == "fill" => "black",
        None => return None,
    };
    if value.eq_ignore_ascii_case("currentcolor") {
        return inherited(node, "color").and_then(hex);
    }
    hex(value)
}

/// A CSS colour as `#rrggbb`, or `None` when it is transparent or no colour.
pub(super) fn hex(colour: &str) -> Option<String> {
    let colour = Color::parse_css(colour).filter(|c| c.a > 0.0)?;
    let [r, g, b, _] = colour.to_rgba8();
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// `colour` as it looks at `alpha` over white.
pub(super) fn tint(colour: &str, alpha: f64) -> String {
    let Some(colour) = Color::parse_css(colour) else {
        return colour.to_string();
    };
    let alpha = alpha.clamp(0.0, 1.0);
    let over_white = |v: u8| (f64::from(v) * alpha + 255.0 * (1.0 - alpha)).round() as u8;
    let [r, g, b, _] = colour.to_rgba8();
    format!(
        "#{:02x}{:02x}{:02x}",
        over_white(r),
        over_white(g),
        over_white(b)
    )
}

/// Whether `node` is drawn: not hidden, not fully transparent, not in a definition and, unless it
/// is a group or a text, painted.
pub(super) fn visible(node: Node) -> bool {
    let hidden = node
        .ancestors()
        .filter(Node::is_element)
        .any(|n| declared(n, "display") == Some("none") || NOT_DRAWN.contains(&tag(n)));
    if hidden || matches!(inherited(node, "visibility"), Some("hidden" | "collapse")) {
        return false;
    }
    if declared(node, "opacity").and_then(length) == Some(0.0) {
        return false;
    }
    matches!(tag(node), "g" | "text" | "svg")
        || paint(node, "fill").is_some()
        || paint(node, "stroke").is_some()
}

/// Whether `node` is the page that `sawit_core::mermaid::flatten` paints behind the diagram.
pub(super) fn is_background(node: Node) -> bool {
    node.has_tag_name("rect")
        && node.attribute("width") == Some("100%")
        && node.attribute("height") == Some("100%")
}

pub(super) fn tag<'a>(node: Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

/// Whether `node` is an icon: an `<svg>` inside the diagram's, which draws in its own coordinates
/// (its `viewBox`) and comes in as a picture of itself.
pub(super) fn is_icon(node: Node) -> bool {
    node.has_tag_name("svg") && node.parent_element().is_some()
}

/// Whether text `node` is turned (or slanted) on its way to the diagram's coordinates, as a lane's
/// title or an axis's is, which a board's text can't be.
pub(super) fn is_turned(node: Node) -> bool {
    let [_, b, c, _, _, _] = transform_of(node).as_coeffs();
    node.has_tag_name("text") && (b.abs() > 1e-9 || c.abs() > 1e-9)
}

/// The start tag of the element whose markup starts at `start` in `input`: up to its first `>`
/// outside a quoted attribute value.
pub(super) fn open_tag(input: &str, start: usize) -> &str {
    let mut quote = None;
    for (at, c) in input[start..].char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('>', None) => return &input[start..=start + at],
            _ => {}
        }
    }
    &input[start..]
}

/// `value` as an attribute's value in double quotes.
pub(super) fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

pub(super) fn is_shape(node: Node) -> bool {
    node.is_element() && SHAPES.contains(&tag(node))
}

pub(super) fn has_class(node: Node, classes: &[&str]) -> bool {
    node.attribute("class")
        .is_some_and(|class| class.split_whitespace().any(|c| classes.contains(&c)))
}
