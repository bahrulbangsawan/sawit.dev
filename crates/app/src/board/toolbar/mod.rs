//! The floating toolbar at the bottom centre of the canvas (spec D11, "Toolbar and context bar"):
//! the tools of a board or of a design file in their groups (`sawit_board::toolbar::groups`), a
//! chevron beside each board tool with choices that opens its flyout, and on a board a last button
//! for the Templates & table picker. A group of several tools of one kind (the frames, the shapes,
//! the pens, the texts) shows one of them, the one in use or used last, with a chevron whose
//! flyout lists them all with their keys, as FigCraft's own toolbar does; the shapes' flyout ends
//! with Place image…. A button or a choice runs its command, with its params, through
//! [`BoardView::choose`], as the model's tests run them; Place image… asks for the file
//! ([`commands::run_ui`]).
//!
//! The button of the session's tool is marked on every frame, so a tool chosen by a key shows at
//! once (Review Focus 3), and so does the group's button, which shows it. A flyout or the picker
//! that is open closes when the file on show turns from a board into a design file or back, and
//! when the UI is hidden ([`BoardView::settle_chrome`]).

mod flyouts;
mod picker;

use figcraft_engine::{ToolId, guard};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::DocKind;
use sawit_board::toolbar::{
    self as model, Flyout, FlyoutItem, Template, ToolButton, ToolGroup, ToolIcon,
};
use serde_json::json;

use super::BoardView;
use super::top_bar::with_keys;
use flyouts::flyout;
use picker::picker_button;

/// The toolbar's height.
pub(in crate::board) const HEIGHT: f32 = 44.;
/// How far the toolbar floats off the canvas's bottom edge.
pub(in crate::board) const BOTTOM: f32 = 16.;
/// The least room the toolbar leaves at each side of the canvas: in a canvas narrower than the
/// toolbar and this room, the toolbar is as wide as the canvas less it, and its buttons scroll.
const MARGIN: f32 = 8.;
/// The side of a colour swatch's button, in the flyouts and the context bar.
pub(super) const SWATCH: f32 = 24.;

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

