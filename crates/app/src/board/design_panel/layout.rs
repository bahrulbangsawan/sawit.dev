//! The Auto layout section (spec "Design panel" 3), shown while every selected layer is a frame:
//! add and remove; the direction (vertical, horizontal, or wrapped onto new rows); the gap; the
//! padding, one for all sides or each side on its own; the 3×3 grid that aligns the children; and
//! how the frames size themselves across and down (fixed, hug or fill).
//!
//! The edits are `sawit_board::design::layout`'s and `DesignSelection::set`'s: an auto layout key
//! goes to every selected frame, so frames of which only some have auto layout read "Mixed", and
//! an edit gives the rest one too.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::design::layout::{add_layout, align_cell, remove_layout};
use sawit_board::design::{DesignSelection, Edit, Mixed};
use serde_json::{Value, json};

use super::number_field::NumberField;
use super::{icon_button, row, section, set_props};
use crate::board::BoardView;

/// The pictures of the buttons that add and remove auto layout.
pub(super) const ADD_ICON: Lucide = Lucide::Plus;
pub(super) const REMOVE_ICON: Lucide = Lucide::Minus;
/// The direction buttons' pictures.
pub(super) const VERTICAL_ICON: Lucide = Lucide::ArrowDown;
pub(super) const HORIZONTAL_ICON: Lucide = Lucide::ArrowRight;
pub(super) const WRAP_ICON: Lucide = Lucide::TextWrap;
/// The gap field's picture, across for a horizontal layout and down for a vertical one.
pub(super) const GAP_ACROSS_ICON: Lucide = Lucide::MoveHorizontal;
pub(super) const GAP_DOWN_ICON: Lucide = Lucide::MoveVertical;
/// The padding field's picture.
pub(super) const PADDING_ICON: Lucide = Lucide::SquareDashed;
/// The picture of the toggle that shows each side's padding on its own.
pub(super) const SIDES_ICON: Lucide = Lucide::Scan;

/// The side of the alignment grid.
const GRID: f32 = 64.;

/// The directions, in the order their buttons show: each one's name, picture, and the layout and
/// wrap it sets. Wrap is a horizontal layout that runs its children onto new rows.
const DIRECTIONS: [(&str, Lucide, &str, bool); 3] = [
    ("vertical", VERTICAL_ICON, "vertical", false),
    ("horizontal", HORIZONTAL_ICON, "horizontal", false),
    ("wrap", WRAP_ICON, "horizontal", true),
];

/// The sides' own padding, in the order their fields show: top and right, then bottom and left.
const SIDES: [(&str, &str); 4] = [
    ("paddingTop", "T"),
    ("paddingRight", "R"),
    ("paddingBottom", "B"),
    ("paddingLeft", "L"),
];

/// How a frame can size itself along an axis: what `node.set` takes, what the button shows, and
/// what the menu says.
const SIZINGS: [(&str, &str, &str); 3] = [
    ("fixed", "Fixed", "Fixed"),
    ("hug", "Hug", "Hug contents"),
    ("fill", "Fill", "Fill container"),
];

/// The Auto layout section for the frames of `sel`. Without auto layout on any of them it is its
/// title and the button that adds it.
pub(super) fn render(
    view: &mut BoardView,
    sel: &Rc<DesignSelection>,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let this = cx.entity().downgrade();
    let actions = add_and_remove(sel, &this);
    let layout = sel.get("layout");
    if layout == Mixed::Absent {
        return section("layout", "Auto layout", actions, Vec::new(), cx);
    }
    let gap_icon = if layout == Mixed::Same(json!("vertical")) {
        GAP_DOWN_ICON
    } else {
        GAP_ACROSS_ICON
    };
    let gap = NumberField::new("gap", "Gap")
        .icon(gap_icon)
        .render(view, sel, window, cx);
    let padding = NumberField::new("padding", "Padding")
        .icon(PADDING_ICON)
        .render(view, sel, window, cx);
    let open = view.design.padding_open;
    let sides = {
        let view = this.clone();
        icon_button("design-padding-sides", SIDES_ICON, "Individual padding")
            .selected(open)
            .debug_selector(|| "design-padding-sides".into())
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.design.padding_open = !view.design.padding_open;
                    cx.notify();
                });
            })
    };
    let fields = v_flex()
        .flex_1()
        .min_w_0()
        .gap_2()
        .child(gap)
        .child(row().child(padding).child(sides));
    let mut rows = vec![
        directions(sel, &this),
        h_flex()
            .w_full()
            .gap_2()
            .items_start()
            .child(alignment_grid(sel, &this, cx))
            .child(fields)
            .into_any_element(),
    ];
    if open {
        let mut fields = SIDES
            .map(|(key, label)| NumberField::new(key, label).render(view, sel, window, cx))
            .into_iter();
        while let (Some(left), Some(right)) = (fields.next(), fields.next()) {
            rows.push(row().child(left).child(right).into_any_element());
        }
    }
    rows.extend(sizing(sel, &this, cx));
    section("layout", "Auto layout", actions, rows, cx)
}

