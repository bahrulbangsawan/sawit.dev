//! The side panel beside the page: a title, a close button and a body that
//! scrolls. Annotate and Design put their content in [`frame`].

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::*;

use super::{BrowserView, Panel};

/// The panel's width, on the right of the page.
pub const PANEL_WIDTH: f32 = 340.;

/// `title` over a scrolling `body`, with a close button in the header.
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
                .px_3()
                .gap_1()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.to_string()),
                )
                .child(
                    Button::new("browser-panel-close")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .tooltip("Close")
                        .on_click(on_close),
                ),
        )
        .child(
            div()
                .id("browser-panel-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .child(body),
        )
}

impl BrowserView {
    /// The open panel. Plans for Annotate and Design replace the bodies.
    pub(super) fn side_panel(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let (title, text) = match panel {
            Panel::Annotate => ("Annotate", "Click an element on the page to annotate it."),
            Panel::Design => (
                "Design",
                "Inspect, colors, fonts, assets and the design system appear here.",
            ),
        };
        let body = div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text);
        let close = cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.toggle_panel(panel, window, cx);
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
            .child(frame(title, body, close, cx))
            .into_any_element()
    }
}
