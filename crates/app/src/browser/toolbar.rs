//! The browser's toolbar: back, forward, reload, the address, Local Explorer while the page's dev
//! server serves one, the device menu, the Annotate and Design toggles, open externally and Web
//! Inspector.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::browser::{BrowserOp, DEVICES, ViewportMode};

use super::viewport::{caption, device_of, rotated};
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
            .when_some(explorer, |el, (port, on_explorer)| {
                el.child(explorer_button(port, on_explorer, cx))
            })
            .child(device_button(&self.viewport, weak))
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

/// The device menu's button: the device the page imitates, by its icon and name (a size of no
/// device's by its width and height), or Responsive's icon alone. The menu opens under it,
/// leftwards, as the button sits near the window's right edge. While it is open the page is
/// hidden ([`BrowserView::set_menu_open`]), or the menu would open unseen behind it.
pub(super) fn device_button(
    mode: &ViewportMode,
    view: WeakEntity<BrowserView>,
) -> impl IntoElement + use<> {
    let opened = view.clone();
    let current = mode.clone();
    Button::new("browser-device")
        .debug_selector(|| "browser-device".into())
        .ghost()
        .xsmall()
        .icon(device_icon(mode))
        .when_some(device_label(mode), |button, label| button.label(label))
        // Its icon alone says nothing to a screen reader in responsive mode: its name is the
        // tooltip's, which names the mode or the device and its size.
        .accessibility_label(device_tooltip(mode))
        .tooltip(device_tooltip(mode))
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            device_menu(menu, view.clone(), &current)
        })
        .on_open_change(move |open, _, cx| {
            let _ = opened.update(cx, |this, cx| this.set_menu_open(*open, cx));
        })
}

/// What the device button says beside its icon: the device's name, or the size when no device
/// has it; nothing in responsive mode.
fn device_label(mode: &ViewportMode) -> Option<String> {
    match mode {
        ViewportMode::Responsive => None,
        ViewportMode::Fixed {
            name: Some(name), ..
        } => Some(name.clone()),
        ViewportMode::Fixed {
            name: None,
            width,
            height,
        } => Some(format!("{width} × {height}")),
    }
}

/// "Device Size: iPhone 15 · 393 × 852", or "Device Size: Responsive".
fn device_tooltip(mode: &ViewportMode) -> String {
    match mode {
        ViewportMode::Responsive => "Device Size: Responsive".into(),
        ViewportMode::Fixed {
            name,
            width,
            height,
        } => format!(
            "Device Size: {}",
            caption(name.as_deref(), *width, *height, 1.)
        ),
    }
}

/// A phone, a tablet, a laptop or a desktop screen for a device; phone and screen together for
/// responsive mode.
fn device_icon(mode: &ViewportMode) -> Lucide {
    let Some((device, _)) = device_of(mode) else {
        return match mode {
            ViewportMode::Responsive => Lucide::MonitorSmartphone,
            ViewportMode::Fixed { .. } => Lucide::Proportions,
        };
    };
    match device.name {
        name if name.starts_with("iPad") => Lucide::Tablet,
        "Laptop" => Lucide::Laptop,
        "Desktop" => Lucide::Monitor,
        _ => Lucide::Smartphone,
    }
}

/// Which of the menu's items have a check for `mode`: Responsive, the device it imitates in
/// either orientation (by its place in [`DEVICES`]), and Rotate when that device is on its side.
fn device_checks(mode: &ViewportMode) -> (bool, Option<usize>, bool) {
    let device = device_of(mode);
    (
        *mode == ViewportMode::Responsive,
        device.and_then(|(d, _)| DEVICES.iter().position(|each| each.name == d.name)),
        device.is_some_and(|(_, sideways)| sideways),
    )
}

