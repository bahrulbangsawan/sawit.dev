//! The toast: a message at the bottom of the canvas, or just above the toolbar while that shows,
//! up for a while ([`TOAST_TIME`]).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{BoardView, TOAST_TIME, toolbar};

/// How far the toast stays off the canvas's bottom edge.
const TOAST_BOTTOM: f32 = 16.;
/// How far the toast stays off the toolbar while that shows (ruling P3-C10).
const TOAST_OVER_TOOLBAR: f32 = 8.;
/// A toast longer than this (an error naming a path) gets a Copy button and stays up longer.
const LONG_TOAST: usize = 120;

/// How long a toast of `text` stays up: a long one gets time to be read.
pub(super) fn toast_time(text: &str) -> std::time::Duration {
    if text.chars().count() > LONG_TOAST {
        TOAST_TIME * 4
    } else {
        TOAST_TIME
    }
}

impl BoardView {
    /// Shows `text` at the bottom of the canvas for [`TOAST_TIME`], in place of any toast up.
    pub(super) fn show_toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.toast_timer = Some(hide_toast_after(toast_time(&text), cx));
        self.toast = Some(text);
        cx.notify();
    }

    /// The toast at the bottom of the canvas, or just above the toolbar while that shows (ruling
    /// P3-C10).
    pub(super) fn render_toast(
        &self,
        above_toolbar: bool,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        let text = self.toast.clone()?;
        let bottom = if above_toolbar {
            toolbar::BOTTOM + toolbar::HEIGHT + TOAST_OVER_TOOLBAR
        } else {
            TOAST_BOTTOM
        };
        let theme = cx.theme();
        Some(
            div()
                .absolute()
                .bottom(px(bottom))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("board-toast")
                        .role(Role::Status)
                        .aria_label(text.clone())
                        .debug_selector(|| "board-toast".into())
                        .max_w(px(560.))
                        .flex()
                        .items_start()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .text_xs()
                        .shadow_md()
                        // Five lines, then the rest scrolls; a long one can be copied whole.
                        .child(
                            div()
                                .id("board-toast-text")
                                .flex_1()
                                .min_w_0()
                                .max_h(px(80.))
                                .overflow_y_scroll()
                                .child(text.clone()),
                        )
                        .when(text.chars().count() > LONG_TOAST, |el| {
                            el.child(
                                Button::new("board-toast-copy")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Copy)
                                    .tooltip("Copy")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            text.to_string(),
                                        ))
                                    }),
                            )
                        }),
                ),
        )
    }
}

/// Takes the toast down after [`TOAST_TIME`]. Dropping the task leaves it up.
pub(super) fn hide_toast_later(cx: &mut Context<BoardView>) -> Task<()> {
    hide_toast_after(TOAST_TIME, cx)
}

/// Takes the toast down after `time`. Dropping the task leaves it up.
fn hide_toast_after(time: std::time::Duration, cx: &mut Context<BoardView>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(time).await;
        let _ = this.update(cx, |this, cx| {
            this.toast = None;
            cx.notify();
        });
    })
}
