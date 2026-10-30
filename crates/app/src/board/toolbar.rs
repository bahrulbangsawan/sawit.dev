//! The floating toolbar at the bottom centre of the canvas (spec D11, "Toolbar and context bar"):
//! the tools of a board or of a design file in their groups (`sawit_board::toolbar::groups`), a
//! chevron beside each board tool with choices that opens its flyout, and on a board a last button
//! for the Templates & table picker. A button or a choice runs its command, with its params,
//! through [`BoardView::choose`], as the model's tests run them.
//!
//! The button of the session's tool is marked on every frame, so a tool chosen by a key shows at
//! once (Review Focus 3). A flyout or the picker that is open closes when the file on show turns
//! from a board into a design file or back, and when the UI is hidden
//! ([`BoardView::settle_chrome`]).

use figcraft_engine::{ToolId, guard};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::DocKind;
use sawit_board::toolbar::{
    self as model, Flyout, FlyoutItem, Template, ToolButton, ToolGroup, ToolIcon,
};
use serde_json::json;

use super::BoardView;
use super::top_bar::with_keys;

/// The toolbar's height.
pub(super) const HEIGHT: f32 = 44.;
/// How far the toolbar floats off the canvas's bottom edge.
pub(super) const BOTTOM: f32 = 16.;
/// A tool button's side.
const BUTTON: f32 = 32.;
/// The width of the chevron beside a tool that opens its flyout.
const CHEVRON: f32 = 16.;
/// The side of a colour swatch's button, in the flyouts and the context bar.
pub(super) const SWATCH: f32 = 24.;
/// The picker's table grid: up to this many rows, and as many columns.
const GRID: u32 = 6;
/// The side of a cell of that grid.
const CELL: f32 = 18.;

/// The picture of the chevron that opens a flyout.
pub(super) const CHEVRON_ICON: Lucide = Lucide::ChevronUp;
/// The picture of the Templates & table button.
pub(super) const PICKER_ICON: Lucide = Lucide::LayoutTemplate;
/// The mark of the choice a flyout's tool makes now.
pub(super) const CHECK_ICON: Lucide = Lucide::Check;

/// The Templates & table picker while it is open: the templates the engine listed when it opened,
/// kept until it closes rather than asked for on every frame (ruling P3-C14), and the cell of the
/// table grid under the pointer, as rows and columns.
#[derive(Clone, Debug, Default)]
pub(super) struct Picker {
    pub(super) templates: Vec<Template>,
    pub(super) hover: Option<(u32, u32)>,
}

/// The Lucide icon that pictures a tool: the nearest one Sawit's set has.
pub(super) fn tool_icon(icon: ToolIcon) -> Lucide {
    match icon {
        ToolIcon::Move => Lucide::MousePointer2,
        ToolIcon::Hand => Lucide::Hand,
        ToolIcon::Scale => Lucide::Scaling,
        ToolIcon::Frame => Lucide::Frame,
        ToolIcon::Section => Lucide::SquareDashed,
        ToolIcon::Slice => Lucide::Slice,
        ToolIcon::Rectangle => Lucide::Square,
        ToolIcon::Line => Lucide::Slash,
        ToolIcon::Arrow => Lucide::ArrowUpRight,
        ToolIcon::Ellipse => Lucide::Circle,
        ToolIcon::Polygon => Lucide::Triangle,
        ToolIcon::Star => Lucide::Star,
        ToolIcon::Pen => Lucide::PenTool,
        ToolIcon::Pencil => Lucide::Pencil,
        ToolIcon::Text => Lucide::Type,
        ToolIcon::Marker => Lucide::PenLine,
        ToolIcon::Highlighter => Lucide::Highlighter,
        ToolIcon::Eraser => Lucide::Eraser,
        ToolIcon::Sticky => Lucide::StickyNote,
        ToolIcon::Shapes => Lucide::Shapes,
        ToolIcon::Connector => Lucide::Spline,
        ToolIcon::Stamp => Lucide::Stamp,
    }
}

impl BoardView {
    /// The picker opens: the engine lists the templates now, once, for as long as it stays open
    /// (ruling P3-C14), and a flyout that was open closes. When the list can't be had, the error
    /// shows as a toast and the picker offers its table alone.
    fn open_picker(&mut self, cx: &mut Context<Self>) {
        self.open_flyout = None;
        let templates = self.run(model::templates, cx).unwrap_or_default();
        self.picker = Some(Picker {
            templates,
            hover: None,
        });
    }

