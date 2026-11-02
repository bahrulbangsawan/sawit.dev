//! What the Design panel's sections are made of: the section with its title and actions, rows,
//! small buttons, the "Mixed" row of a list, menus of choices, and the guard every model the panel
//! is drawn from is built behind.

use std::rc::Rc;

use figcraft_engine::guard;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::design::{DesignSelection, Edit};
use serde_json::{Value, json};

use super::appearance;
use crate::board::BoardView;
use crate::board::option_icons::{self, OptionIcon};

/// What `model` gives, built behind the engine's guard as every model the panel is drawn from:
/// a panic costs the panel that part, which shows as `fallback`, and is logged, not the app
/// (D15, ruling on P2.T3).
pub(super) fn guarded<T>(name: &str, fallback: T, model: impl FnOnce() -> T) -> T {
    guard::run(name, || Ok(model())).unwrap_or(fallback)
}

/// A click handler that makes an edit of the selected layers, as they are when the click comes,
/// with `make`, and runs it ([`BoardView::edit_layers`]).
pub(super) fn on_click_edit(
    view: &WeakEntity<BoardView>,
    make: impl Fn(&DesignSelection) -> Option<Edit> + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, _, cx| {
        let _ = view.update(cx, |view, cx| view.edit_layers(&make, cx));
    }
}

/// A section of the panel: `title`, with `actions` (small buttons) on its right, over `rows`, and
/// a line under it all.
pub(super) fn section(
    id: &'static str,
    title: &'static str,
    actions: Vec<AnyElement>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .debug_selector(move || format!("design-section-{id}"))
        .w_full()
        .px_3()
        .pt_2()
        .pb_3()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .w_full()
                .h(px(24.))
                .justify_between()
                .child(div().font_medium().child(title))
                .child(h_flex().gap_1().children(actions)),
        )
        .children(rows)
        .into_any_element()
}

/// A row of a section: fields and buttons side by side.
pub(super) fn row() -> Div {
    h_flex().w_full().gap_2()
}

/// A small button showing `icon` alone, for a section's toggles and actions.
pub(super) fn icon_button(
    id: impl Into<ElementId>,
    icon: Lucide,
    tooltip: impl Into<SharedString>,
) -> Button {
    Button::new(id).ghost().xsmall().icon(icon).tooltip(tooltip)
}

/// The button of row `id` (`fills-0`, `effects-1`) that makes `make`'s edit, showing `icon` and
/// `tooltip`, drawn as `design-<id>-<what>`.
pub(super) fn row_button(
    id: &str,
    what: &str,
    icon: Lucide,
    tooltip: &'static str,
    view: &WeakEntity<BoardView>,
    make: impl Fn(&DesignSelection) -> Option<Edit> + 'static,
) -> Button {
    let selector = format!("design-{id}-{what}");
    icon_button(SharedString::from(selector.clone()), icon, tooltip)
        .debug_selector(move || selector)
        .on_click(on_click_edit(view, make))
}

/// The eye of row `id`, a paint or an effect: open while the row shows. A click makes `make`'s
/// edit, which hides or shows it.
pub(super) fn eye_button(
    id: &str,
    visible: bool,
    view: &WeakEntity<BoardView>,
    make: impl Fn(&DesignSelection) -> Option<Edit> + 'static,
) -> Button {
    let (icon, tooltip) = if visible {
        (appearance::SHOWN_ICON, "Hide")
    } else {
        (appearance::HIDDEN_ICON, "Show")
    };
    row_button(id, "visible", icon, tooltip, view, make)
}

/// The row that stands for a list the layers hold differently (`list` is `fills`, `strokes`,
/// `effects` or `exports`): it says "Mixed", and its "Replace all" button makes `replace`'s edit,
/// which gives every layer the same list.
pub(super) fn mixed_row(
    list: &str,
    view: &WeakEntity<BoardView>,
    replace: impl Fn(&DesignSelection) -> Option<Edit> + 'static,
    cx: &App,
) -> AnyElement {
    let selector = format!("design-{list}-replace-all");
    let button = Button::new(SharedString::from(selector.clone()))
        .ghost()
        .xsmall()
        .label("Replace all")
        .debug_selector(move || selector)
        .on_click(on_click_edit(view, replace));
    row()
        .items_center()
        .child(
            div()
                .flex_1()
                .text_color(cx.theme().muted_foreground)
                .child("Mixed"),
        )
        .child(button)
        .into_any_element()
}

