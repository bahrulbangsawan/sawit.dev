//! Preview: a screen full size and live over the canvas, at the size it was made for, the
//! window's, or one of the Browser tab's device sizes ([`DEVICES`]), turned on its side if asked.
//! It has a web view of its own, in the canvas's host, scaled down to fit when the size is larger
//! than the room (WebKit's view scale, so the page keeps its CSS viewport), and the keyboard.
//! Escape or Close ends it.

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
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

/// Preview's bar over the canvas, and the room under it where its page shows: the screen's name,
/// its sizes and Close. It takes every click over the canvas while it shows.
pub(in crate::board) fn render_preview(
    view: &BoardView,
    cx: &mut Context<BoardView>,
) -> Option<impl IntoElement + use<>> {
    let preview = view.screen_ui.preview.as_ref()?;
    let this = cx.entity().downgrade();
    let theme = cx.theme();
    let made = preview.made;
    let mut sizes = vec![(Size::Made, format!("As made · {} × {}", made.0, made.1))];
    sizes.push((Size::Window, "Window".to_string()));
    sizes.extend(DEVICES.iter().enumerate().map(|(i, d)| {
        (
            Size::Device(i),
            format!("{} · {} × {}", d.name, d.width, d.height),
        )
    }));
    let buttons = sizes.into_iter().enumerate().map(|(i, (size, label))| {
        let this = this.clone();
        let chosen = preview.size == size;
        let button = Button::new(("screen-preview-size", i))
            .small()
            .label(label.clone())
            .accessibility_label(label)
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.preview_size(Some(size), cx));
            });
        match chosen {
            true => button.primary(),
            false => button.ghost(),
        }
    });
    let buttons: Vec<Button> = buttons.collect();
    let rotate = {
        let this = this.clone();
        Button::new("screen-preview-rotate")
            .small()
            .ghost()
            .label("Rotate")
            .disabled(preview.size == Size::Window)
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |view, cx| view.preview_size(None, cx));
            })
    };
    let close = Button::new("screen-preview-close")
        .small()
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
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(
            div()
                .flex_shrink_0()
                .font_semibold()
                .child(format!("Preview · {}", preview.title)),
        )
        .child(
            h_flex()
                .id("screen-preview-sizes")
                .flex_1()
                .min_w_0()
                .gap_1()
                .overflow_x_scroll()
                .children(buttons)
                .child(rotate),
        )
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
