//! The toolbar's model: which tools a board and a design file offer and in which groups, which tool
//! is active, what each flyout holds, and the templates and tables a board can place. The view in
//! `crates/app/src/board/toolbar/` draws what these return and runs the `command` of a button or
//! a flyout item, with its params, through `BoardView::choose`: the tests here run the same
//! commands with the same params.
//!
//! The groups, in the order the toolbar shows them, are the spec's lists ("Toolbar and context
//! bar") with every kind of layer the engine's tools make on that kind of file: the user asked
//! for the frame and the rest of FigCraft's design tools on a board too, where the engine makes
//! them as on a design file. A group marked ^ shows one of its tools, the one in use or used
//! last, with a flyout that lists them all ([`Flyout::Tools`]), as FigCraft's own toolbar does;
//! the others show each of their tools.
//!
//! | File | Groups |
//! |---|---|
//! | board | `move` (Move, Hand), `ink` (Marker, Highlighter, Eraser), `sticky` (Sticky), `shape` (Shapes), `connector` (Connector), `text` ^ (Text, Text on a path), `frame` ^ (Frame, Section, Slice), `geometry` ^ (Rectangle, Line, Arrow, Ellipse, Polygon, Star; Place image…), `pen` ^ (Pen, Pencil), `stamp` (Stamp) |
//! | design | `move` (Move, Scale, Hand), `frame` ^ (Frame, Section, Slice), `shape` ^ (Rectangle, Line, Arrow, Ellipse, Polygon, Star; Place image…), `pen` ^ (Pen, Pencil), `text` ^ (Text, Text on a path) |
//!
//! A design file has no board tools: the engine refuses them there ("this is for boards"). No
//! other tool is offered: [`crate::policy`] hides the comment tool, the shape builder and the
//! colour picker.

use figcraft_engine::ToolId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{BoardDoc, BoardError};

mod flyout;
mod groups;

pub use flyout::{PLACE_IMAGE, flyout};
pub use groups::{active, groups, shown_tool};

/// The picture of a tool button. The view turns each into an icon of its own set, so this module
/// names pictures without depending on one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolIcon {
    Move,
    Hand,
    Scale,
    Frame,
    Section,
    Slice,
    Rectangle,
    Line,
    Arrow,
    Ellipse,
    Polygon,
    Star,
    Pen,
    Pencil,
    Text,
    Marker,
    Highlighter,
    Eraser,
    Sticky,
    Shapes,
    Connector,
    Stamp,
    TextOnPath,
    /// Place image…, which asks for an image file and places it.
    Image,
}

/// One tool button of the toolbar.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolButton {
    /// The engine command a click runs, without params: `tool.` and the tool's id.
    pub command: &'static str,
    /// The tool the command chooses. The button is the active one while [`active`] is this tool.
    pub tool: ToolId,
    /// What the tooltip calls the tool: FigCraft's own label for it.
    pub label: &'static str,
    /// The tool's key in FigCraft's keymap, as a Mac menu writes it (`V`, `⇧S`). `None` for a
    /// tool without a key. The app's tooltip shows the key the keymap binds now instead.
    pub shortcut: Option<String>,
    pub icon: ToolIcon,
}

/// A flyout: the choices that open beside a tool button and set what the tool makes, or the
/// tools of its group. [`flyout`] lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flyout {
    /// The board colours of a new sticky note.
    StickyColors,
    /// The outlines of a new shape.
    Shapes,
    /// How a new connector runs.
    ConnectorStyles,
    /// The stamps.
    Stamps,
    /// The tools of the group with this key, which shows one of them at a time
    /// ([`shown_tool`]): picking one chooses it. The group of the Rectangle tool also lists Place
    /// image…, as Figma's shape tools end with Image.
    Tools(&'static str),
}

impl Flyout {
    /// What the view names the flyout by: the variant, or for a group's tools the group's key
    /// (`StickyColors`, `geometry`).
    pub fn name(self) -> String {
        match self {
            Flyout::Tools(group) => group.to_string(),
            other => format!("{other:?}"),
        }
    }
}

/// A run of tool buttons that sit together, and the flyout that belongs to them if they have one.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolGroup {
    /// What the group is: `move`, `ink`, `sticky`, `shape`, `connector`, `text`, `section`,
    /// `stamp`, `frame` or `pen`.
    pub key: &'static str,
    pub tools: Vec<ToolButton>,
    /// The flyout the group's tool opens, with the choices that set what the tool makes.
    pub flyout: Option<Flyout>,
}

/// One choice of a flyout. Picking it runs `command` with `params`. It has the fields of
/// `context_bar::ControlOption`, so the view can draw the two alike, and the picture of a tool.
#[derive(Clone, Debug, PartialEq)]
pub struct FlyoutItem {
    /// The command picking the item runs: the tool's own, with the choice as its one param. It
    /// chooses the tool as well as the choice.
    pub command: &'static str,
    pub params: Value,
    /// What the view calls the item: the colour's name (`yellow`), or the label of the shape, the
    /// style or the stamp.
    pub label: String,
    /// The colour to draw for a colour item, `None` for the others.
    pub swatch: Option<[u8; 4]>,
    /// Whether the item is the choice the tool makes now, or for a group's tools the tool in use.
    pub selected: bool,
    /// The picture of a tool, `None` for the other choices.
    pub icon: Option<ToolIcon>,
}

/// A board template, as `board.templates` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Template {
    /// What [`place_template`] takes.
    pub id: String,
    pub name: String,
    /// One line about what is in it.
    pub description: String,
}

/// The templates a board offers, in the order the engine lists them (`board.templates`).
///
/// This runs an engine command like any other, so although it changes nothing in the document it
/// moves the engine's revision and adds to the session's journal each time. A view that asked on
/// every frame would never let the canvas go idle: it asks when the picker opens, and keeps the
/// list until the picker closes.
pub fn templates(board: &mut BoardDoc) -> Result<Vec<Template>, BoardError> {
    let listed = board.execute("board.templates", &json!({}))?;
    serde_json::from_value(listed)
        .map_err(|e| BoardError::Engine(format!("the list of templates can't be read: {e}")))
}

/// Places the template whose id is `id` (one of [`templates`]'s) in the middle of the view and
/// selects its layers: `board.template {name}` with no position, which the engine centres on the
/// view, wherever that is and at whatever zoom. It is one undo step.
pub fn place_template(board: &mut BoardDoc, id: &str) -> Result<(), BoardError> {
    board.execute("board.template", &json!({ "name": id }))?;
    Ok(())
}

/// Places a table of `rows` by `cols` cells in the middle of the view and selects it:
/// `board.table {rows, columns}` with no position, which the engine centres on the view. It is one
/// undo step. The engine takes 1 to 50 rows and 1 to 20 columns, and refuses any other count with
/// an error that says so.
pub fn place_table(board: &mut BoardDoc, rows: u32, cols: u32) -> Result<(), BoardError> {
    board.execute("board.table", &json!({ "rows": rows, "columns": cols }))?;
    Ok(())
}

#[cfg(test)]
mod tests;