    /// Places template `id`, one the picker lists, in the middle of the view, and closes the
    /// picker.
    pub(super) fn place_template(&mut self, id: &str, cx: &mut Context<Self>) {
        self.picker = None;
        self.run(|board| model::place_template(board, id), cx);
    }

    /// Places a table of `rows` by `cols` cells in the middle of the view, and closes the picker.
    pub(super) fn place_table(&mut self, rows: u32, cols: u32, cx: &mut Context<Self>) {
        self.picker = None;
        self.run(|board| model::place_table(board, rows, cols), cx);
    }
}

/// What the toolbar shows this frame, as the model gives it.
#[derive(Default)]
struct Shown {
    groups: Vec<ToolGroup>,
    /// The session's tool, whose button is marked.
    active: Option<ToolId>,
    /// The flyout that is open, and its items.
    flyout: Option<(Flyout, Vec<FlyoutItem>)>,
    /// The file is a board: the picker's button ends the toolbar.
    board: bool,
}

/// What the toolbar of `view`'s document shows. It is read on every frame, behind the engine's
/// guard: a panic in it costs that frame its toolbar, not the app (D15).
fn shown(view: &BoardView) -> Shown {
    let Some(board) = view.board() else {
        return Shown::default();
    };
    let kind = board.kind();
    let session = &board.session;
    guard::run("ui.toolbar", || {
        Ok(Shown {
            groups: model::groups(kind),
            active: Some(model::active(session)),
            flyout: view.open_flyout.map(|f| (f, model::flyout(session, f))),
            board: kind == DocKind::Board,
        })
    })
    .unwrap_or_default()
}

/// The toolbar of `view`'s canvas, centred [`BOTTOM`] off its bottom edge. It takes its clicks:
/// none reaches the canvas under it.
pub(super) fn render_toolbar(
    view: &mut BoardView,
    _: &mut Window,
    cx: &mut Context<BoardView>,
) -> impl IntoElement + use<> {
    let this = cx.entity().downgrade();
    let shown = shown(view);
    let picker = view.picker.clone();
    let theme = cx.theme();
    let line = theme.border;
    let mut bar = h_flex()
        .id("board-toolbar")
        .debug_selector(|| "board-toolbar".into())
        .occlude()
        .flex_shrink_0()
        .h(px(HEIGHT))
        .px(px(6.))
        .gap(px(2.))
        .rounded(px(12.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow_md();
    for (i, group) in shown.groups.iter().enumerate() {
        if i > 0 {
            bar = bar.child(divider(line));
        }
        for tool in &group.tools {
            bar = bar.child(tool_button(tool, shown.active == Some(tool.tool), &this));
        }
        if let Some(f) = group.flyout {
            let items = (shown.flyout.as_ref())
                .filter(|(open, _)| *open == f)
                .map(|(_, items)| items.clone());
            bar = bar.child(flyout(f, items, &this));
        }
    }
    if shown.board {
        bar = bar.child(divider(line)).child(picker_button(picker, &this));
    }
    // The row around the toolbar takes no clicks: only the toolbar does.
    h_flex()
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(BOTTOM))
        .justify_center()
        .child(bar)
}

/// The button of `tool`, marked while it is the `active` one. Its tooltip reads its label and
/// key ("Sticky note  Q").
fn tool_button(tool: &ToolButton, active: bool, view: &WeakEntity<BoardView>) -> Button {
    let command = tool.command;
    let view = view.clone();
    Button::new(command)
        .ghost()
        .icon(tool_icon(tool.icon))
        .selected(active)
        .tooltip(with_keys(tool.label, tool.shortcut.clone()))
        .debug_selector(|| command.to_owned())
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |view, cx| view.choose(command, json!({}), cx));
        })
}

/// What a flyout's chevron says it opens.
fn flyout_name(f: Flyout) -> &'static str {
    match f {
        Flyout::StickyColors => "Sticky note colors",
        Flyout::Shapes => "Shapes",
        Flyout::ConnectorStyles => "Connector styles",
        Flyout::Stamps => "Stamps",
    }
}

