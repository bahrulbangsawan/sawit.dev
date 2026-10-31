//! Pieces the catalog pages (Dev Tools, MCP) share: the toolbar (search,
//! filter tabs, menus), status pills, tags, logo tiles, the list's column
//! header, its pager and the empty state.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::pagination::Pagination;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Rows on one page of a catalog.
pub const PER_PAGE: usize = 8;

/// Width of the status column, header and rows alike.
pub const STATUS_W: f32 = 200.;
/// Width of the actions column, header and rows alike.
pub const ACTIONS_W: f32 = 236.;
/// Side of a row's logo tile.
pub const TILE: f32 = 36.;

/// How many pages `n` rows fill (at least one).
pub fn pages(n: usize) -> usize {
    n.div_ceil(PER_PAGE).max(1)
}

/// Page `page` (from 0) of `rows`, and that page clamped to the last one.
pub fn page_of<T>(rows: Vec<T>, page: usize) -> (Vec<T>, usize) {
    let page = page.min(pages(rows.len()) - 1);
    let rows = rows
        .into_iter()
        .skip(page * PER_PAGE)
        .take(PER_PAGE)
        .collect();
    (rows, page)
}

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

/// What a status pill leads with.
pub enum Mark {
    Icon(Lucide),
    Spinner,
}

/// A status: icon and words on a tint of `color`, so it never rests on the
/// colour alone.
pub fn pill(mark: Mark, text: impl Into<SharedString>, color: Hsla) -> Div {
    h_flex()
        .flex_none()
        .max_w_full()
        .h(px(22.))
        .px_2()
        .gap_1()
        .rounded_full()
        .border_1()
        .border_color(color.opacity(0.3))
        .bg(color.opacity(0.1))
        .text_xs()
        .text_color(color)
        .child(match mark {
            Mark::Icon(icon) => Icon::new(icon).xsmall().into_any_element(),
            Mark::Spinner => Spinner::new().xsmall().color(color).into_any_element(),
        })
        .child(div().min_w_0().truncate().child(text.into()))
}

/// A neutral label next to a name: "Remote", "Sign-in", "Runtime".
pub fn tag(icon: Option<Lucide>, text: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .h(px(18.))
        .px_1p5()
        .gap_1()
        .rounded(cx.theme().radius)
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .when_some(icon, |el, icon| el.child(Icon::new(icon).xsmall()))
        .child(text.into())
}

/// A logo on a square tile, so logos of any shape line up.
pub fn tile(logo: AnyElement, cx: &App) -> Div {
    div()
        .size(px(TILE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .child(logo)
}

/// The list's column titles: `lead` (a checkbox or a gap) under the
/// checkboxes, then name, status and actions over their columns.
pub fn header(lead: Option<AnyElement>, name: &'static str, cx: &App) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .px_4()
        .h(px(32.))
        .gap_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .text_xs()
        .font_medium()
        .text_color(muted)
        .when_some(lead, |el, lead| {
            el.child(div().w(px(16.)).flex_none().child(lead))
        })
        .child(div().w(px(TILE)).flex_none())
        .child(div().flex_1().child(name))
        .child(div().w(px(STATUS_W)).flex_none().child("Status"))
        .child(
            div()
                .w(px(ACTIONS_W))
                .flex_none()
                .flex()
                .justify_end()
                .child("Actions"),
        )
}

/// "9–16 of 24 tools" and the page buttons, under the list.
pub fn pager(
    id: &'static str,
    page: usize,
    total: usize,
    noun: &str,
    on: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let first = (page * PER_PAGE + 1).min(total);
    let last = ((page + 1) * PER_PAGE).min(total);
    h_flex()
        .px_4()
        .h(px(40.))
        .gap_2()
        .justify_between()
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(format!("{first}–{last} of {total} {noun}"))
        .when(pages(total) > 1, |el| {
            el.child(
                Pagination::new(id)
                    .xsmall()
                    .current_page(page + 1)
                    .total_pages(pages(total))
                    .on_click(move |p, window, cx| on(&(p - 1), window, cx)),
            )
        })
}

/// Nothing matches: says so and offers to clear what hides the rows.
pub fn empty(title: String, clear: Button, cx: &App) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .py_12()
        .gap_2()
        .items_center()
        .child(Icon::new(Lucide::SearchX).large().text_color(muted))
        .child(div().text_sm().font_semibold().child(title))
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("Try another search, or clear the filters."),
        )
        .child(div().pt_1().child(clear))
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
