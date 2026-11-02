//! The items of the toolbar's flyouts: the colours, shapes, connector styles and stamps the board
//! tools make, and the tools of a group that shows one of them at a time.

use figcraft_doc::{BOARD_COLORS, BoardShape, Routing, Stamp};
use figcraft_engine::{Session, ToolId};
use serde_json::json;

use super::groups::tool_groups;
use super::{Flyout, FlyoutItem, ToolIcon, active};
use crate::DocKind;

/// The command of Place image…, the last item of the shape tools' flyout: Sawit asks for the
/// image file (`crate::policy::UI_COMMANDS`).
pub const PLACE_IMAGE: &str = "file.placeImage";

/// How a new connector can run, in the order the flyout lists them, and how the flyout calls
/// each.
const ROUTINGS: [(Routing, &str); 3] = [
    (Routing::Straight, "Straight"),
    (Routing::Elbow, "Elbow"),
    (Routing::Curved, "Curved"),
];

/// A flyout item that runs `command` with the one param `key` set to `value`.
fn item(command: &'static str, key: &str, value: &str, label: &str, selected: bool) -> FlyoutItem {
    FlyoutItem {
        command,
        params: json!({ key: value }),
        label: label.to_owned(),
        swatch: None,
        selected,
        icon: None,
    }
}

/// The items of flyout `f`, each running the command of the tool the flyout belongs to with the
/// choice as its param. The item that is what the tool makes now is `selected`, so the view can
/// mark it:
///
/// | Flyout | Items | Runs | Selected |
/// |---|---|---|---|
/// | [`Flyout::StickyColors`] | `BOARD_COLORS` | `tool.sticky {color}` | the colour of new stickies |
/// | [`Flyout::Shapes`] | `BoardShape::ALL` | `tool.shape {shape}` | the shape the tool places |
/// | [`Flyout::ConnectorStyles`] | straight, elbow, curved | `tool.connector {style}` | how new connectors run |
/// | [`Flyout::Stamps`] | `Stamp::ALL` | `tool.stamp {stamp}` | the stamp the tool places |
/// | [`Flyout::Tools`] | the group's tools, then Place image… for the shape tools | `tool.…`, [`PLACE_IMAGE`] | the tool in use |
///
/// The names in the params are the ones the engine reads: the colour's name, and the camelCase ids
/// of the shapes, the styles and the stamps. A colour item has its colour as `swatch`. When new
/// stickies have a colour that is none of the eight (an agent set it by hex code), none of the
/// items is selected.
pub fn flyout(s: &Session, f: Flyout) -> Vec<FlyoutItem> {
    // What the board tools make now: the colour, the shape, how connectors run, the stamp.
    let now = &s.tool.board;
    match f {
        Flyout::StickyColors => BOARD_COLORS
            .iter()
            .map(|(name, colour)| {
                // Compared as 8-bit channels, as the context bar tells a colour: a colour set by
                // hex code that is one of the eight counts as it.
                let selected = colour.to_rgba8() == now.sticky.to_rgba8();
                FlyoutItem {
                    swatch: Some(colour.to_rgba8()),
                    ..item("tool.sticky", "color", name, name, selected)
                }
            })
            .collect(),
        Flyout::Shapes => BoardShape::ALL
            .iter()
            .map(|shape| {
                let selected = *shape == now.shape;
                item("tool.shape", "shape", shape.id(), shape.label(), selected)
            })
            .collect(),
        Flyout::ConnectorStyles => ROUTINGS
            .iter()
            .map(|(routing, label)| {
                let selected = *routing == now.routing;
                item("tool.connector", "style", routing.id(), label, selected)
            })
            .collect(),
        Flyout::Stamps => Stamp::ALL
            .iter()
            .map(|stamp| {
                let selected = *stamp == now.stamp;
                item("tool.stamp", "stamp", stamp.id(), stamp.label(), selected)
            })
            .collect(),
        Flyout::Tools(key) => group_tools(s, key),
    }
}

/// The items of the flyout of the group `key` of the toolbar of `s`'s file: its tools, the one in
/// use selected, and for the shape tools Place image… last.
fn group_tools(s: &Session, key: &str) -> Vec<FlyoutItem> {
    let kind = match s.doc() {
        Ok(doc) if figcraft_engine::board::is_board(doc) => DocKind::Board,
        _ => DocKind::Design,
    };
    let Some(group) = tool_groups(kind).iter().find(|group| group.key == key) else {
        return Vec::new();
    };
    let shapes = group
        .tools
        .iter()
        .any(|tool| tool.tool == ToolId::Rectangle);
    let mut items: Vec<FlyoutItem> = group
        .tools
        .iter()
        .map(|tool| FlyoutItem {
            command: tool.command,
            params: json!({}),
            label: tool.label.to_owned(),
            swatch: None,
            selected: tool.tool == active(s),
            icon: Some(tool.icon),
        })
        .collect();
    if shapes {
        items.push(FlyoutItem {
            command: PLACE_IMAGE,
            params: json!({}),
            label: "Place image…".to_owned(),
            swatch: None,
            selected: false,
            icon: Some(ToolIcon::Image),
        });
    }
    items
}
