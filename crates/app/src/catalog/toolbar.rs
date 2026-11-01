//! A catalog page's toolbar: the search field, the filter tabs, the menus, and the rule between
//! groups of buttons.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;

type OnPick = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// The search field, with a magnifier and a clear button.
pub fn search(input: &Entity<InputState>, cx: &App) -> impl IntoElement + use<> {
    div().flex_1().min_w(px(180.)).max_w(px(340.)).child(
        Input::new(input)
            .small()
            .prefix(
                Icon::new(Lucide::Search)
                    .xsmall()
                    .text_color(cx.theme().muted_foreground),
            )
            .cleanable(true),
    )
}

/// Segmented tabs, "Installed 16", one picked; `on` gets the index.
pub fn tabs(
    id: &'static str,
    choices: Vec<(&'static str, usize)>,
    current: usize,
    on: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let on: OnPick = Rc::new(on);
    h_flex()
        .flex_none()
        .p(px(2.))
        .gap(px(2.))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .children(
            choices
                .into_iter()
                .enumerate()
                .map(move |(ix, (label, n))| {
                    let on = on.clone();
                    Button::new((id, ix))
                        .ghost()
                        .xsmall()
                        .selected(ix == current)
                        .toggled(ix == current)
                        .label(format!("{label} {n}"))
                        .on_click(move |_, window, cx| on(&ix, window, cx))
                }),
        )
}

/// A button that opens a menu of `options` with a check by the current one.
pub fn menu(
    id: &'static str,
    icon: Lucide,
    label: String,
    options: Vec<&'static str>,
    current: usize,
    on: impl Fn(&usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on: OnPick = Rc::new(on);
    Button::new(id)
        .outline()
        .small()
        .icon(icon)
        .label(label)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
            for (ix, option) in options.iter().enumerate() {
                let on = on.clone();
                menu = menu.item(
                    PopupMenuItem::new(*option)
                        .checked(ix == current)
                        .on_click(move |_, window, cx| on(&ix, window, cx)),
                );
            }
            menu
        })
        .into_any_element()
}

/// A thin upright rule between groups of buttons.
pub fn divider(cx: &App) -> impl IntoElement + use<> {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(14.))
        .mx_1()
        .bg(cx.theme().border)
}
