//! How the browser tab draws: the toolbar, the capture bar, the page area (with the device's
//! caption and backdrop) and the side panel.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::browser::ViewportMode;

use super::viewport::{CAPTION_BAND, caption};
use super::{BrowserView, ToggleAnnotate};
use crate::states::Placeholder;

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(error) = &self.error {
            return Placeholder::error("The browser couldn't start")
                .detail(error.clone())
                .into_any_element();
        }
        let url = self.current_url();
        self.drawn_url = Some(url.clone());
        let this = cx.entity().downgrade();
        let device = match &self.viewport {
            ViewportMode::Fixed {
                name,
                width,
                height,
            } => Some(caption(name.as_deref(), *width, *height, self.scale())),
            ViewportMode::Responsive => None,
        };
        let frozen = self.frozen_paint();
        let page = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            // The page hidden under the device menu leaves the same backdrop as a device's.
            .when(
                device.is_some() || self.menu_open || self.frozen.hidden,
                |d| d.bg(cx.theme().muted),
            )
            .children(device.map(|text| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(CAPTION_BAND))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text)
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = this.update(cx, |this, cx| this.place(bounds, window, cx));
                    },
                    move |bounds, _, window, _| frozen.paint(bounds, window),
                )
                .size_full(),
            )
            .child(Self::cover_watch(cx));
        let side = self.panel.map(|panel| self.side_panel(panel, cx));
        v_flex()
            .key_context(self.mesurer_key_context(window))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleAnnotate, _, cx| {
                this.annotate_key(cx);
            }))
            .map(|el| Self::with_mesurer_keys(el, cx))
            .size_full()
            .child(self.toolbar(&url, window, cx))
            .children(self.capture_bar(cx))
            .map(|el| self.with_find(el, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(page)
                    .children(side),
            )
            .into_any_element()
    }
}
