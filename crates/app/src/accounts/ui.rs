//! The Accounts page's building blocks, after its design (`docs/design/accounts.sawit`): section
//! headers with an icon, grouped boxes of settings rows (label and description on the left, the
//! control right-aligned), status pills that carry an icon as well as a colour, account avatars,
//! usage meters and empty states. Everything takes its colours from the theme.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// The widest the content of a tab gets: wider, bars and rows are hard to read across.
pub const CONTENT_MAX: Pixels = px(720.);

/// A section's header: an icon, the title and whatever trails it (a note, an action).
pub fn section_header(icon: Lucide, title: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .flex_wrap()
        .gap_x_2()
        .gap_y_1()
        .mb_2()
        .min_h(px(24.))
        .child(
            Icon::new(icon)
                .small()
                .text_color(cx.theme().muted_foreground),
        )
        .child(div().text_sm().font_semibold().child(title.into()))
}

/// A box that groups rows: rows inside are split by hairlines (see [`rows`]).
pub fn group_box(cx: &App) -> Div {
    v_flex()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().sidebar)
        .overflow_hidden()
}

/// `items` stacked with a hairline between each.
pub fn rows(items: Vec<AnyElement>, cx: &App) -> Vec<AnyElement> {
    let border = cx.theme().border;
    items
        .into_iter()
        .enumerate()
        .map(|(ix, item)| {
            div()
                .when(ix > 0, |el| el.border_t_1().border_color(border))
                .child(item)
                .into_any_element()
        })
        .collect()
}

/// A settings row: an icon tile, the label with a one-line description under it, and the
/// control right-aligned. `dim` greys the words of a setting that is unavailable (its control
/// says so too, disabled).
pub fn setting_row(
    icon: Lucide,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    dim: bool,
    cx: &App,
) -> Div {
    h_flex()
        .gap_3()
        .px_4()
        .py_3()
        .min_h(px(56.))
        .child(icon_tile(icon, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .when(dim, |el| el.opacity(0.6))
                .child(div().text_sm().font_medium().child(title.into()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(description.into()),
                ),
        )
        .child(h_flex().flex_none().gap_2().child(control))
}

/// A 28 px tile with an icon: the start of a settings row.
pub fn icon_tile(icon: Lucide, cx: &App) -> Div {
    div()
        .size(px(28.))
        .flex_none()
        .rounded(cx.theme().radius)
        .bg(cx.theme().accent)
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(icon)
                .small()
                .text_color(cx.theme().muted_foreground),
        )
}

/// What a pill says about its state: the colour, always with an icon and words beside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Neutral,
    Good,
    Warn,
    Bad,
}

/// A small status pill: an icon and a label, tinted by `tone` (never the colour alone).
pub fn pill(
    id: impl Into<ElementId>,
    tone: Tone,
    icon: Option<Lucide>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (fg, bg) = match tone {
        Tone::Neutral => (theme.foreground, theme.accent),
        Tone::Good => (theme.success, theme.success.opacity(0.14)),
        Tone::Warn => (theme.warning, theme.warning.opacity(0.14)),
        Tone::Bad => (theme.danger, theme.danger.opacity(0.16)),
    };
    let label = label.into();
    h_flex()
        .id(id)
        .role(Role::Label)
        .aria_label(label.clone())
        .flex_none()
        .gap_1()
        .h(px(20.))
        .pl(px(6.))
        .pr_2()
        .rounded(px(10.))
        .bg(bg)
        .text_color(fg)
        .text_xs()
        .whitespace_nowrap()
        .when_some(icon, |el, icon| el.child(Icon::new(icon).xsmall()))
        .child(label)
}

/// A quiet text badge (`cp1`).
pub fn badge(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_none()
        .px(px(5.))
        .rounded(px(4.))
        .bg(cx.theme().accent)
        .text_color(cx.theme().muted_foreground)
        .text_size(px(11.))
        .line_height(px(16.))
        .child(text.into())
}

/// The account's colour, parsed from `#rrggbb` (Claude's orange when it can't be read).
pub fn account_color(hex: &str) -> Hsla {
    let rgb = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xD97757);
    gpui_kit::rgb(rgb).into()
}

/// The account's avatar: its colour as a tinted disc, ringed in the colour, with the name's
/// first letter. Not a status: status is a pill with words.
pub fn avatar(name: &str, hex: &str, size: f32, cx: &App) -> Div {
    let color = account_color(hex);
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    // The letter is the colour lifted toward the text, so it reads on the tinted disc.
    let ink = color.blend(cx.theme().foreground.opacity(0.45));
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(color.opacity(0.22))
        .border_1()
        .border_color(color.opacity(0.55))
        .flex()
        .items_center()
        .justify_center()
        .text_color(ink)
        .text_size(px((size * 0.42).round()))
        .font_semibold()
        .child(letter)
}

/// The meter's colour at `used` percent: the accent, the warning colour from 70%, the danger
/// colour from 90%. A meter at warning or danger also gets an icon ([`level_icon`]).
pub fn meter_color(used: f64, cx: &App) -> Hsla {
    let theme = cx.theme();
    match used.round() as i64 {
        ..70 => theme.primary,
        70..90 => theme.warning,
        _ => theme.danger,
    }
}

/// The icon a meter at `used` percent carries beside its colour, past the warning line.
pub fn level_icon(used: f64) -> Option<Lucide> {
    match used.round() as i64 {
        ..70 => None,
        70..90 => Some(Lucide::TriangleAlert),
        _ => Some(Lucide::CircleX),
    }
}

/// A thin meter track, `height` tall, filled to `fill` (0–1) in `color`.
pub fn track(fill: f32, color: Hsla, height: f32, cx: &App) -> Div {
    div()
        .h(px(height))
        .w_full()
        .rounded(px(height / 2.))
        .bg(cx.theme().accent)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .rounded(px(height / 2.))
                .w(relative(fill.clamp(0., 1.)))
                .bg(color),
        )
}

/// An empty state: an icon tile, what the place is, and the next step.
pub fn empty_state(
    icon: Lucide,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    cx: &App,
) -> Div {
    v_flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_8()
        .max_w(px(440.))
        .child(
            div()
                .size(px(40.))
                .mb_1()
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().accent)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(icon)
                        .size(px(18.))
                        .text_color(cx.theme().muted_foreground),
                ),
        )
        .child(
            div()
                .text_base()
                .font_semibold()
                .text_center()
                .child(title.into()),
        )
        .child(
            div()
                .text_sm()
                .text_center()
                .whitespace_normal()
                .text_color(cx.theme().muted_foreground)
                .child(detail.into()),
        )
}

/// A tab's scrolling body, with the content capped at [`CONTENT_MAX`] (or `max`).
pub fn tab_body(id: &'static str, max: Pixels) -> (Stateful<Div>, Div) {
    (
        div().id(id).flex_1().min_w_0().h_full().overflow_y_scroll(),
        v_flex().w_full().max_w(max).p_6().gap_6(),
    )
}