/// The chevron that opens flyout `f`, and the flyout above it, open while `items` are given.
fn flyout(f: Flyout, items: Option<Vec<FlyoutItem>>, view: &WeakEntity<BoardView>) -> Popover {
    let open = items.is_some();
    let items = items.unwrap_or_default();
    let chevron = Button::new(("board-flyout", f as usize))
        .ghost()
        .xsmall()
        .icon(CHEVRON_ICON)
        .w(px(CHEVRON))
        .h(px(BUTTON))
        .tooltip(flyout_name(f))
        .debug_selector(move || format!("board-flyout-{f:?}"));
    let (on_change, view) = (view.clone(), view.clone());
    Popover::new(("board-flyout-popover", f as usize))
        .anchor(Anchor::BottomCenter)
        .trigger(chevron)
        .open(open)
        .on_open_change(move |open, _, cx| {
            let _ = on_change.update(cx, |view, cx| {
                if *open {
                    view.open_flyout = Some(f);
                    view.picker = None;
                } else if view.open_flyout == Some(f) {
                    view.open_flyout = None;
                }
                cx.notify();
            });
        })
        .content(move |_, _, cx| flyout_items(&items, &view, cx))
}

/// A flyout's choices: a row of swatches for colours, else a list of labels with the one the
/// tool makes now checked. Picking one closes the flyout and runs its command with its params.
fn flyout_items(items: &[FlyoutItem], view: &WeakEntity<BoardView>, cx: &App) -> AnyElement {
    let hover = cx.theme().accent;
    let pick = |item: &FlyoutItem| {
        let (command, params, view) = (item.command, item.params.clone(), view.clone());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| {
                view.open_flyout = None;
                view.choose(command, params.clone(), cx);
            });
        }
    };
    let selector = |item: &FlyoutItem| {
        let label = item.label.clone();
        move || format!("board-flyout-item-{label}")
    };
    if items.iter().all(|item| item.swatch.is_some()) {
        let swatches = items.iter().enumerate().map(|(i, item)| {
            let rgba = item.swatch.unwrap_or_default();
            let id = ("board-flyout-item", i);
            swatch(id, rgba, &item.label, item.selected, pick(item)).debug_selector(selector(item))
        });
        return h_flex()
            .debug_selector(|| "board-flyout".into())
            .gap(px(4.))
            .children(swatches)
            .into_any_element();
    }
    let rows = items.iter().enumerate().map(|(i, item)| {
        h_flex()
            .id(("board-flyout-item", i))
            .debug_selector(selector(item))
            .h(px(28.))
            .px_2()
            .gap_2()
            .rounded(px(6.))
            .text_sm()
            .hover(move |row| row.bg(hover))
            .child(div().w(px(14.)).when(item.selected, |slot| {
                slot.child(Icon::new(CHECK_ICON).xsmall())
            }))
            .child(item.label.clone())
            .on_click(pick(item))
    });
    v_flex()
        .debug_selector(|| "board-flyout".into())
        .min_w(px(160.))
        .gap(px(2.))
        .children(rows)
        .into_any_element()
}

/// The button that opens the Templates & table picker, and the picker above it, open while
/// `picker` is given.
fn picker_button(picker: Option<Picker>, view: &WeakEntity<BoardView>) -> Popover {
    let open = picker.is_some();
    let Picker { templates, hover } = picker.unwrap_or_default();
    let button = Button::new("board-picker")
        .ghost()
        .icon(PICKER_ICON)
        .tooltip("Templates & table")
        .debug_selector(|| "board-picker".into());
    let (on_change, view) = (view.clone(), view.clone());
    Popover::new("board-picker-popover")
        .anchor(Anchor::BottomCenter)
        .trigger(button)
        .open(open)
        .on_open_change(move |open, _, cx| {
            let _ = on_change.update(cx, |view, cx| {
                if *open {
                    view.open_picker(cx);
                } else {
                    view.picker = None;
                    cx.notify();
                }
            });
        })
        .content(move |_, _, cx| picker_content(&templates, hover, &view, cx))
}