/// Whether `edit` names any layer: the add and remove edits name none when there is no frame for
/// them.
fn names_layers(edit: &Edit) -> bool {
    edit.params["ids"]
        .as_array()
        .is_some_and(|ids| !ids.is_empty())
}

/// The buttons that add auto layout to the frames of `sel` that have none, and remove it from
/// those that have it: each while there is such a frame.
fn add_and_remove(sel: &Rc<DesignSelection>, view: &WeakEntity<BoardView>) -> Vec<AnyElement> {
    let mut buttons = Vec::new();
    if names_layers(&add_layout(sel)) {
        let (sel, view) = (sel.clone(), view.clone());
        let add = icon_button("design-layout-add", ADD_ICON, "Add auto layout")
            .debug_selector(|| "design-layout-add".into())
            .on_click(move |_, _, cx| {
                let _ = view.update(cx, |view, cx| view.add_auto_layout(&sel, cx));
            });
        buttons.push(add.into_any_element());
    }
    let remove = remove_layout(sel);
    if names_layers(&remove) {
        let view = view.clone();
        let button = icon_button("design-layout-remove", REMOVE_ICON, "Remove auto layout")
            .debug_selector(|| "design-layout-remove".into())
            .on_click(move |_, _, cx| {
                let edit = remove.clone();
                let _ = view.update(cx, |view, cx| view.apply_edit(edit, cx));
            });
        buttons.push(button.into_any_element());
    }
    buttons
}

impl BoardView {
    /// Gives the frames of `sel` that have no auto layout one ([`add_layout`]), as one undo step.
    /// The engine selects the frames it gave one; the selection is the user's, so it goes back to
    /// what it was, the frames that had a layout already among it.
    fn add_auto_layout(&mut self, sel: &DesignSelection, cx: &mut Context<Self>) {
        let edit = add_layout(sel);
        self.run(
            |board| {
                let selection = board.session.selection();
                board.execute(edit.command, &edit.params)?;
                if board.session.selection() != selection {
                    board.session.set_selection(selection);
                }
                Ok(())
            },
            cx,
        );
    }
}

/// The direction buttons: the one the frames of `sel` all run in is marked. A click sets its
/// layout and wrap together, as one undo step.
fn directions(sel: &Rc<DesignSelection>, view: &WeakEntity<BoardView>) -> AnyElement {
    let (layout, wrap) = (sel.get("layout"), sel.get("wrap"));
    let buttons = DIRECTIONS.map(|(name, icon, mode, wraps)| {
        let on = if wraps {
            wrap == Mixed::Same(json!(true))
        } else {
            layout == Mixed::Same(json!(mode)) && wrap == Mixed::Same(json!(false))
        };
        let (sel, view) = (sel.clone(), view.clone());
        let tooltip = match name {
            "vertical" => "Vertical layout",
            "horizontal" => "Horizontal layout",
            _ => "Wrap",
        };
        icon_button(
            SharedString::from(format!("design-layout-{name}")),
            icon,
            tooltip,
        )
        .selected(on)
        .debug_selector(move || format!("design-layout-{name}"))
        .on_click(move |_, _, cx| {
            let edit = set_props(&sel, json!({"layout": mode, "wrap": wraps}));
            let _ = view.update(cx, |view, cx| view.apply_edit(edit, cx));
        })
    });
    h_flex().gap_1().children(buttons).into_any_element()
}

/// The cell of the alignment grid the frames of `sel` sit in, as its row and column from the top
/// left: `None` when they differ, or when their alignment has no cell (an auto gap, baseline).
/// A horizontal layout (and a grid) runs along the columns, a vertical one down the rows
/// (`sawit_board::design::layout::align_cell`).
fn current_cell(sel: &DesignSelection) -> Option<(u8, u8)> {
    let index = |value: Mixed| -> Option<u8> {
        let Mixed::Same(Value::String(align)) = value else {
            return None;
        };
        let at = ["min", "center", "max"]
            .iter()
            .position(|cell| *cell == align)?;
        u8::try_from(at).ok()
    };
    let Mixed::Same(mode) = sel.get("layout") else {
        return None;
    };
    let (main, cross) = (index(sel.get("mainAlign"))?, index(sel.get("crossAlign"))?);
    Some(if mode == json!("vertical") {
        (main, cross)
    } else {
        (cross, main)
    })
}