/// Responsive, the devices, and Rotate.
fn device_menu(
    menu: PopupMenu,
    view: WeakEntity<BrowserView>,
    current: &ViewportMode,
) -> PopupMenu {
    let (responsive, device_ix, sideways) = device_checks(current);
    let set = |mode: ViewportMode| {
        let view = view.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let mode = mode.clone();
            let _ = view.update(cx, |this, cx| this.set_viewport(mode, cx));
        }
    };
    let mut menu = menu.item(
        PopupMenuItem::new("Responsive")
            .checked(responsive)
            .on_click(set(ViewportMode::Responsive)),
    );
    for (ix, device) in DEVICES.iter().enumerate() {
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
            .checked(device_ix == Some(ix))
            .on_click(set(mode)),
        );
    }
    let rotate = rotated(current);
    let mut item = PopupMenuItem::new("Rotate")
        .checked(sideways)
        .disabled(rotate.is_none());
    if let Some(mode) = rotate {
        item = item.on_click(set(mode));
    }
    menu.separator().item(item)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
        TestAppContext, VisualTestContext, WeakEntity, Window, div, px, size,
    };
    use sawit_core::browser::ViewportMode;

    use super::super::BrowserView;
    use super::{device_button, device_checks, device_label, device_tooltip};

    fn fixed(name: Option<&str>, width: u32, height: u32) -> ViewportMode {
        ViewportMode::Fixed {
            name: name.map(str::to_string),
            width,
            height,
        }
    }

    /// The menu checks the device the page imitates whichever way up it is, with Rotate checked
    /// while it is on its side; Responsive alone in responsive mode, and nothing for a size no
    /// device has.
    #[test]
    fn the_menu_checks_the_device_either_way_up() {
        assert_eq!(
            device_checks(&ViewportMode::Responsive),
            (true, None, false)
        );
        assert_eq!(
            device_checks(&fixed(Some("iPhone 15"), 393, 852)),
            (false, Some(0), false)
        );
        assert_eq!(
            device_checks(&fixed(Some("iPhone 15"), 852, 393)),
            (false, Some(0), true)
        );
        // Laptop is wider than tall: turned, it stands up.
        assert_eq!(
            device_checks(&fixed(Some("Laptop"), 800, 1280)),
            (false, Some(4), true)
        );
        assert_eq!(
            device_checks(&fixed(Some("iPad Pro 11″"), 834, 1194)),
            (false, Some(3), false)
        );
        assert_eq!(device_checks(&fixed(None, 500, 700)), (false, None, false));
    }

    /// The button says which device the page imitates, or its size; its tooltip says the mode in
    /// words, responsive included.
    #[test]
    fn the_button_says_the_device_in_use() {
        assert_eq!(device_label(&ViewportMode::Responsive), None);
        assert_eq!(
            device_tooltip(&ViewportMode::Responsive),
            "Device Size: Responsive"
        );
        let phone = fixed(Some("iPhone 15"), 393, 852);
        assert_eq!(device_label(&phone).as_deref(), Some("iPhone 15"));
        assert_eq!(device_tooltip(&phone), "Device Size: iPhone 15 · 393 × 852");
        let sideways = fixed(Some("iPhone 15"), 852, 393);
        assert_eq!(
            device_tooltip(&sideways),
            "Device Size: iPhone 15 · 852 × 393"
        );
        let custom = fixed(None, 500, 700);
        assert_eq!(device_label(&custom).as_deref(), Some("500 × 700"));
        assert_eq!(device_tooltip(&custom), "Device Size: 500 × 700");
    }

    /// The device button at the right end of a toolbar, as the browser draws it.
    struct Toolbar {
        browser: WeakEntity<BrowserView>,
    }

    impl Render for Toolbar {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                gpui_kit::component::h_flex()
                    .w_full()
                    .h(px(40.))
                    .justify_end()
                    .child(device_button(
                        &ViewportMode::Responsive,
                        self.browser.clone(),
                    )),
            )
        }
    }

    fn page_shown(browser: &Entity<BrowserView>, cx: &mut VisualTestContext) -> bool {
        cx.update(|_, cx| browser.read(cx).page_shown())
    }

    /// The webview draws above GPUI, the device menu too, and the menu opens over the page: the
    /// page is hidden while the menu is open, and shown again once it closes. A menu left open
    /// when the tab goes off screen leaves the page hidden no longer.
    #[gpui_kit::test]
    fn the_page_makes_way_for_the_device_menu(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::super::init(cx);
        });
        let made = std::rc::Rc::new(std::cell::RefCell::new(None));
        let window = cx.open_window(size(px(480.), px(400.)), {
            let made = made.clone();
            move |window, cx| {
                let browser = cx.new(|cx| BrowserView::new(None, None, window, cx));
                browser.update(cx, |b, _| b.set_visible(true));
                let toolbar = Toolbar {
                    browser: browser.downgrade(),
                };
                *made.borrow_mut() = Some(browser);
                toolbar
            }
        });
        let browser: Entity<BrowserView> = made.borrow_mut().take().unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        assert!(page_shown(&browser, &mut cx));

        let button = cx.debug_bounds("browser-device").expect("no device button");
        cx.simulate_click(button.center(), Default::default());
        cx.run_until_parked();
        assert!(
            !page_shown(&browser, &mut cx),
            "the menu opened behind the page"
        );

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(
            page_shown(&browser, &mut cx),
            "the menu closed, the page stayed hidden"
        );

        cx.simulate_click(button.center(), Default::default());
        cx.run_until_parked();
        assert!(!page_shown(&browser, &mut cx));
        cx.update(|_, cx| browser.update(cx, |b, _| b.set_visible(false)));
        cx.update(|_, cx| browser.update(cx, |b, _| b.set_visible(true)));
        assert!(
            page_shown(&browser, &mut cx),
            "the tab came back to a hidden page"
        );
    }
}
