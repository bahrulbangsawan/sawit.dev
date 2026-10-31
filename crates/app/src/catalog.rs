//! Pieces the catalog pages (Dev Tools, MCP) share: the toolbar (search,
//! filter tabs, menus), status pills, tags, logo tiles, the list's columns
//! and their header, its pager and the empty state.

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
/// The narrowest the name column gets beside status and actions: narrower,
/// and they wrap under the name.
const NAME_MIN: f32 = 240.;

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

/// A line of the list, the header's or a row's: `lead` (a checkbox or a gap)
/// and the tile, then name, status and actions in their columns. Where the
/// name would be narrower than `NAME_MIN`, status and then actions wrap under
/// it, actions still on the right, so a narrow page shows every column.
pub fn columns(
    lead: Option<AnyElement>,
    tile: impl IntoElement,
    name: impl IntoElement,
    status: impl IntoElement,
    actions: impl IntoElement,
) -> Div {
    h_flex()
        .gap_3()
        .items_start()
        .when_some(lead, |el, lead| {
            el.child(div().w(px(16.)).flex_none().child(lead))
        })
        .child(tile)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .flex_wrap()
                .items_start()
                .gap_x_3()
                .gap_y_2()
                .child(
                    div()
                        .flex_grow_1()
                        .flex_basis(px(NAME_MIN))
                        .min_w_0()
                        .child(name),
                )
                .child(
                    v_flex()
                        .w(px(STATUS_W))
                        .flex_none()
                        .max_w_full()
                        .items_start()
                        .child(status),
                )
                .child(
                    h_flex()
                        .w(px(ACTIONS_W))
                        .flex_none()
                        .max_w_full()
                        .ml_auto()
                        .justify_end()
                        .child(actions),
                ),
        )
}

/// The list's column titles: `lead` (a checkbox or a gap) over the
/// checkboxes, then name, status and actions over their columns. It keeps to
/// one line: a column that wrapped under the name has nothing to title.
pub fn header(lead: Option<AnyElement>, name: &'static str, cx: &App) -> impl IntoElement + use<> {
    columns(
        lead,
        div().w(px(TILE)).flex_none(),
        name,
        "Status",
        "Actions",
    )
    .h(px(32.))
    .px_4()
    .py_2()
    .overflow_hidden()
    .border_b_1()
    .border_color(cx.theme().border)
    .bg(cx.theme().secondary)
    .text_xs()
    .font_medium()
    .text_color(cx.theme().muted_foreground)
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

#[cfg(test)]
mod tests {
    use gpui_kit::{
        Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
        Styled as _, TestAppContext, VisualTestContext, Window, div, px, size,
    };

    use super::{NAME_MIN, TILE, columns};

    /// A line of the list whose parts the tests can find.
    struct Line;

    impl Render for Line {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(columns(
                None,
                div().size(px(TILE)).flex_none(),
                div().debug_selector(|| "name".into()).h(px(40.)),
                div()
                    .debug_selector(|| "status".into())
                    .w(px(140.))
                    .h(px(22.)),
                div()
                    .debug_selector(|| "actions".into())
                    .w(px(120.))
                    .h(px(28.)),
            ))
        }
    }

    /// Where a line `width` wide puts its name, status and actions.
    fn lay_out(width: f32, cx: &mut TestAppContext) -> [Bounds<Pixels>; 3] {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(width), px(400.)), |_, _| Line);
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        ["name", "status", "actions"].map(|selector| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} isn't drawn"))
        })
    }

    fn at(edge: Pixels) -> f32 {
        f32::from(edge).round()
    }

    #[gpui_kit::test]
    fn a_wide_list_keeps_name_status_and_actions_on_one_line(cx: &mut TestAppContext) {
        let [name, status, actions] = lay_out(1000., cx);
        assert_eq!(at(status.top()), at(name.top()));
        assert_eq!(at(actions.top()), at(name.top()));
        assert!(name.size.width > px(NAME_MIN), "{name:?}");
        assert!(name.right() < status.left() && status.right() < actions.left());
        assert_eq!(at(actions.right()), 1000.);
    }

    /// The window's narrowest page, with the sidebar open, still shows every column: what
    /// doesn't fit beside the name wraps under it, the actions on the right.
    #[gpui_kit::test]
    fn a_narrower_list_wraps_the_actions_under_the_name(cx: &mut TestAppContext) {
        let [name, status, actions] = lay_out(700., cx);
        assert_eq!(at(status.top()), at(name.top()));
        assert!(actions.top() >= name.bottom(), "{name:?} {actions:?}");
        assert_eq!(at(actions.right()), 700.);
    }

    #[gpui_kit::test]
    fn a_narrow_list_wraps_status_and_actions_under_the_name(cx: &mut TestAppContext) {
        let [name, status, actions] = lay_out(400., cx);
        assert!(status.top() >= name.bottom(), "{name:?} {status:?}");
        assert_eq!(at(status.left()), at(name.left()));
        assert!(actions.top() >= status.bottom(), "{status:?} {actions:?}");
        assert_eq!(at(actions.right()), 400.);
        assert!(name.right() <= px(400.), "{name:?}");
    }
}