/// The Lucide icon that pictures a tool, from the canvas's one mapping of pictures
/// ([`super::option_icons::tool`]).
pub(super) fn tool_icon(icon: ToolIcon) -> Lucide {
    super::option_icons::tool(icon)
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
/// guard: a panic in it costs that frame its toolbar, not the app (D15). The tool in use is
/// remembered for its group, which shows it until another of the group's tools is used.
fn shown(view: &mut BoardView) -> Shown {
    let shown = read_shown(view);
    if let Some(active) = shown.active
        && let Some(group) =
            (shown.groups.iter()).find(|group| group.tools.iter().any(|tool| tool.tool == active))
    {
        view.last_tools.insert(group.key, active);
    }
    shown
}

/// [`shown`], read.
fn read_shown(view: &BoardView) -> Shown {
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

/// The toolbar of `view`'s canvas, centred [`BOTTOM`] off its bottom edge. It takes the pointer:
/// no click or scroll over it reaches the canvas under it.
///
/// It keeps its full width while the canvas has room for it and [`MARGIN`] at each side, and is
/// never wider than the canvas less that room. Narrower than its buttons, it keeps them at their
/// size and they scroll sideways inside it, under a swipe or the wheel (FigCraft's toolbar pill,
/// ui-egui `toolbar.rs`, `pill_rect` and `pill_scroll`).
pub(super) fn render_toolbar(
    view: &mut BoardView,
    _: &mut Window,
    cx: &mut Context<BoardView>,
) -> impl IntoElement + use<> {
    let this = cx.entity().downgrade();
    let shown = shown(view);
    let picker = view.picker.clone();
    // The buttons each group shows: all its tools, or for a group whose flyout lists them the one
    // in use or used last.
    let buttons: Vec<Vec<ToolButton>> = (shown.groups.iter())
        .map(|group| match group.flyout {
            Some(Flyout::Tools(_)) => {
                let active = shown.active.unwrap_or_default();
                let last = view.last_tools.get(group.key).copied();
                vec![model::shown_tool(group, active, last).clone()]
            }
            _ => group.tools.clone(),
        })
        .collect();
    // Each button's keys, and those of the open flyout's tools, as the keymap binds them now.
    let keys: Vec<Vec<Option<String>>> = (buttons.iter())
        .map(|tools| {
            (tools.iter())
                .map(|tool| super::command_keys(tool.command, cx))
                .collect()
        })
        .collect();
    let flyout_keys: Vec<Option<String>> = (shown.flyout.iter())
        .flat_map(|(_, items)| items)
        .map(|item| {
            item.icon
                .and_then(|_| super::command_keys(item.command, cx))
        })
        .collect();
    let theme = cx.theme();
    let line = theme.border;
    // The buttons, chevrons and lines, at their full size: the row never shrinks.
    let mut row = h_flex().flex_shrink_0().gap(px(2.));
    for (i, ((group, tools), keys)) in shown.groups.iter().zip(&buttons).zip(keys).enumerate() {
        if i > 0 {
            row = row.child(divider(line));
        }
        for (tool, keys) in tools.iter().zip(keys) {
            row = row.child(tool_button(
                tool,
                keys,
                shown.active == Some(tool.tool),
                &this,
            ));
        }
        if let Some(f) = group.flyout {
            let items = (shown.flyout.as_ref())
                .filter(|(open, _)| *open == f)
                .map(|(_, items)| items.iter().cloned().zip(flyout_keys.clone()).collect());
            row = row.child(flyout(f, items, &this));
        }
    }
    if shown.board {
        row = row.child(divider(line)).child(picker_button(picker, &this));
    }
    // The part of the row the toolbar has room for. A swipe scrolls it sideways, and so does the
    // wheel: GPUI turns a vertical scroll into a sideways one in a box that only scrolls
    // sideways. The row is cut inside the toolbar's border, never over it. The toolbar's 6 px of
    // padding is split: half is the strip's own, room for the focus ring a button draws outside
    // itself (3 px in gpui-component), which the cut would otherwise take off the first and last
    // buttons. The flyouts and the picker still open above it: GPUI draws popovers last, uncut.
    let strip = h_flex()
        .id("board-toolbar-strip")
        .min_w_0()
        .h_full()
        .px(px(3.))
        .overflow_x_scroll()
        .child(row);
    let bar = h_flex()
        .id("board-toolbar")
        .role(Role::Toolbar)
        .aria_label("Canvas tools")
        .debug_selector(|| "board-toolbar".into())
        .occlude()
        .min_w_0()
        .h(px(HEIGHT))
        .px(px(3.))
        .rounded(px(12.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow_md()
        .child(strip);
    // The row around the toolbar takes no clicks: only the toolbar does. Its padding is the room
    // the toolbar leaves at the canvas's sides: the toolbar shrinks to fit inside it.
    h_flex()
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(BOTTOM))
        .px(px(MARGIN))
        .justify_center()
        .child(bar)
}

/// The button of `tool`, marked while it is the `active` one. Its tooltip reads its label and
/// `keys`, the keymap's ("Sticky note  Q").
fn tool_button(
    tool: &ToolButton,
    keys: Option<String>,
    active: bool,
    view: &WeakEntity<BoardView>,
) -> Button {
    let command = tool.command;
    let view = view.clone();
    Button::new(command)
        .ghost()
        .icon(tool_icon(tool.icon))
        .selected(active)
        .accessibility_label(tool.label)
        .toggled(active)
        .tooltip(with_keys(tool.label, keys))
        .debug_selector(|| command.to_owned())
        .on_click(move |_, _, cx| {
            let _ = view.update(cx, |view, cx| view.choose(command, json!({}), cx));
        })
}

/// A thin line between groups of buttons.
pub(in crate::board) fn divider(color: Hsla) -> Div {
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
        .accessibility_label(capitalized(label))
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
