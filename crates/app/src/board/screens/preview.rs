//! Preview: a screen full size and live over the canvas, at the size it was made for, the
//! window's, or one of the Browser tab's device sizes ([`DEVICES`]), turned on its side if asked.
//! It has a web view of its own, in the canvas's host, scaled down to fit when the size is larger
//! than the room (WebKit's view scale, so the page keeps its CSS viewport), and the keyboard.
//! Escape or Close ends it.

use figcraft_doc::NodeId;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::screens;
use sawit_core::browser::{DEVICES, fit};

use crate::board::BoardView;
use crate::board::web::{INIT, WebMsg};
use crate::browser::engine::web_frame::ScreenSource;
use crate::browser::engine::{Engine, EngineSpec, Frame as ViewFrame};

/// The height of Preview's bar, and the room around the page under it.
pub(in crate::board) const BAR: f32 = 44.;
const PAD: f32 = 16.;

/// The size Preview shows a screen at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::board) enum Size {
    /// The size the screen was made at.
    Made,
    /// The room the canvas has.
    Window,
    /// One of [`DEVICES`].
    Device(usize),
}

/// The screen shown full size.
pub(in crate::board) struct Preview {
    pub frame: NodeId,
    pub title: String,
    pub made: (u32, u32),
    pub size: Size,
    pub rotated: bool,
    pub engine: Option<Engine>,
    pub placed: Option<(ViewFrame, f64)>,
}

impl Preview {
    /// The CSS size the page is laid out at, in a room `area` wide and high.
    pub fn css_size(&self, area: (f32, f32)) -> (u32, u32) {
        let (w, h) = match self.size {
            Size::Made => self.made,
            Size::Window => (
                area.0.floor().max(1.0) as u32,
                area.1.floor().max(1.0) as u32,
            ),
            Size::Device(i) => DEVICES.get(i).map_or(self.made, |d| (d.width, d.height)),
        };
        match self.rotated && self.size != Size::Window {
            true => (h, w),
            false => (w, h),
        }
    }

    /// Where the page goes in a canvas `canvas` wide and high, under the bar, and its scale.
    pub fn place(&self, canvas: (f32, f32)) -> (ViewFrame, f64) {
        let area = (
            (canvas.0 - 2.0 * PAD).max(1.0),
            (canvas.1 - BAR - 2.0 * PAD).max(1.0),
        );
        let fitted = fit(self.css_size(area), area);
        let frame = ViewFrame {
            x: PAD + ((area.0 - fitted.width) / 2.0).max(0.0),
            y: BAR + PAD + ((area.1 - fitted.height) / 2.0).max(0.0),
            width: fitted.width,
            height: fitted.height,
        };
        (frame, fitted.scale)
    }
}

impl BoardView {
    /// Whether a screen is in Preview.
    pub(in crate::board) fn preview_open(&self) -> bool {
        self.screen_ui.preview.is_some()
    }

    /// Shows screen frame `frame` in Preview, at the size it was made at.
    pub(in crate::board) fn open_preview(
        &mut self,
        frame: NodeId,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.web_exit(cx);
        self.close_preview(cx);
        let Some(found) = self.selected_screen().filter(|s| s.frame == frame) else {
            return;
        };
        let Some(web) = self.web.as_ref() else {
            return;
        };
        let to_view = web.to_view.clone();
        let name = found.data.screen.clone();
        let spec = EngineSpec {
            url: screens::url(&name),
            init_script: INIT.into(),
            data_store: None,
            visible: false,
            frame: ViewFrame {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            screen: Some(ScreenSource {
                folder: web.folder(&name),
                name,
            }),
        };
        let engine = Engine::build(
            spec,
            move |event| {
                let _ = to_view.unbounded_send(WebMsg::Preview(event));
            },
            window,
        );
        let engine = match engine {
            Ok(engine) => engine,
            Err(error) => {
                self.show_toast(format!("Preview couldn't start: {error}"), cx);
                return;
            }
        };
        if let Some(host) = web.host.as_ref() {
            engine.adopt_into(host);
        }
        // The user opened it: it shares the canvas's keyboard ([`engine::keyboard`]).
        engine.set_keyboard_host(Some(&self.focus));
        let made = found.data.viewport;
        self.screen_ui.preview = Some(Preview {
            frame,
            title: found.title,
            made: (
                made.w.round().max(1.0) as u32,
                made.h.round().max(1.0) as u32,
            ),
            size: Size::Made,
            rotated: false,
            engine: Some(engine),
            placed: None,
        });
        cx.notify();
    }

    /// Ends Preview: its view goes.
    pub(in crate::board) fn close_preview(&mut self, cx: &mut Context<Self>) {
        if self.screen_ui.preview.take().is_some() {
            self.wants_keyboard = true;
            cx.notify();
        }
    }

    /// Puts Preview's view where the canvas, `canvas` big, has room for it now.
    pub(in crate::board) fn place_preview(&mut self, canvas: (f32, f32)) {
        let Some(host) = self.web.as_ref().and_then(|web| web.host.as_ref()) else {
            return;
        };
        let Some(preview) = self.screen_ui.preview.as_mut() else {
            return;
        };
        let target = preview.place(canvas);
        let Some(engine) = preview.engine.as_ref() else {
            return;
        };
        if preview.placed == Some(target) {
            return;
        }
        let first = preview.placed.is_none();
        engine.place_in(host, target.0);
        if !engine.set_view_scale(target.1) {
            engine.set_zoom(target.1);
        }
        if first {
            engine.set_visible(true);
            engine.focus();
        }
        preview.placed = Some(target);
    }

    /// Shows Preview at `size`, or turns it on its side.
    fn preview_size(&mut self, size: Option<Size>, cx: &mut Context<Self>) {
        if let Some(preview) = self.screen_ui.preview.as_mut() {
            match size {
                Some(size) => preview.size = size,
                None => preview.rotated = !preview.rotated,
            }
            preview.placed = None;
        }
        cx.notify();
    }
}

/// The group a device belongs to in Preview's size menu, by its width.
fn group_of(width: u32) -> &'static str {
    match width {
        0..600 => "Mobile",
        600..1100 => "Tablet",
        1100..1500 => "Laptop",
        _ => "Desktop",
    }
}