/// The picker: the board's templates, then a grid of 6 × 6 cells that places a table of as many
/// rows and columns as the cell under the pointer.
fn picker_content(
    templates: &[Template],
    hover: Option<(u32, u32)>,
    view: &WeakEntity<BoardView>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, line, hovered) = (theme.muted_foreground, theme.border, theme.accent);
    let heading = |text: String| div().text_xs().text_color(muted).child(text);
    let rows = templates.iter().enumerate().map(|(i, template)| {
        let (id, view) = (template.id.clone(), view.clone());
        v_flex()
            .id(("board-template", i))
            .px_2()
            .py_1()
            .rounded(px(6.))
            .hover(move |row| row.bg(hovered))
            .child(div().text_sm().child(template.name.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(template.description.clone()),
            )
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.place_template(&id, cx));
            })
    });
    let table = match hover {
        Some((rows, cols)) => format!("Table  {rows} × {cols}"),
        None => "Table".to_owned(),
    };
    v_flex()
        .id("board-picker-content")
        .debug_selector(|| "board-picker-content".into())
        .w(px(260.))
        .gap_2()
        .child(heading("Templates".to_owned()))
        .child(v_flex().gap(px(2.)).children(rows))
        .child(div().h(px(1.)).bg(line))
        .child(heading(table))
        .child(table_grid(hover, view, cx))
}

/// The picker's grid: the cells up to the one under the pointer light up, and a click places a
/// table of that many rows and columns.
fn table_grid(hover: Option<(u32, u32)>, view: &WeakEntity<BoardView>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (line, lit_line, lit) = (theme.border, theme.primary, theme.primary.opacity(0.3));
    let leave = view.clone();
    let rows = (1..=GRID).map(|row| {
        let cells = (1..=GRID).map(|col| {
            let on = hover.is_some_and(|(rows, cols)| row <= rows && col <= cols);
            let (point, place) = (view.clone(), view.clone());
            div()
                .id(("board-table-cell", ((row - 1) * GRID + col - 1) as usize))
                .size(px(CELL))
                .rounded(px(3.))
                .border_1()
                .border_color(if on { lit_line } else { line })
                .when(on, |cell| cell.bg(lit))
                .on_hover(move |entered, _, cx| {
                    if *entered {
                        let _ = point.update(cx, |view, cx| point_at(view, Some((row, col)), cx));
                    }
                })
                .on_click(move |_, _, cx| {
                    let _ = place.update(cx, |view, cx| view.place_table(row, col, cx));
                })
        });
        h_flex().gap(px(3.)).children(cells)
    });
    v_flex()
        .id("board-table-grid")
        .gap(px(3.))
        .on_hover(move |entered, _, cx| {
            if !*entered {
                let _ = leave.update(cx, |view, cx| point_at(view, None, cx));
            }
        })
        .children(rows)
}

/// The pointer is over cell `cell` of the picker's grid (rows and columns), or off it.
fn point_at(view: &mut BoardView, cell: Option<(u32, u32)>, cx: &mut Context<BoardView>) {
    if let Some(picker) = &mut view.picker
        && picker.hover != cell
    {
        picker.hover = cell;
        cx.notify();
    }
}

/// A thin line between groups of buttons.
pub(super) fn divider(color: Hsla) -> Div {
    div().w(px(1.)).h(px(20.)).mx(px(4.)).bg(color)
}

/// A colour swatch button: `rgba` in a rounded square, marked while `selected`, with `label` (a
/// board colour's name) as its tooltip. The flyouts and the context bar draw colours alike.
pub(super) fn swatch(
    id: impl Into<ElementId>,
    rgba: [u8; 4],
    label: &str,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .ghost()
        .selected(selected)
        .w(px(SWATCH))
        .h(px(SWATCH))
        .p_0()
        .tooltip(capitalized(label))
        .child(
            div()
                .size(px(16.))
                .rounded(px(4.))
                .border_1()
                .border_color(hsla(0., 0., 0., 0.16))
                .bg(colour(rgba)),
        )
        .on_click(on_click)
}

/// `rgba` (8 bits a channel) as GPUI's colour.
fn colour([r, g, b, a]: [u8; 4]) -> Hsla {
    let channel = |value: u8| f32::from(value) / 255.;
    Rgba {
        r: channel(r),
        g: channel(g),
        b: channel(b),
        a: channel(a),
    }
    .into()
}

/// `name` with a capital first letter: `yellow` reads "Yellow".
fn capitalized(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
