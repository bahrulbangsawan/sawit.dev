//! The Appearance section (spec "Design panel" 4): visibility, opacity, corner radius (one for
//! all corners, or each corner on its own), and blend mode.

use std::rc::Rc;

use figcraft_doc::color::BlendMode;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::{DesignSelection, Mixed};
use serde_json::{Value, json};

use super::number_field::NumberField;
use super::{icon_button, row, section};
use crate::board::BoardView;

/// The opacity field's picture.
pub(super) const OPACITY_ICON: Lucide = Lucide::Contrast;
/// The corner radius field's picture.
pub(super) const RADIUS_ICON: Lucide = Lucide::SquareRoundCorner;
/// The picture of the toggle that shows each corner's radius on its own.
pub(super) const CORNERS_ICON: Lucide = Lucide::Scan;
/// The blend mode menu's picture.
pub(super) const BLEND_ICON: Lucide = Lucide::Blend;
/// The visibility toggle's pictures: every layer shown, every one hidden, some of each.
pub(super) const SHOWN_ICON: Lucide = Lucide::Eye;
pub(super) const HIDDEN_ICON: Lucide = Lucide::EyeOff;
pub(super) const MIXED_SHOWN_ICON: Lucide = Lucide::EyeDashed;

/// The corners' own radii, in the order their fields show: the top two, then the bottom two.
const CORNERS: [(&str, &str); 4] = [
    ("topLeftRadius", "TL"),
    ("topRightRadius", "TR"),
    ("bottomLeftRadius", "BL"),
    ("bottomRightRadius", "BR"),
];

/// The blend modes after which the menu draws a line, as FigCraft's own menu groups them.
const GROUP_ENDS: [BlendMode; 5] = [
    BlendMode::Normal,
    BlendMode::ColorBurn,
    BlendMode::ColorDodge,
    BlendMode::HardLight,
    BlendMode::Exclusion,
];

/// The Appearance section for the layers of `sel`.
pub(super) fn render(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let opacity = NumberField::new("opacity", "Opacity")
        .icon(OPACITY_ICON)
        .render(view, sel, window, cx);
    let radius = NumberField::new("cornerRadius", "Corner radius")
        .icon(RADIUS_ICON)
        .render(view, sel, window, cx);
    let open = view.design.corners_open;
    let corners = {
        let view = this.clone();
        icon_button("design-corners", CORNERS_ICON, "Individual corners")
            .selected(open)
            .debug_selector(|| "design-corners".into())
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.design.corners_open = !view.design.corners_open;
                    cx.notify();
                });
            })
    };
    let mut rows = vec![
        row()
            .child(opacity)
            .child(radius)
            .child(corners)
            .into_any_element(),
    ];
    if open {
        let mut fields = CORNERS
            .map(|(key, label)| NumberField::new(key, label).render(view, sel, window, cx))
            .into_iter();
        while let (Some(left), Some(right)) = (fields.next(), fields.next()) {
            rows.push(row().child(left).child(right).into_any_element());
        }
    }
    rows.push(blend_menu(sel, &this));
    let actions = vec![visibility_toggle(sel, &this).into_any_element()];
    section("appearance", "Appearance", actions, rows, cx)
}

/// What the visibility toggle shows for `visible`, the layers' `visible`, and what a click sets:
/// it hides layers that are all shown, and shows layers that are all hidden, or some of each
/// (ruling P5-C17).
pub(super) fn visibility(visible: &Mixed) -> (Lucide, &'static str, bool) {
    match visible {
        Mixed::Same(Value::Bool(true)) => (SHOWN_ICON, "Hide", false),
        Mixed::Same(Value::Bool(false)) => (HIDDEN_ICON, "Show", true),
        _ => (MIXED_SHOWN_ICON, "Some are hidden: show all", true),
    }
}

/// The toggle that shows or hides the layers of `sel`.
fn visibility_toggle(sel: &Rc<DesignSelection>, view: &WeakEntity<BoardView>) -> Button {
    let (icon, tooltip, show) = visibility(&sel.get("visible"));
    let (sel, view) = (sel.clone(), view.clone());
    icon_button("design-visible", icon, tooltip)
        .debug_selector(|| "design-visible".into())
        .on_click(move |_, _, cx| {
            let edit = sel.set("visible", json!(show));
            let _ = view.update(cx, |view, cx| view.apply_edit(edit, cx));
        })
}

/// The blend mode the layers of `sel` share, or `None` when they differ.
fn blend_of(sel: &DesignSelection) -> Option<BlendMode> {
    let Mixed::Same(blend) = sel.get("blend") else {
        return None;
    };
    BlendMode::ALL.into_iter().find(|mode| json!(mode) == blend)
}

/// The blend mode menu: its button shows the layers' mode ("Mixed" when they differ), and its
/// items are every mode, theirs checked. Pass through, which a frame or a group composites with
/// unless it is told otherwise, is among them.
fn blend_menu(sel: &Rc<DesignSelection>, view: &WeakEntity<BoardView>) -> AnyElement {
    let current = blend_of(sel);
    let label = current.map_or("Mixed", BlendMode::label);
    let (sel, view) = (sel.clone(), view.clone());
    Button::new("design-blend")
        .ghost()
        .small()
        .icon(BLEND_ICON)
        .label(label)
        .dropdown_caret(true)
        .w_full()
        .tooltip("Blend mode")
        .debug_selector(|| "design-blend".into())
        .dropdown_menu(move |mut menu: PopupMenu, _, _| {
            for mode in BlendMode::ALL {
                let (sel, view) = (sel.clone(), view.clone());
                let label = mode.label();
                let item = PopupMenuItem::element(move |_, _| {
                    div()
                        .debug_selector(move || format!("design-blend-{label}"))
                        .child(label)
                })
                .checked(current == Some(mode))
                .on_click(move |_, _, cx| {
                    let edit = sel.set("blend", json!(mode));
                    let _ = view.update(cx, |view, cx| view.apply_edit(edit, cx));
                });
                menu = menu.item(item);
                if GROUP_ENDS.contains(&mode) {
                    menu = menu.separator();
                }
            }
            menu
        })
        .into_any_element()
}