/// What a menu of choices ([`choice_menu`]) offers and does: its items, each a value and its name,
/// which it asks for when it opens; the value the layers hold, if they hold one; what a pick
/// does with its value; and the picture of each value, if the menu has them.
pub(super) struct Choices<T> {
    items: Box<Offer<T>>,
    current: Option<T>,
    pick: Rc<Pick<T>>,
    /// The picture of a value, drawn before its name on the button and in the menu
    /// ([`super::option_icons`](option_icons)). `None` for a menu whose names are their own
    /// picture: a scale ("2×"), a font's style ("Bold").
    picture: Option<Rc<Picture<T>>>,
}

/// What a menu of choices offers: each value and its name.
type Offer<T> = dyn Fn() -> Vec<(T, SharedString)>;
/// What a pick from a menu of choices does with the value picked.
type Pick<T> = dyn Fn(&T, &mut App);
/// The picture of a value of a menu of choices.
type Picture<T> = dyn Fn(&T) -> Option<OptionIcon>;

impl<T> Choices<T> {
    pub(super) fn new(
        items: impl Fn() -> Vec<(T, SharedString)> + 'static,
        current: Option<T>,
        pick: impl Fn(&T, &mut App) + 'static,
    ) -> Self {
        Self {
            items: Box::new(items),
            current,
            pick: Rc::new(pick),
            picture: None,
        }
    }

    /// The menu with `picture` of each value before its name.
    pub(super) fn with_pictures(
        mut self,
        picture: impl Fn(&T) -> Option<OptionIcon> + 'static,
    ) -> Self {
        self.picture = Some(Rc::new(picture));
        self
    }
}

/// A menu of choices for a property of the layers, drawn as `design-<id>`, taking an equal share
/// of its row with the others: its button shows `label` (the layers' choice, or "Mixed" when they
/// differ), with the choice's picture when the menu has them, and `tooltip`, and its items are the
/// ones `choices` offers, each drawn as `design-<id>-<name>` with its picture before its name, the
/// one the layers hold checked. Like the blend mode and sizing menus, it
/// leaves the keyboard where it was when it opened (delegate ruling on P5.T4). An `off` menu
/// doesn't open.
pub(super) fn choice_menu<T: Clone + PartialEq + 'static>(
    id: &str,
    label: impl Into<SharedString>,
    tooltip: &'static str,
    choices: Choices<T>,
    off: bool,
) -> Div {
    let selector = format!("design-{id}");
    let prefix = selector.clone();
    let shown = (choices.picture.as_ref()).and_then(|picture| picture(choices.current.as_ref()?));
    let button = Button::new(SharedString::from(selector.clone()))
        .ghost()
        .small()
        .w_full()
        .when_some(shown, |button, shown| button.icon(shown.icon()))
        .label(label)
        .dropdown_caret(true)
        .tooltip(tooltip)
        .debug_selector(move || selector);
    let menu = if off {
        button.disabled(true).into_any_element()
    } else {
        let Choices {
            items: offered,
            current,
            pick,
            picture,
        } = choices;
        let menu = button.dropdown_menu(move |mut menu: PopupMenu, _, _| {
            for (value, name) in offered() {
                let checked = current.as_ref() == Some(&value);
                let selector = format!("{prefix}-{name}");
                let pick = pick.clone();
                let shown = picture.as_ref().and_then(|picture| picture(&value));
                let item = PopupMenuItem::element(move |_, _| {
                    let selector = selector.clone();
                    option_icons::menu_row(shown, name.clone()).debug_selector(move || selector)
                })
                .checked(checked)
                .on_click(move |_, _, cx| pick(&value, cx));
                menu = menu.item(item);
            }
            menu
        });
        menu.into_any_element()
    };
    div().flex_1().min_w_0().child(menu)
}

/// The edit that sets several properties at once, `props`, on every selected layer, as
/// [`DesignSelection::set`] sets one that is not a text's.
pub(super) fn set_props(sel: &DesignSelection, props: Value) -> Edit {
    Edit {
        command: "node.set",
        params: json!({"ids": sel.ids, "props": props}),
    }
}
