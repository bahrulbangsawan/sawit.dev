//! The tool groups of the toolbar of each kind of file, the tool a group shows, and the tool in
//! use.

use figcraft_engine::keymap::{self, Platform};
use figcraft_engine::{Session, ToolId};

use super::{Flyout, ToolButton, ToolGroup, ToolIcon};
use crate::DocKind;

/// A tool as the groups list it: what a [`ToolButton`] is before its label and shortcut are looked
/// up.
#[derive(Clone, Copy)]
struct ToolDef {
    command: &'static str,
    tool: ToolId,
    icon: ToolIcon,
}

impl ToolDef {
    const fn new(command: &'static str, tool: ToolId, icon: ToolIcon) -> Self {
        Self {
            command,
            tool,
            icon,
        }
    }
}

const MOVE: ToolDef = ToolDef::new("tool.move", ToolId::Move, ToolIcon::Move);
const HAND: ToolDef = ToolDef::new("tool.hand", ToolId::Hand, ToolIcon::Hand);
const SCALE: ToolDef = ToolDef::new("tool.scale", ToolId::Scale, ToolIcon::Scale);
const FRAME: ToolDef = ToolDef::new("tool.frame", ToolId::Frame, ToolIcon::Frame);
const SECTION: ToolDef = ToolDef::new("tool.section", ToolId::Section, ToolIcon::Section);
const SLICE: ToolDef = ToolDef::new("tool.slice", ToolId::Slice, ToolIcon::Slice);
const RECTANGLE: ToolDef = ToolDef::new("tool.rectangle", ToolId::Rectangle, ToolIcon::Rectangle);
const LINE: ToolDef = ToolDef::new("tool.line", ToolId::Line, ToolIcon::Line);
const ARROW: ToolDef = ToolDef::new("tool.arrow", ToolId::Arrow, ToolIcon::Arrow);
const ELLIPSE: ToolDef = ToolDef::new("tool.ellipse", ToolId::Ellipse, ToolIcon::Ellipse);
const POLYGON: ToolDef = ToolDef::new("tool.polygon", ToolId::Polygon, ToolIcon::Polygon);
const STAR: ToolDef = ToolDef::new("tool.star", ToolId::Star, ToolIcon::Star);
const PEN: ToolDef = ToolDef::new("tool.pen", ToolId::Pen, ToolIcon::Pen);
const PENCIL: ToolDef = ToolDef::new("tool.pencil", ToolId::Pencil, ToolIcon::Pencil);
const TEXT: ToolDef = ToolDef::new("tool.text", ToolId::Text, ToolIcon::Text);
const MARKER: ToolDef = ToolDef::new("tool.marker", ToolId::Marker, ToolIcon::Marker);
const HIGHLIGHTER: ToolDef = ToolDef::new(
    "tool.highlighter",
    ToolId::Highlighter,
    ToolIcon::Highlighter,
);
const ERASER: ToolDef = ToolDef::new("tool.eraser", ToolId::Eraser, ToolIcon::Eraser);
const STICKY: ToolDef = ToolDef::new("tool.sticky", ToolId::Sticky, ToolIcon::Sticky);
const SHAPES: ToolDef = ToolDef::new("tool.shape", ToolId::BoardShape, ToolIcon::Shapes);
const CONNECTOR: ToolDef = ToolDef::new("tool.connector", ToolId::Connector, ToolIcon::Connector);
const STAMP: ToolDef = ToolDef::new("tool.stamp", ToolId::Stamp, ToolIcon::Stamp);
const TEXT_ON_PATH: ToolDef =
    ToolDef::new("tool.textOnPath", ToolId::TextOnPath, ToolIcon::TextOnPath);

/// The button for `def`. Its label is FigCraft's own for the tool, and its shortcut is the one a
/// Mac menu shows for the command: the first of its keys, with ⇧ for Shift.
fn button(def: ToolDef) -> ToolButton {
    ToolButton {
        command: def.command,
        tool: def.tool,
        label: def.tool.label(),
        shortcut: crate::shortcuts::primary(def.command)
            .map(|keys| keymap::display(keys, Platform::Mac)),
        icon: def.icon,
    }
}

/// A group of `tools`, with `flyout` if the group has one.
fn group(key: &'static str, tools: &[ToolDef], flyout: Option<Flyout>) -> ToolGroup {
    ToolGroup {
        key,
        tools: tools.iter().copied().map(button).collect(),
        flyout,
    }
}

/// A group of `tools` under `key` that shows one of them at a time, with a flyout that lists them
/// ([`Flyout::Tools`]).
fn tools(key: &'static str, tools: &[ToolDef]) -> ToolGroup {
    group(key, tools, Some(Flyout::Tools(key)))
}

/// The shape tools, in FigCraft's order.
const SHAPE_TOOLS: [ToolDef; 6] = [RECTANGLE, LINE, ARROW, ELLIPSE, POLYGON, STAR];

/// The tool groups of the toolbar of a file of `kind`, in the order the toolbar shows them. A
/// board and a design file have tools of their own: the module's docs list both.
pub fn groups(kind: DocKind) -> Vec<ToolGroup> {
    match kind {
        DocKind::Board => vec![
            group("move", &[MOVE, HAND], None),
            group("ink", &[MARKER, HIGHLIGHTER, ERASER], None),
            group("sticky", &[STICKY], Some(Flyout::StickyColors)),
            group("shape", &[SHAPES], Some(Flyout::Shapes)),
            group("connector", &[CONNECTOR], Some(Flyout::ConnectorStyles)),
            tools("text", &[TEXT, TEXT_ON_PATH]),
            tools("frame", &[FRAME, SECTION, SLICE]),
            tools("geometry", &SHAPE_TOOLS),
            tools("pen", &[PEN, PENCIL]),
            group("stamp", &[STAMP], Some(Flyout::Stamps)),
        ],
        DocKind::Design => vec![
            group("move", &[MOVE, SCALE, HAND], None),
            tools("frame", &[FRAME, SECTION, SLICE]),
            tools("shape", &SHAPE_TOOLS),
            tools("pen", &[PEN, PENCIL]),
            tools("text", &[TEXT, TEXT_ON_PATH]),
        ],
    }
}

/// The tool the button of `group` shows when its flyout lists its tools ([`Flyout::Tools`]):
/// the tool in use when it is one of them, else the one of them used last (`last`, which the view
/// remembers), else the first. Any other group shows each of its tools.
pub fn shown_tool(group: &ToolGroup, active: ToolId, last: Option<ToolId>) -> &ToolButton {
    let find = |tool: ToolId| group.tools.iter().find(|button| button.tool == tool);
    find(active)
        .or_else(|| last.and_then(find))
        .or(group.tools.first())
        .expect("a toolbar group has a tool")
}

/// The tool that is chosen now, which the canvas's pointer events go to. It is the session's own
/// tool, so one chosen by a key (Q, B, X…) is this as much as one chosen by a button. The view
/// marks the button of this tool as selected on every frame.
pub fn active(s: &Session) -> ToolId {
    s.tool.tool
}
