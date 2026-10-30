//! The Position section (spec "Design panel" 2): X and Y; W and H, with the proportions lock
//! beside them; rotation; and the align and distribute buttons, which act on the engine's
//! selection as their shortcuts do.

use std::rc::Rc;

use figcraft_engine::cmd::find_command;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::{DesignSelection, Mixed, align};
use sawit_board::menu::shortcut_label;
use serde_json::json;

use super::number_field::NumberField;
use super::{icon_button, row, section};
use crate::board::BoardView;
use crate::board::top_bar::with_keys;

/// The rotation field's picture.
pub(super) const ROTATION_ICON: Lucide = Lucide::RotateCw;
/// The proportions lock while it is on, and while it is off.
pub(super) const LINKED_ICON: Lucide = Lucide::Link;
pub(super) const UNLINKED_ICON: Lucide = Lucide::Unlink;

/// The align and distribute buttons, in the order they show: each one's command and picture.
const ALIGNS: [(&str, Lucide); 8] = [
    ("align.left", Lucide::AlignStartVertical),
    ("align.hcenter", Lucide::AlignCenterVertical),
    ("align.right", Lucide::AlignEndVertical),
    ("align.top", Lucide::AlignStartHorizontal),
    ("align.vcenter", Lucide::AlignCenterHorizontal),
    ("align.bottom", Lucide::AlignEndHorizontal),
    (
        "distribute.horizontal",
        Lucide::AlignHorizontalDistributeCenter,
    ),
    ("distribute.vertical", Lucide::AlignVerticalDistributeCenter),
];

/// The pictures of the align and distribute buttons, for the test that they are embedded.
#[cfg(test)]
pub(super) fn align_icons() -> impl Iterator<Item = Lucide> {
    ALIGNS.into_iter().map(|(_, icon)| icon)
}

/// The Position section for the layers of `sel`.
pub(super) fn render(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let x = NumberField::new("x", "X").render(view, sel, window, cx);
    let y = NumberField::new("y", "Y").render(view, sel, window, cx);
    let width = NumberField::new("width", "W").render(view, sel, window, cx);
    let height = NumberField::new("height", "H").render(view, sel, window, cx);
    let rotation = NumberField::new("rotation", "Rotation")
        .icon(ROTATION_ICON)
        .render(view, sel, window, cx);
    let rows = vec![
        row().child(x).child(y).into_any_element(),
        row()
            .child(width)
            .child(height)
            .child(proportions_lock(sel, &this))
            .into_any_element(),
        // Half the row, as wide as the fields above it.
        row()
            .child(rotation)
            .child(div().flex_1())
            .into_any_element(),
        align_buttons(sel.ids.len(), &this),
    ];
    section("position", "Position", Vec::new(), rows, cx)
}

/// The proportions lock of the layers of `sel`: on while every one keeps its proportions, when
/// W or H typed or scrubbed changes the other too. A click turns it off, or on for all of them
/// when it is off for any.
fn proportions_lock(sel: &Rc<DesignSelection>, view: &WeakEntity<BoardView>) -> Button {
    let locked = sel.get("constrainProportions") == Mixed::Same(json!(true));
    let (sel, view) = (sel.clone(), view.clone());
    let icon = if locked { LINKED_ICON } else { UNLINKED_ICON };
    icon_button("design-constrain", icon, "Constrain proportions")
        .selected(locked)
        .debug_selector(|| "design-constrain".into())
        .on_click(move |_, _, cx| {
            let edit = sel.set("constrainProportions", json!(!locked));
            let _ = view.update(cx, |view, cx| view.apply_edit(edit, cx));
        })
}

/// The align and distribute buttons for `count` selected layers, each with the engine's label
/// and its keys as its tooltip. Distributing takes three layers or more: with fewer, its buttons
/// are off, as the engine would do nothing.
fn align_buttons(count: usize, view: &WeakEntity<BoardView>) -> AnyElement {
    let buttons = ALIGNS.into_iter().map(|(command, icon)| {
        let view = view.clone();
        let label = find_command(command).map_or(command, |spec| spec.label);
        let distribute = command.starts_with("distribute.");
        icon_button(command, icon, with_keys(label, shortcut_label(command)))
            .disabled(distribute && count < 3)
            .debug_selector(move || format!("design-{command}"))
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.apply_edit(align(command), cx));
            })
    });
    h_flex()
        .w_full()
        .justify_between()
        .children(buttons)
        .into_any_element()
}
