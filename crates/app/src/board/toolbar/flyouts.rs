//! The toolbar's flyouts: the chevron beside a tool with choices, and the choices it opens above
//! it, a row of swatches for colours or a list of pictures, labels and keys.

use gpui_kit::component::button::*;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::toolbar::{Flyout, FlyoutItem};

use super::{CHECK_ICON, CHEVRON_ICON, swatch, tool_icon};
use crate::board::{BoardView, commands, option_icons};

/// A tool button's side.
const BUTTON: f32 = 32.;
/// The width of the chevron beside a tool that opens its flyout.
const CHEVRON: f32 = 16.;

/// The side of a choice's picture in a flyout: a stamp's badge, a shape's outline, a connector's
/// line.
const OPTION_GLYPH: f32 = 16.;

/// What a flyout's chevron says it opens.
fn flyout_name(f: Flyout) -> &'static str {
    match f {
        Flyout::StickyColors => "Sticky note colors",
        Flyout::Shapes => "Shapes",
        Flyout::ConnectorStyles => "Connector styles",
        Flyout::Stamps => "Stamps",
        Flyout::Tools("frame") => "Frame tools",
        Flyout::Tools("shape" | "geometry") => "Shape tools",
        Flyout::Tools("pen") => "Drawing tools",
        Flyout::Tools("text") => "Text tools",
        Flyout::Tools(_) => "Tools",
    }
}

/// The chevron that opens flyout `f`, and the flyout above it, open while `items` are given, each
/// with its keys.
pub(super) fn flyout(
    f: Flyout,
    items: Option<Vec<(FlyoutItem, Option<String>)>>,
    view: &WeakEntity<BoardView>,
) -> Popover {
    let open = items.is_some();
    let items = items.unwrap_or_default();
    let name = SharedString::from(f.name());
    let chevron = Button::new(SharedString::from(format!("board-flyout-{name}")))
        .ghost()
        .xsmall()
        .icon(CHEVRON_ICON)
        .w(px(CHEVRON))
        .h(px(BUTTON))
        .accessibility_label(flyout_name(f))
        .tooltip(flyout_name(f))
        .debug_selector(move || format!("board-flyout-{name}"));
    let (on_change, view) = (view.clone(), view.clone());
    Popover::new(SharedString::from(format!(
        "board-flyout-popover-{}",
        f.name()
    )))
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

/// A flyout's choices: a row of swatches for colours, else a list of pictures and labels with the
/// one the tool makes now checked, and for a group's tools each tool's keys. Picking one closes
/// the flyout and runs its command with its params; Place image… asks for the file, as its menu
/// item does.
fn flyout_items(
    items: &[(FlyoutItem, Option<String>)],
    view: &WeakEntity<BoardView>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let (hover, muted) = (theme.accent, theme.muted_foreground);
    let pick = |item: &FlyoutItem| {
        let (command, params, view) = (item.command, item.params.clone(), view.clone());
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| {
                view.open_flyout = None;
                if sawit_board::policy::is_ui(command) {
                    // As its menu item: a text being typed in ends first (ruling R-TEXTEDIT).
                    if view.end_text_edit(Some(command), cx) {
                        commands::run_ui(view, command, window, cx);
                    }
                } else {
                    view.choose(command, params.clone(), cx);
                }
            });
        }
    };
    let selector = |item: &FlyoutItem| {
        let label = item.label.clone();
        move || format!("board-flyout-item-{label}")
    };
    if items.iter().all(|(item, _)| item.swatch.is_some()) {
        let swatches = items.iter().enumerate().map(|(i, (item, _))| {
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
    let rows = items.iter().enumerate().map(|(i, (item, keys))| {
        h_flex()
            .id(("board-flyout-item", i))
            .role(Role::MenuItem)
            .aria_label(item.label.clone())
            .aria_selected(item.selected)
            .when_some(keys.clone(), |row, keys| row.aria_description(keys))
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
            // The choice's picture before its words: a tool's icon, or a stamp's badge, a shape's
            // outline, a connector's line (`option_icons`).
            .when_some(item.icon, |row, icon| {
                row.child(Icon::new(tool_icon(icon)).small())
            })
            .when(item.icon.is_none(), |row| {
                let picture = option_icons::of_params(&item.params);
                row.children(picture.map(|picture| picture.element(px(OPTION_GLYPH))))
            })
            .child(div().flex_1().child(item.label.clone()))
            .when_some(keys.clone(), |row, keys| {
                row.child(div().pl_4().text_color(muted).child(keys))
            })
            .on_click(pick(item))
    });
    v_flex()
        .debug_selector(|| "board-flyout".into())
        .min_w(px(160.))
        .gap(px(2.))
        .children(rows)
        .into_any_element()
}
