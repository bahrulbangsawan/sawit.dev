//! The pager under a catalog list: the count, the Show menu (how many rows a page holds) and
//! the page buttons.

use std::rc::Rc;

use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::pagination::Pagination;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::label;
use super::paging::{PageSize, Paging, save_size};

type OnPaging = Rc<dyn Fn(&Paging, &mut Window, &mut App)>;
type OnPageSize = Rc<dyn Fn(&PageSize, &mut Window, &mut App)>;

/// The Show menu: "Show 8", which opens `choices` ("Rows per page") with a check by `current`,
/// from its `anchor` corner. A size picked is kept as list `list`'s, for the next view opened and
/// the next launch ([`saved_size`](super::saved_size)), and handed to `on`.
pub fn page_size_menu(
    list: &'static str,
    current: PageSize,
    choices: &'static [PageSize],
    anchor: Anchor,
    on: impl Fn(&PageSize, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on: OnPageSize = Rc::new(on);
    Button::new(SharedString::from(format!("{list}-page-size")))
        .debug_selector(move || format!("{list}-page-size"))
        .ghost()
        .xsmall()
        .label(format!("Show {}", current.label()))
        .dropdown_caret(true)
        .tooltip("Rows per Page")
        .dropdown_menu_with_anchor(anchor, move |mut menu, _, _| {
            menu = menu.label("Rows per page");
            for size in choices.iter().copied() {
                let on = on.clone();
                menu = menu.item(
                    PopupMenuItem::new(size.label())
                        .checked(size == current)
                        .on_click(move |_, window, cx| {
                            save_size(list, size, cx);
                            on(&size, window, cx);
                        }),
                );
            }
            menu
        })
}

/// Under the list: "9–16 of 24 tools", the Show menu (how many rows a page holds) and the page
/// buttons. `list` names its elements and keeps the size picked for the next page opened and the
/// next launch ([`Paging::new`]); `paging` is what [`page_of`](super::page_of) gave. `on` gets the
/// page turned to, or the new size on the page holding row `selected` (the list's selected row, by
/// its place in the whole list) when there is one, else the row that was first on the page. Too
/// narrow for one line, the page buttons wrap under the count, on the right.
pub fn pager(
    list: &'static str,
    paging: Paging,
    total: usize,
    noun: &str,
    selected: Option<usize>,
    on: impl Fn(&Paging, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let on: OnPaging = Rc::new(on);
    let per_page = paging.size.rows(total);
    let first = (paging.page * per_page + 1).min(total);
    let last = ((paging.page + 1) * per_page).min(total);
    let pages = paging.size.pages(total);
    let resize = on.clone();
    // Under the list, near the window's foot: the menu opens upwards.
    let size_menu = page_size_menu(
        list,
        paging.size,
        &PageSize::CHOICES,
        Anchor::BottomLeft,
        move |size, window, cx| resize(&paging.resized(*size, selected), window, cx),
    );
    h_flex()
        .px_4()
        .py_1()
        .min_h(px(40.))
        .flex_wrap()
        .gap_x_4()
        .gap_y_1()
        .justify_between()
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(
            h_flex()
                .debug_selector(move || format!("{list}-count"))
                .flex_none()
                .gap_2()
                .child(label(
                    SharedString::from(format!("{list}-count")),
                    format!("{first}–{last} of {total} {noun}"),
                ))
                // Fewer rows than the smallest page: the size changes nothing.
                .when(total > PageSize::CHOICES[0].rows(total), |el| {
                    el.child(size_menu)
                }),
        )
        .when(pages > 1, |el| {
            el.child(
                div()
                    .debug_selector(move || format!("{list}-pages"))
                    .flex_none()
                    .ml_auto()
                    .child(
                        Pagination::new(SharedString::from(format!("{list}-pages")))
                            .xsmall()
                            .current_page(paging.page + 1)
                            .total_pages(pages)
                            .on_click(move |p, window, cx| {
                                on(
                                    &Paging {
                                        page: p - 1,
                                        ..paging
                                    },
                                    window,
                                    cx,
                                )
                            }),
                    ),
            )
        })
}
