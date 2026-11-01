//! The toolbar's model: which tools a board and a design file offer and in which groups, which tool
//! is active, what each flyout holds, and the templates and tables a board can place. The view in
//! `crates/app/src/board/toolbar.rs` draws what these return and runs the `command` of a button or
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

use figcraft_doc::{BOARD_COLORS, BoardShape, Routing, Stamp};
use figcraft_engine::keymap::{self, Platform};
use figcraft_engine::{Session, ToolId};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{BoardDoc, BoardError, DocKind};

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

/// The command of Place image…, the last item of the shape tools' flyout: Sawit asks for the
/// image file (`crate::policy::UI_COMMANDS`).
pub const PLACE_IMAGE: &str = "file.placeImage";

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
    let Some(group) = groups(kind).into_iter().find(|group| group.key == key) else {
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
mod tests {
    use figcraft_doc::{BoardItem, Document};
    use figcraft_engine::board_templates::TEMPLATES;
    use figcraft_geom::Rect;

    use super::*;
    use crate::input::{KeyOutcome, Mods, route_key};
    use crate::policy::is_hidden;

    /// A new, empty board.
    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// A design file with nothing on its page.
    fn design() -> BoardDoc {
        BoardDoc {
            session: Session::with_document(Document::new("Design")),
        }
    }

    /// Both kinds of file, with how many tool buttons the toolbar of each has. The tests that go
    /// over every button check the count too, so that an empty toolbar fails them.
    const KINDS: [(DocKind, usize); 2] = [(DocKind::Board, 22), (DocKind::Design, 16)];

    /// Every tool button of the toolbar of a file of `kind`, in order.
    fn buttons(kind: DocKind) -> Vec<ToolButton> {
        groups(kind)
            .into_iter()
            .flat_map(|group| group.tools)
            .collect()
    }

    /// The commands of those buttons.
    fn commands(kind: DocKind) -> Vec<&'static str> {
        buttons(kind).iter().map(|button| button.command).collect()
    }

    /// The shortcut of the button for `command` in the toolbar of a file of `kind`.
    fn shortcut(kind: DocKind, command: &str) -> Option<String> {
        let found = buttons(kind)
            .into_iter()
            .find(|button| button.command == command);
        found.unwrap().shortcut
    }

    /// The labels of the items that are selected.
    fn selected(items: &[FlyoutItem]) -> Vec<&str> {
        items
            .iter()
            .filter(|item| item.selected)
            .map(|item| item.label.as_str())
            .collect()
    }

    fn page_is_empty(b: &BoardDoc) -> bool {
        let page = b.session.page().unwrap();
        b.doc().unwrap().node(page).unwrap().children.is_empty()
    }

    /// The part of the page that the view shows, in page coordinates.
    fn viewport(b: &BoardDoc) -> Rect {
        let view = &b.session.ds().unwrap().view;
        let (x, y) = view.origin();
        Rect::new(x, y, x + view.vw / view.zoom, y + view.vh / view.zoom)
    }

    /// The box around everything on the page, in page coordinates.
    fn content(b: &BoardDoc) -> Rect {
        let page = b.session.page().unwrap();
        b.doc().unwrap().page_content_bounds(page).unwrap()
    }

    /// Whether `inner` lies inside `outer`.
    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x0 >= outer.x0 && inner.y0 >= outer.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
    }

    #[test]
    fn groups_differ_by_kind() {
        let on_board = commands(DocKind::Board);
        let on_design = commands(DocKind::Design);
        assert!(on_board.contains(&"tool.sticky"));
        assert!(on_design.contains(&"tool.frame"));
        assert!(!on_design.contains(&"tool.sticky"));
        // The frame and the design tools are on a board too, where the engine makes them.
        for command in on_design.iter().filter(|command| **command != "tool.scale") {
            assert!(on_board.contains(command), "{command} isn't on a board");
        }
        // Comments are not part of the canvas.
        assert!(!on_board.contains(&"tool.comment"));
        assert!(!on_design.contains(&"tool.comment"));
    }

    #[test]
    fn groups_follow_the_specs_lists() {
        let layout = |kind| {
            groups(kind)
                .into_iter()
                .map(|group| {
                    let tools: Vec<_> = group.tools.iter().map(|tool| tool.command).collect();
                    (group.key, tools, group.flyout)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            layout(DocKind::Board),
            [
                ("move", vec!["tool.move", "tool.hand"], None),
                (
                    "ink",
                    vec!["tool.marker", "tool.highlighter", "tool.eraser"],
                    None
                ),
                ("sticky", vec!["tool.sticky"], Some(Flyout::StickyColors)),
                ("shape", vec!["tool.shape"], Some(Flyout::Shapes)),
                (
                    "connector",
                    vec!["tool.connector"],
                    Some(Flyout::ConnectorStyles)
                ),
                (
                    "text",
                    vec!["tool.text", "tool.textOnPath"],
                    Some(Flyout::Tools("text"))
                ),
                (
                    "frame",
                    vec!["tool.frame", "tool.section", "tool.slice"],
                    Some(Flyout::Tools("frame"))
                ),
                (
                    "geometry",
                    SHAPES_TOOLS.to_vec(),
                    Some(Flyout::Tools("geometry"))
                ),
                (
                    "pen",
                    vec!["tool.pen", "tool.pencil"],
                    Some(Flyout::Tools("pen"))
                ),
                ("stamp", vec!["tool.stamp"], Some(Flyout::Stamps)),
            ]
        );
        assert_eq!(
            layout(DocKind::Design),
            [
                ("move", vec!["tool.move", "tool.scale", "tool.hand"], None),
                (
                    "frame",
                    vec!["tool.frame", "tool.section", "tool.slice"],
                    Some(Flyout::Tools("frame"))
                ),
                ("shape", SHAPES_TOOLS.to_vec(), Some(Flyout::Tools("shape"))),
                (
                    "pen",
                    vec!["tool.pen", "tool.pencil"],
                    Some(Flyout::Tools("pen"))
                ),
                (
                    "text",
                    vec!["tool.text", "tool.textOnPath"],
                    Some(Flyout::Tools("text"))
                ),
            ]
        );
    }

    /// The shape tools' commands, in the toolbar's order.
    const SHAPES_TOOLS: [&str; 6] = [
        "tool.rectangle",
        "tool.line",
        "tool.arrow",
        "tool.ellipse",
        "tool.polygon",
        "tool.star",
    ];

    /// A file of `kind` with nothing on its page.
    fn empty(kind: DocKind) -> BoardDoc {
        match kind {
            DocKind::Board => board(),
            DocKind::Design => design(),
        }
    }

    /// Drags on the page of `doc` from `from` to `to`, at 100%, with the tool in use.
    fn drag(doc: &mut BoardDoc, from: (f64, f64), to: (f64, f64)) {
        use figcraft_engine::{PointerEvent, PointerKind};
        let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
        for (kind, (x, y)) in [
            (PointerKind::Down, from),
            (PointerKind::Move, mid),
            (PointerKind::Move, to),
            (PointerKind::Up, to),
        ] {
            crate::input::pointer(doc, &PointerEvent::at(kind, x, y, 1.0)).unwrap();
        }
    }

    /// How many layers the page of `doc` holds, however deep.
    fn layer_count(doc: &BoardDoc) -> usize {
        let page = doc.session.page().unwrap();
        doc.doc().unwrap().subtree(page).len()
    }

    /// The user's ask: every kind of layer the engine's tools make is on the toolbar, on a board
    /// as on a design file, and none of them only errors there. A drag with each drawing tool
    /// makes a layer on both kinds of file; the pen makes a path of two clicks; text on a path
    /// goes on a rectangle's outline.
    #[test]
    fn every_drawing_tool_makes_a_layer_on_both_kinds_of_file() {
        for (kind, _) in KINDS {
            let shown = commands(kind);
            for command in [
                "tool.frame",
                "tool.section",
                "tool.slice",
                "tool.rectangle",
                "tool.line",
                "tool.arrow",
                "tool.ellipse",
                "tool.polygon",
                "tool.star",
                "tool.pencil",
            ] {
                assert!(shown.contains(&command), "{kind:?}: {command} isn't shown");
                let mut doc = empty(kind);
                doc.execute(command, &json!({})).unwrap();
                drag(&mut doc, (10.0, 10.0), (120.0, 90.0));
                assert!(layer_count(&doc) > 1, "{kind:?}: {command} made nothing");
            }
            // The pen: two clicks and Enter make a path.
            let mut doc = empty(kind);
            doc.execute("tool.pen", &json!({})).unwrap();
            drag(&mut doc, (10.0, 10.0), (10.0, 10.0));
            drag(&mut doc, (120.0, 90.0), (120.0, 90.0));
            let _ = crate::input::route_key(&mut doc, "Enter", Mods::default());
            assert!(layer_count(&doc) > 1, "{kind:?}: the pen made nothing");
            // Text on a path: a click on a rectangle's outline.
            let mut doc = empty(kind);
            doc.execute("tool.rectangle", &json!({})).unwrap();
            drag(&mut doc, (0.0, 0.0), (200.0, 100.0));
            doc.execute("tool.textOnPath", &json!({})).unwrap();
            drag(&mut doc, (100.0, 0.0), (100.0, 0.0));
            // The text takes the rectangle's place, along its outline.
            let page = doc.session.page().unwrap();
            let on_path = doc.doc().unwrap().subtree(page).into_iter().any(|id| {
                let doc = doc.doc().unwrap();
                doc.node(id)
                    .and_then(|node| node.kind.text())
                    .is_some_and(|text| text.path.is_some())
            });
            assert!(on_path, "{kind:?}: no text on the path");
        }
    }

    #[test]
    fn a_tools_flyout_lists_its_groups_tools_and_marks_the_one_in_use() {
        for (kind, _) in KINDS {
            let mut doc = empty(kind);
            for group in groups(kind) {
                let Some(Flyout::Tools(key)) = group.flyout else {
                    continue;
                };
                assert_eq!(key, group.key);
                let items = flyout(&doc.session, Flyout::Tools(key));
                let tools: Vec<&str> = group.tools.iter().map(|tool| tool.command).collect();
                let listed: Vec<&str> = items.iter().map(|item| item.command).collect();
                let shapes = tools.contains(&"tool.rectangle");
                let mut expected = tools.clone();
                if shapes {
                    expected.push(PLACE_IMAGE);
                }
                assert_eq!(listed, expected, "{kind:?} {key}");
                assert!(items.iter().all(|item| item.icon.is_some()), "{key}");
                for item in items.iter().filter(|item| item.command != PLACE_IMAGE) {
                    // Picking a tool chooses it, and it is the one marked.
                    doc.execute("tool.move", &json!({})).unwrap();
                    doc.execute(item.command, &item.params).unwrap();
                    let after = flyout(&doc.session, Flyout::Tools(key));
                    assert_eq!(selected(&after), [item.label.as_str()], "{key}");
                }
            }
        }
        // Place image… is Sawit's to carry out: it asks for the file.
        assert!(crate::policy::is_ui(PLACE_IMAGE));
    }

    #[test]
    fn a_group_shows_the_tool_in_use_else_the_one_used_last() {
        let groups = groups(DocKind::Design);
        let shapes = groups.iter().find(|group| group.key == "shape").unwrap();
        // Nothing of the group used yet: its first tool.
        let shown = |active, last| shown_tool(shapes, active, last).tool;
        assert_eq!(shown(ToolId::Move, None), ToolId::Rectangle);
        // The tool in use, when it is one of them.
        assert_eq!(shown(ToolId::Star, None), ToolId::Star);
        assert_eq!(shown(ToolId::Star, Some(ToolId::Ellipse)), ToolId::Star);
        // Else the one used last.
        assert_eq!(shown(ToolId::Move, Some(ToolId::Ellipse)), ToolId::Ellipse);
        // A tool of another group used last counts for nothing here.
        assert_eq!(shown(ToolId::Move, Some(ToolId::Pen)), ToolId::Rectangle);
    }

    #[test]
    fn every_tool_button_chooses_its_tool() {
        for (kind, count) in KINDS {
            let mut doc = match kind {
                DocKind::Board => board(),
                DocKind::Design => design(),
            };
            assert_eq!(buttons(kind).len(), count, "{kind:?}");
            for button in buttons(kind) {
                assert_eq!(button.command, format!("tool.{}", button.tool.id()));
                assert!(!button.label.is_empty(), "{}", button.command);
                // From another tool, so that the click is what makes this one active.
                let other = if button.tool == ToolId::Hand {
                    "tool.move"
                } else {
                    "tool.hand"
                };
                doc.execute(other, &json!({})).unwrap();
                assert_ne!(active(&doc.session), button.tool);
                if let Err(e) = doc.execute(button.command, &json!({})) {
                    panic!("{kind:?} {}: {e}", button.command);
                }
                assert_eq!(active(&doc.session), button.tool, "{}", button.command);
            }
        }
    }

    #[test]
    fn each_tool_has_an_icon_of_its_own() {
        // Section and Text are in both toolbars, with the same icon in both.
        let all: Vec<ToolButton> = buttons(DocKind::Board)
            .into_iter()
            .chain(buttons(DocKind::Design))
            .collect();
        assert_eq!(
            all.len(),
            KINDS.iter().map(|(_, count)| count).sum::<usize>()
        );
        for a in &all {
            for b in &all {
                let same = (a.tool == b.tool, a.icon == b.icon);
                assert_eq!(same.0, same.1, "{} and {}", a.command, b.command);
            }
        }
    }

    #[test]
    fn no_tool_is_one_the_policy_hides() {
        // A hidden command is dead on the keyboard too, so its button would be the only way in.
        for (kind, count) in KINDS {
            let commands = commands(kind);
            assert_eq!(commands.len(), count, "{kind:?}");
            for command in commands {
                assert!(!is_hidden(command), "{command}");
            }
        }
    }

    #[test]
    fn shortcuts_use_mac_glyphs() {
        let (on_board, on_design) = (DocKind::Board, DocKind::Design);
        assert_eq!(shortcut(on_board, "tool.move").as_deref(), Some("V"));
        assert_eq!(shortcut(on_board, "tool.section").as_deref(), Some("⇧S"));
        assert_eq!(shortcut(on_design, "tool.section").as_deref(), Some("⇧S"));
        // The other keys with Shift.
        assert_eq!(shortcut(on_design, "tool.arrow").as_deref(), Some("⇧L"));
        assert_eq!(shortcut(on_design, "tool.pencil").as_deref(), Some("⇧P"));
        // A tool with two keys shows the first, as the menu does.
        assert_eq!(shortcut(on_design, "tool.frame").as_deref(), Some("F"));
        // The board tools.
        for (command, key) in [
            ("tool.hand", "H"),
            ("tool.marker", "M"),
            ("tool.highlighter", "U"),
            ("tool.eraser", "E"),
            ("tool.sticky", "Q"),
            ("tool.shape", "B"),
            ("tool.connector", "X"),
            ("tool.text", "T"),
            ("tool.stamp", "W"),
        ] {
            let shown = shortcut(on_board, command);
            assert_eq!(shown.as_deref(), Some(key), "{command}");
        }
        // Polygon and Star have Sawit's keys, ^ and * (FigCraft has none for them).
        assert_eq!(shortcut(on_design, "tool.polygon").as_deref(), Some("⇧6"));
        assert_eq!(shortcut(on_board, "tool.star").as_deref(), Some("⇧8"));
        // The design tools on a board have their keys there too.
        for (command, key) in [
            ("tool.frame", "F"),
            ("tool.rectangle", "R"),
            ("tool.ellipse", "O"),
            ("tool.line", "L"),
            ("tool.arrow", "⇧L"),
            ("tool.pen", "P"),
            ("tool.slice", "S"),
        ] {
            assert_eq!(
                shortcut(on_board, command).as_deref(),
                Some(key),
                "{command}"
            );
        }
        // Text on a path has none in FigCraft: Sawit gives it ⇧T, beside Text's T.
        assert_eq!(shortcut(on_board, "tool.textOnPath").as_deref(), Some("⇧T"));
    }

    #[test]
    fn active_tool_follows_session() {
        let mut b = board();
        assert_eq!(active(&b.session), ToolId::Move);
        // The keys of Sticky, Shapes and Connector, then Move.
        for (key, tool) in [
            ("q", ToolId::Sticky),
            ("b", ToolId::BoardShape),
            ("x", ToolId::Connector),
            ("v", ToolId::Move),
        ] {
            assert_eq!(
                route_key(&mut b, key, Mods::default()),
                Ok(KeyOutcome::Handled)
            );
            assert_eq!(active(&b.session), tool, "{key}");
        }
    }

    #[test]
    fn sticky_flyout_marks_current_colour() {
        let mut b = board();
        let items = flyout(&b.session, Flyout::StickyColors);
        assert_eq!(items.len(), 8);
        // A new board starts on the first colour.
        assert_eq!(selected(&items), ["yellow"]);

        b.execute("tool.sticky", &json!({"color": "blue"})).unwrap();
        let items = flyout(&b.session, Flyout::StickyColors);
        assert_eq!(items.len(), 8);
        assert_eq!(selected(&items), ["blue"]);
        // Each item sets its colour by name, and shows it.
        let blue = items.iter().find(|item| item.label == "blue").unwrap();
        assert_eq!(blue.command, "tool.sticky");
        assert_eq!(blue.params, json!({"color": "blue"}));
        assert_eq!(blue.swatch, Some([0xAC, 0xD3, 0xFA, 0xFF]));

        // A colour that is none of the eight marks none of them.
        b.execute("tool.sticky", &json!({"color": "#336699"}))
            .unwrap();
        assert_eq!(selected(&flyout(&b.session, Flyout::StickyColors)).len(), 0);
    }

    #[test]
    fn every_flyout_item_runs() {
        let mut b = board();
        let board_groups = groups(DocKind::Board);
        for (f, count) in [
            (Flyout::StickyColors, 8),
            (Flyout::Shapes, 7),
            (Flyout::ConnectorStyles, 3),
            (Flyout::Stamps, 6),
        ] {
            // The tool the flyout belongs to.
            let group = board_groups.iter().find(|group| group.flyout == Some(f));
            let tool = group.and_then(|group| group.tools.first()).unwrap().tool;
            let items = flyout(&b.session, f);
            assert_eq!(items.len(), count, "{f:?}");
            for item in &items {
                // From another tool, so that the item is what makes this one active.
                b.execute("tool.move", &json!({})).unwrap();
                if let Err(e) = b.execute(item.command, &item.params) {
                    panic!("{f:?} {}: {e}", item.label);
                }
                assert_eq!(active(&b.session), tool, "{f:?} {}", item.label);
                // It is the choice now, and the only one.
                let after = flyout(&b.session, f);
                assert_eq!(selected(&after), [item.label.as_str()], "{f:?}");
            }
        }
    }

    #[test]
    fn template_is_one_undo_step() {
        let mut b = board();
        // Zoomed far out, on a part of the page far from its origin: the template goes where the
        // view is, in view, not to the origin.
        let view = json!({"zoom": 0.05, "cx": 20000, "cy": 20000, "vw": 800, "vh": 600});
        b.execute("view.set", &view).unwrap();
        let first = templates(&mut b).unwrap().into_iter().next().unwrap();
        assert!(page_is_empty(&b));

        place_template(&mut b, &first.id).unwrap();
        assert!(!page_is_empty(&b));
        let (bounds, shown) = (content(&b), viewport(&b));
        assert!(inside(bounds, shown), "{bounds:?} is not inside {shown:?}");

        b.execute("edit.undo", &json!({})).unwrap();
        assert!(page_is_empty(&b));
    }

    #[test]
    fn every_template_is_listed_and_places() {
        let listed = templates(&mut board()).unwrap();
        let ids: Vec<&str> = listed.iter().map(|t| t.id.as_str()).collect();
        let engines: Vec<&str> = TEMPLATES.iter().map(|t| t.id).collect();
        assert_eq!(ids, engines);
        for template in &listed {
            assert!(!template.name.is_empty(), "{template:?}");
            assert!(!template.description.is_empty(), "{template:?}");
            let mut b = board();
            if let Err(e) = place_template(&mut b, &template.id) {
                panic!("{}: {e}", template.id);
            }
            assert!(!page_is_empty(&b), "{} placed nothing", template.id);
        }
    }

    #[test]
    fn an_unknown_template_is_refused_and_places_nothing() {
        let mut b = board();
        assert!(matches!(
            place_template(&mut b, "no such template"),
            Err(BoardError::Engine(_))
        ));
        assert!(page_is_empty(&b));
    }

    #[test]
    fn table_places_rows_and_columns() {
        let mut b = board();
        place_table(&mut b, 2, 3).unwrap();
        let doc = b.doc().unwrap();
        let page = doc.node(b.session.page().unwrap()).unwrap();
        let [table] = page.children[..] else {
            panic!("one new layer on the page: {:?}", page.children);
        };
        let table = doc.node(table).unwrap();
        assert!(matches!(table.board, Some(BoardItem::Table)));
        // The rows lay out one under another; the lines over them stay where they are.
        let rows: Vec<_> = table
            .children
            .iter()
            .filter_map(|id| doc.node(*id))
            .filter(|row| !row.child_layout.absolute)
            .collect();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.children.len(), 3);
        }
        assert!(inside(content(&b), viewport(&b)));
    }

    #[test]
    fn a_table_size_the_engine_refuses_is_an_error() {
        let mut b = board();
        for (rows, cols) in [(0, 3), (3, 0), (51, 3), (3, 21)] {
            assert!(
                matches!(place_table(&mut b, rows, cols), Err(BoardError::Engine(_))),
                "{rows} x {cols}"
            );
        }
        assert!(page_is_empty(&b));
    }
}
