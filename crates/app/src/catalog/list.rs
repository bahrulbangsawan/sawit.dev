//! A catalog list's parts: status pills, tags, logo tiles, the columns and their header, the
//! empty state, and the labels and items assistive technology reads.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Width of the status column, header and rows alike.
pub const STATUS_W: f32 = 200.;
/// Width of the actions column, header and rows alike.
pub const ACTIONS_W: f32 = 236.;
/// Side of a row's logo tile.
pub const TILE: f32 = 36.;
/// The narrowest the name column gets beside status and actions: narrower,
/// and they wrap under the name.
pub(super) const NAME_MIN: f32 = 240.;

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

/// Text that assistive technology reads as text: GPUI exposes no plain text to it, so a count or
/// a status that matters on its own is shown through this, under its element `id`.
pub fn label(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text.clone())
        .child(text)
}

/// A row of a catalog list as assistive technology sees it: an item named `name`, so a screen
/// reader (and a test) can tell the rows apart rather than hearing only their buttons.
pub fn list_item(
    row: Div,
    id: impl Into<ElementId>,
    name: impl Into<SharedString>,
) -> Stateful<Div> {
    row.id(id).role(Role::ListItem).aria_label(name)
}