/// The 3×3 grid that aligns the children of the frames of `sel`: a click on a cell puts them
/// there ([`align_cell`]), and the cell they sit in is marked.
fn alignment_grid(sel: &Rc<DesignSelection>, view: &WeakEntity<BoardView>, cx: &App) -> AnyElement {
    let current = current_cell(sel);
    let theme = cx.theme();
    let (border, hover, dot, mark) = (
        theme.input,
        theme.accent,
        theme.muted_foreground,
        theme.foreground,
    );
    let rows = (0..3u8).map(|row| {
        let cells = (0..3u8).map(|col| {
            let on = current == Some((row, col));
            let (sel, view) = (sel.clone(), view.clone());
            div()
                .id(("design-align-cell", usize::from(row * 3 + col)))
                .debug_selector(move || format!("design-align-cell-{row}-{col}"))
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .hover(move |cell| cell.bg(hover))
                .child(
                    div()
                        .size(px(if on { 8. } else { 3. }))
                        .rounded(px(if on { 2. } else { 1.5 }))
                        .bg(if on { mark } else { dot }),
                )
                .on_click(move |_, _, cx| {
                    let edit = align_cell(&sel, row, col);
                    let _ = view.update(cx, |view, cx| view.apply_edit(edit, cx));
                })
        });
        h_flex().flex_1().w_full().children(cells)
    });
    v_flex()
        .debug_selector(|| "design-align-grid".into())
        .flex_shrink_0()
        .size(px(GRID))
        .p(px(2.))
        .rounded(px(5.))
        .border_1()
        .border_color(border)
        .children(rows)
        .into_any_element()
}

/// The sizing menus across (W) and down (H) of the frames of `sel`, while they have a sizing to
/// choose: inside auto layout, or with one of their own.
fn sizing(sel: &Rc<DesignSelection>, view: &WeakEntity<BoardView>, cx: &App) -> Option<AnyElement> {
    let (across, down) = (sel.get("sizingH"), sel.get("sizingV"));
    if across == Mixed::Absent && down == Mixed::Absent {
        return None;
    }
    let menus = [("sizingH", "W", across), ("sizingV", "H", down)]
        .map(|(key, axis, current)| sizing_menu(key, axis, &current, sel, view, cx));
    Some(row().children(menus).into_any_element())
}

/// The sizing menu of `key` (`sizingH` or `sizingV`), named `axis`: its button shows `current`
/// ("Mixed" when the frames differ), and its items are fixed, hug and fill, the frames' checked.
/// The engine refuses a sizing that a frame can't take (hug without auto layout, fill outside
/// one), and says why in a toast.
fn sizing_menu(
    key: &'static str,
    axis: &'static str,
    current: &Mixed,
    sel: &Rc<DesignSelection>,
    view: &WeakEntity<BoardView>,
    cx: &App,
) -> AnyElement {
    let current = match current {
        Mixed::Same(Value::String(sizing)) => (SIZINGS.iter())
            .map(|(name, ..)| *name)
            .find(|name| *name == sizing.as_str()),
        _ => None,
    };
    let label = SIZINGS
        .iter()
        .find(|(name, ..)| Some(*name) == current)
        .map_or("Mixed", |(_, short, _)| *short);
    let (sel, view) = (sel.clone(), view.clone());
    let menu = Button::new(key)
        .ghost()
        .small()
        .label(label)
        .dropdown_caret(true)
        .flex_1()
        .tooltip(if axis == "W" {
            "Width sizing"
        } else {
            "Height sizing"
        })
        .debug_selector(move || format!("design-{key}"))
        .dropdown_menu(move |mut menu: PopupMenu, _, _| {
            for (name, _, long) in SIZINGS {
                let (sel, view) = (sel.clone(), view.clone());
                let item = PopupMenuItem::element(move |_, _| {
                    div()
                        .debug_selector(move || format!("design-{key}-{name}"))
                        .child(long)
                })
                .checked(current == Some(name))
                .on_click(move |_, _, cx| {
                    let edit = sel.set(key, json!(name));
                    let _ = view.update(cx, |view, cx| view.apply_edit(edit, cx));
                });
                menu = menu.item(item);
            }
            menu
        });
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(
            div()
                .flex_shrink_0()
                .w(px(14.))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(axis),
        )
        .child(menu)
        .into_any_element()
}