/// The picture of a Preview size: a phone, a tablet, a laptop or a screen for a device, the
/// frame for the size it was made at, and a window for the window's.
fn size_icon(size: Size) -> Lucide {
    match size {
        Size::Made => Lucide::Frame,
        Size::Window => Lucide::AppWindow,
        Size::Device(i) => match DEVICES.get(i).map(|d| d.name) {
            Some(name) if name.starts_with("iPad") => Lucide::Tablet,
            Some("Laptop") => Lucide::Laptop,
            Some("Desktop") => Lucide::Monitor,
            _ => Lucide::Smartphone,
        },
    }
}

/// Preview's bar over the canvas, and the room under it where its page shows: the screen's name,
/// its size (a menu of the sizes grouped by kind, the one shown checked), Rotate, Reload and
/// Close. It takes every click
/// over the canvas while it shows.
pub(in crate::board) fn render_preview(
    view: &BoardView,
    cx: &mut Context<BoardView>,
) -> Option<impl IntoElement + use<>> {
    let preview = view.screen_ui.preview.as_ref()?;
    let this = cx.entity().downgrade();
    let theme = cx.theme();
    let made = preview.made;
    let mut sizes = vec![(Size::Made, format!("As Made · {} × {}", made.0, made.1))];
    sizes.push((Size::Window, "Window".to_string()));
    sizes.extend(DEVICES.iter().enumerate().map(|(i, d)| {
        (
            Size::Device(i),
            format!("{} · {} × {}", d.name, d.width, d.height),
        )
    }));
    let chosen = preview.size;
    let shown = sizes
        .iter()
        .find(|(size, _)| *size == chosen)
        .map_or_else(String::new, |(_, label)| label.clone());
    let size_menu = {
        let this = this.clone();
        Button::new("screen-preview-size")
            .small()
            .ghost()
            .icon(Icon::new(size_icon(chosen)).text_color(theme.muted_foreground))
            .label(shown.clone())
            .dropdown_caret(true)
            .accessibility_label(format!("Size: {shown}"))
            .tooltip("Choose the size to preview at")
            .dropdown_menu(move |mut menu, _, _| {
                // As Made and Window, then the devices grouped Mobile, Tablet, Laptop, Desktop
                // by their width (B15).
                let mut group = "";
                for (size, label) in sizes.iter().cloned() {
                    let this = this.clone();
                    if let Size::Device(i) = size {
                        let of = DEVICES.get(i).map_or("", |d| group_of(d.width));
                        if of != group {
                            menu = menu.separator().label(of);
                            group = of;
                        }
                    }
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .icon(size_icon(size))
                            .checked(size == chosen)
                            .on_click(move |_, _, cx| {
                                let _ =
                                    this.update(cx, |view, cx| view.preview_size(Some(size), cx));
                            }),
                    );
                }
                menu
            })
    };
    let rotate = {
        let this = this.clone();
        Button::new("screen-preview-rotate")
            .small()
            .ghost()
            .icon(Icon::new(Lucide::RotateCw).text_color(theme.muted_foreground))
            .label("Rotate")
            .toggled(preview.rotated)
            .selected(preview.rotated)
            .disabled(preview.size == Size::Window)
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.preview_size(None, cx));
            })
    };
    let reload = {
        let this = this.clone();
        Button::new("screen-preview-reload")
            .small()
            .ghost()
            .icon(Icon::new(Lucide::RefreshCw).text_color(theme.muted_foreground))
            .accessibility_label("Reload")
            .tooltip("Reload the page")
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, _| {
                    if let Some(engine) = view
                        .screen_ui
                        .preview
                        .as_ref()
                        .and_then(|p| p.engine.as_ref())
                    {
                        engine.reload();
                    }
                });
            })
    };
    let close = Button::new("screen-preview-close")
        .small()
        .icon(crate::menu_icons::CLOSE)
        .label("Close")
        .tooltip("Close Preview (Escape)")
        .on_click(move |_, _, cx| {
            let _ = this.update(cx, |view, cx| view.close_preview(cx));
        });
    let bar = h_flex()
        .h(px(BAR))
        .flex_shrink_0()
        .px_3()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child({
            // Cut short in a narrow canvas: the whole title is in its tooltip.
            let title = SharedString::from(format!("Preview · {}", preview.title));
            let tip = title.clone();
            div()
                .id("screen-preview-title")
                .flex_1()
                .min_w_0()
                .truncate()
                .font_semibold()
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .child(title)
        })
        .child(size_menu)
        .child(rotate)
        .child(reload)
        .child(div().w(px(1.)).h(px(16.)).mx_1().bg(theme.border))
        .child(close);
    Some(
        v_flex()
            .id("screen-preview")
            .debug_selector(|| "screen-preview".into())
            .occlude()
            .absolute()
            .inset_0()
            .bg(theme.muted)
            .child(bar)
            .child(div().flex_1()),
    )
}
