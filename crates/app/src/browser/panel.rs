//! The side panel beside the page: a title and a close button over the panel's own body, and
//! the pieces both panels build from, so Annotate and Design share one rhythm: a 12 px inset,
//! 16 px between sections, 8 px inside one, sections titled in small grey sentence case,
//! label/value rows on one label column, and empty and loading states that keep the frame.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{BrowserView, Panel};

/// How long a footer note stays.
const NOTE_FOR: Duration = Duration::from_secs(3);

/// A timer that runs `clear` on the panel after [`NOTE_FOR`]: keep it in the panel so a newer
/// note replaces it.
pub fn note_timer<T: 'static>(cx: &mut Context<T>, clear: fn(&mut T)) -> Task<()> {
    note_timer_for(cx, NOTE_FOR, clear)
}

/// [`note_timer`] for `after`.
pub fn note_timer_for<T: 'static>(
    cx: &mut Context<T>,
    after: Duration,
    clear: fn(&mut T),
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(after).await;
        let _ = this.update(cx, |this, cx| {
            clear(this);
            cx.notify();
        });
    })
}

/// The panel's width, on the right of the page.
pub const PANEL_WIDTH: f32 = 340.;
/// The width of the label column in label/value rows.
pub const LABEL_WIDTH: f32 = 92.;

/// `title` over `body`, with a close button in the header. The body lays out its own scroll
/// area, so a panel can keep its tabs or its actions in view.
pub fn frame(
    title: &str,
    body: impl IntoElement,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .child(
            h_flex()
                .h(px(36.))
                .flex_none()
                .pl_3()
                .pr_2()
                .gap_1()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.to_string()),
                )
                .child(
                    Button::new("browser-panel-close")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .tooltip(format!("Close {title}"))
                        .accessibility_label(format!("Close {title}"))
                        .on_click(on_close),
                ),
        )
        .child(div().flex_1().min_h_0().child(body))
}

/// A scrolling column with the panel's inset.
pub fn scroll(id: impl Into<ElementId>) -> Stateful<Div> {
    div().id(id).flex_1().min_h_0().overflow_y_scroll().p_3()
}

/// A section: its title (and what sits at its right, a count or an action) over its body.
pub fn section(
    title: impl Into<SharedString>,
    trailing: Option<AnyElement>,
    body: impl IntoElement,
    cx: &App,
) -> Div {
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .min_h(px(20.))
                .gap_2()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                        .child(title.into()),
                )
                .children(trailing),
        )
        .child(body)
}

/// A label/value row, the label in the panel's label column.
pub fn row(label: impl Into<SharedString>, value: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .min_h(px(22.))
        .gap_2()
        .items_center()
        .text_xs()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .truncate()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// An empty state: what is missing, what to do, and maybe the one action that does it.
pub fn empty(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    action: Option<AnyElement>,
    cx: &App,
) -> Div {
    v_flex()
        .gap_1()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_dashed()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(title.into()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(detail.into()),
        )
        .when_some(action, |el, action| el.child(div().pt_2().child(action)))
}

/// Loading: grey bars where `rows` rows of data will be, at their height.
pub fn loading(rows: usize, label: impl Into<SharedString>, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(muted)
                .child(
                    gpui_kit::component::spinner::Spinner::new()
                        .xsmall()
                        .color(muted),
                )
                .child(label.into()),
        )
        .children((0..rows).map(|i| {
            let bar = cx.theme().muted;
            h_flex()
                .h(px(22.))
                .gap_2()
                .items_center()
                .child(div().size(px(16.)).flex_none().rounded(px(4.)).bg(bar))
                .child(div().h(px(10.)).rounded(px(3.)).bg(bar).w(px(
                    // Rows of different lengths, as data has.
                    [180., 140., 200., 120., 160.][i % 5],
                )))
        }))
}

/// A key cap, as the keymap writes keys: `⇧⌘.`.
pub fn key(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_none()
        .px_1()
        .min_w(px(18.))
        .h(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().foreground)
        .child(text.into())
}

/// A small pale tag beside a value: a weight, a kind, a status.
pub fn tag(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .h(px(18.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(cx.theme().secondary)
        .text_xs()
        .text_color(cx.theme().secondary_foreground)
        .child(text.into())
}

/// An icon in the muted colour, sized for a 12 px line.
pub fn glyph(icon: Lucide, cx: &App) -> Icon {
    Icon::new(icon)
        .xsmall()
        .text_color(cx.theme().muted_foreground)
}

impl BrowserView {
    /// The open panel.
    pub(super) fn side_panel(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let (title, body) = match panel {
            Panel::Annotate => ("Annotate", self.annotate.clone().into_any_element()),
            Panel::Design => ("Design", self.design_panel.clone().into_any_element()),
        };
        let close = cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.toggle_panel(panel, cx);
        });
        div()
            .w(px(PANEL_WIDTH))
            .h_full()
            .flex_none()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // A field in the panel has already taken focus.
                    if window.default_prevented() {
                        return;
                    }
                    if this.focus.contains_focused(window, cx) {
                        // GPUI focus is ours; the page may still hold the keys.
                        if let Some(engine) = &this.engine {
                            engine.focus_parent();
                        }
                    } else {
                        this.take_keyboard(window, cx);
                    }
                }),
            )
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(frame(title, body, close, cx))
            .into_any_element()
    }
}
