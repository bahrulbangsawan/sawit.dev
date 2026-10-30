//! The browser's toolbar: back, forward, reload, the address, the device
//! menu, the Annotate and Design toggles, open externally and Web Inspector.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, h_flex,
};
use gpui_kit::*;
use sawit_core::browser::{BrowserOp, DEVICES, ViewportMode};

use super::{BrowserView, Panel, TOOLBAR_HEIGHT, rotated};

impl BrowserView {
    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let loading = self.loading;
        let waiting = self.pick_waiting;
        let tip = |panel: Panel, text: &'static str| {
            if waiting && self.panel() == Some(panel) {
                "Waiting for the page…"
            } else {
                text
            }
        };
        let weak = cx.entity().downgrade();
        let mode = self.viewport.clone();
        h_flex()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("browser-back")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ChevronLeft)
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
            .child(div().flex_1().child(Input::new(&self.address).small()))
            .child(
                Button::new("browser-device")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Smartphone)
                    .tooltip("Device Size")
                    .dropdown_menu(move |menu, _, _| device_menu(menu, weak.clone(), &mode)),
            )
            .child(
                Button::new("browser-annotate")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Pencil)
                    .tooltip(tip(Panel::Annotate, "Annotate (⌘⇧.)"))
                    .selected(self.panel() == Some(Panel::Annotate))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_panel(Panel::Annotate, window, cx);
                    })),
            )
            .child(
                Button::new("browser-design")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Palette)
                    .tooltip(tip(Panel::Design, "Design"))
                    .selected(self.panel() == Some(Panel::Design))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_panel(Panel::Design, window, cx);
                    })),
            )
            .child(
                Button::new("browser-external")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::ExternalLink)
                    .tooltip("Open in Default Browser")
                    .disabled(self.current_url() == "about:blank")
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.open_url(&this.current_url());
                    })),
            )
            .child(
                Button::new("browser-inspect")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Code)
                    .tooltip("Web Inspector")
                    .on_click(cx.listener(|this, _, _, cx| {
                        drop(this.apply(BrowserOp::Devtools, cx));
                    })),
            )
    }
}

/// Responsive, the devices, and Rotate.
fn device_menu(
    menu: PopupMenu,
    view: WeakEntity<BrowserView>,
    current: &ViewportMode,
) -> PopupMenu {
    let set = |mode: ViewportMode| {
        let view = view.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let mode = mode.clone();
            let _ = view.update(cx, |this, cx| this.set_viewport(mode, cx));
        }
    };
    let mut menu = menu.item(
        PopupMenuItem::new("Responsive")
            .checked(*current == ViewportMode::Responsive)
            .on_click(set(ViewportMode::Responsive)),
    );
    for device in DEVICES {
        let mode = ViewportMode::Fixed {
            name: Some(device.name.to_string()),
            width: device.width,
            height: device.height,
        };
        menu = menu.item(
            PopupMenuItem::new(format!(
                "{} · {} × {}",
                device.name, device.width, device.height
            ))
            .checked(*current == mode)
            .on_click(set(mode)),
        );
    }
    let rotate = rotated(current);
    let mut item = PopupMenuItem::new("Rotate").disabled(rotate.is_none());
    if let Some(mode) = rotate {
        item = item.on_click(set(mode));
    }
    menu.separator().item(item)
}
