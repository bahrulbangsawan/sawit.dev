//! The browser's toolbar: back, forward, reload (Stop and a progress bar while the page loads,
//! [`super::progress`]), the address, the local servers' list ([`super::servers`]), Local
//! Explorer while the page's dev server serves one, the device menu ([`super::device`]), the
//! Annotate, Design and SEO toggles, open externally and Web Inspector.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::browser::BrowserOp;

use super::device::{device_button, responsive_button};
use super::{BrowserView, Panel, TOOLBAR_HEIGHT};

impl BrowserView {
    /// The toolbar, for the page WebKit has at `url` as this frame is drawn.
    pub(super) fn toolbar(
        &self,
        url: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.loading;
        let waiting = self.pick_waiting;
        let tip = |panel: Panel, text: SharedString| {
            if waiting && self.panel() == Some(panel) {
                "Waiting for the page…".into()
            } else {
                text
            }
        };
        let weak = cx.entity().downgrade();
        let explorer = crate::local_explorer::for_page(url, cx)
            .map(|(e, origin)| (e.port, origin.on_explorer));
        h_flex()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .relative()
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("browser-back")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ChevronLeft)
                    .accessibility_label("Back")
                    .tooltip("Back")
                    .on_click(cx.listener(|this, _, _, cx| {
                        drop(this.apply(BrowserOp::Back, cx));
                    })),
            )
            .child(
                Button::new("browser-forward")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ChevronRight)
                    .accessibility_label("Forward")
                    .tooltip("Forward")
                    .on_click(cx.listener(|this, _, _, cx| {
                        drop(this.apply(BrowserOp::Forward, cx));
                    })),
            )
            .child(
                Button::new("browser-reload")
                    .ghost()
                    .xsmall()
                    .icon(if loading { Lucide::X } else { Lucide::RotateCw })
                    .accessibility_label(if loading { "Stop" } else { "Reload" })
                    .tooltip(if loading { "Stop" } else { "Reload" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if loading {
                            if let Some(engine) = &this.engine {
                                engine.eval("window.stop()");
                            }
                            this.loading = false;
                            cx.notify();
                        } else {
                            drop(this.apply(BrowserOp::Reload, cx));
                        }
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .relative()
                    .child(Input::new(&self.address).small())
                    .children(self.render_agent_opening())
                    .child(self.render_agent_status()),
            )
            .child(self.servers_button(cx))
            .when_some(explorer, |el, (port, on_explorer)| {
                el.child(explorer_button(port, on_explorer, cx))
            })
            .child(device_button(&self.viewport, weak.clone()))
            .children(responsive_button(&self.viewport, weak))
            .child(self.capture_button(cx))
            .child(
                Button::new("browser-annotate")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Pencil)
                    .accessibility_label("Annotate")
                    .toggled(self.panel() == Some(Panel::Annotate))
                    .tooltip(tip(Panel::Annotate, annotate_tooltip(window)))
                    .selected(self.panel() == Some(Panel::Annotate))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.take_keyboard(window, cx);
                        this.toggle_panel(Panel::Annotate, cx);
                    })),
            )
            .child(
                Button::new("browser-design")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Palette)
                    .accessibility_label("Design")
                    .toggled(self.panel() == Some(Panel::Design))
                    .tooltip(tip(Panel::Design, "Design".into()))
                    .selected(self.panel() == Some(Panel::Design))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.take_keyboard(window, cx);
                        this.toggle_panel(Panel::Design, cx);
                    })),
            )
            .child(
                Button::new("browser-seo")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::SearchCheck)
                    .accessibility_label("SEO")
                    .toggled(self.panel() == Some(Panel::Seo))
                    .tooltip("SEO: search, sharing and agent checks")
                    .selected(self.panel() == Some(Panel::Seo))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.take_keyboard(window, cx);
                        this.toggle_panel(Panel::Seo, cx);
                    })),
            )
            .child(
                Button::new("browser-agent-cursors")
                    .ghost()
                    .xsmall()
                    .icon(crate::agents::TOGGLE_ICON)
                    .accessibility_label("Agent cursors")
                    .toggled(crate::agents::shown(cx))
                    .selected(crate::agents::shown(cx))
                    .tooltip(crate::agents::toggle_tooltip(window, cx))
                    .on_click(|_, _, cx| crate::agents::toggle(cx)),
            )
            .child(
                Button::new("browser-external")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ExternalLink)
                    .accessibility_label("Open in Default Browser")
                    .tooltip("Open in Default Browser")
                    .disabled(url == "about:blank")
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.open_url(&this.current_url());
                    })),
            )
            .child(
                Button::new("browser-inspect")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Code)
                    .accessibility_label("Web Inspector")
                    .tooltip("Web Inspector")
                    .on_click(cx.listener(|this, _, _, cx| {
                        drop(this.apply(BrowserOp::Devtools, cx));
                    })),
            )
            .children(self.progress_bar(cx))
    }
}

/// The Explorer button, shown while the page's dev server serves Cloudflare's Local Explorer
/// (`crate::local_explorer`): it opens the explorer, and on the explorer it is pressed and goes
/// back to the app the dev server serves.
fn explorer_button(
    port: u16,
    on_explorer: bool,
    cx: &mut Context<BrowserView>,
) -> impl IntoElement + use<> {
    let (tip, url) = if on_explorer {
        (
            "Back to the App".to_string(),
            format!("http://localhost:{port}/"),
        )
    } else {
        (
            "Local Explorer: this dev server's KV, R2, D1, Durable Objects and Workflows"
                .to_string(),
            sawit_core::cloudflare::explorer::page_url(port),
        )
    };
    Button::new("browser-explorer")
        .ghost()
        .xsmall()
        .child(crate::local_explorer::logo(14.))
        .child("Explorer")
        .accessibility_label("Local Explorer")
        .toggled(on_explorer)
        .selected(on_explorer)
        .tooltip(tip)
        .on_click(cx.listener(move |this, _, _, cx| this.navigate(&url, cx)))
}

/// The Annotate button's tooltip, with the key that toggles Annotate as the keymap is now
/// (`Annotate and measure (⇧⌘.)`: Annotate brings mesurer's measuring tools); its name alone
/// when no key does.
pub(super) fn annotate_tooltip(window: &Window) -> SharedString {
    match crate::keymap::keys_for(&super::ToggleAnnotate, "Browser", window) {
        keys if keys.is_empty() => "Annotate and measure".into(),
        keys => format!("Annotate and measure ({keys})").into(),
    }
}
