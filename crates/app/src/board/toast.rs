//! The toast: a message at the bottom of the canvas, or just above the toolbar while that shows,
//! up for a while ([`TOAST_TIME`]).

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

use super::{BoardView, TOAST_TIME, toolbar};

/// How far the toast stays off the canvas's bottom edge.
const TOAST_BOTTOM: f32 = 16.;
/// How far the toast stays off the toolbar while that shows (ruling P3-C10).
const TOAST_OVER_TOOLBAR: f32 = 8.;

impl BoardView {
    /// Shows `text` at the bottom of the canvas for [`TOAST_TIME`], in place of any toast up.
    pub(super) fn show_toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(text.into());
        self.toast_timer = Some(hide_toast_later(cx));
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
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .text_xs()
                        .shadow_md()
                        .child(text),
                ),
        )
    }
}

/// Takes the toast down after [`TOAST_TIME`]. Dropping the task leaves it up.
pub(super) fn hide_toast_later(cx: &mut Context<BoardView>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(TOAST_TIME).await;
        let _ = this.update(cx, |this, cx| {
            this.toast = None;
            cx.notify();
        });
    })
}
