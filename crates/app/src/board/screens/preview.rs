//! Preview: a screen full size and live over the canvas, at the size it was made at, the
//! window's, one of the device sizes ([`DEVICES`], D1's presets) or a custom size, turned on its
//! side if asked. It has a web view of its own, in the canvas's host, and the keyboard. It shows
//! the page fitted to the room (scaled down when it is larger, never up) or at 100% (one CSS px a
//! point; a page taller than the room scrolls inside it), optionally in a device frame. Escape or
//! Close ends it. Its bar is [`super::preview_bar`].

use figcraft_doc::NodeId;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_board::screens;
use sawit_core::browser::{DEVICES, DeviceClass, Fit, fit};

use crate::board::BoardView;
use crate::board::web::{INIT, WebMsg};
use crate::browser::engine::web_frame::ScreenSource;
use crate::browser::engine::{Engine, EngineSpec, Frame as ViewFrame};

/// The height of Preview's bar, and the room around the page under it.
pub(in crate::board) const BAR: f32 = 44.;
pub(in crate::board) const PAD: f32 = 24.;
/// A device frame's bezel around the page, and its corners.
pub(in crate::board) const BEZEL: f32 = 12.;
/// The room a custom size's error note takes under the bar.
pub(in crate::board) const NOTE: f32 = 36.;
pub(in crate::board) const BEZEL_RADIUS: f32 = 32.;
/// The sizes Custom takes, in CSS px.
pub(in crate::board) const CUSTOM_MIN: u32 = 200;
pub(in crate::board) const CUSTOM_MAX: u32 = 3840;

/// The size Preview shows a screen at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::board) enum Size {
    /// The size the screen was made at.
    Made,
    /// The room the canvas has.
    Window,
    /// One of [`DEVICES`].
    Device(usize),
    /// A width and height the user typed.
    Custom(u32, u32),
}

/// How large the page shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::board) enum Zoom {
    /// As large as fits the room, never larger than 100%.
    #[default]
    Fit,
    /// One CSS px a point.
    Actual,
}

/// Custom size's fields while they are typed in.
pub(in crate::board) struct CustomEdit {
    pub w: Entity<InputState>,
    pub h: Entity<InputState>,
    pub error: Option<String>,
    _events: [Subscription; 2],
}

/// The screen shown full size.
pub(in crate::board) struct Preview {
    pub frame: NodeId,
    pub title: String,
    pub made: (u32, u32),
    pub size: Size,
    pub rotated: bool,
    pub zoom: Zoom,
    /// The device frame is asked for.
    pub framed: bool,
    pub custom: Option<CustomEdit>,
    pub engine: Option<Engine>,
    pub placed: Option<(ViewFrame, f64)>,
}

impl Preview {
    pub fn new(frame: NodeId, title: String, made: (u32, u32), engine: Option<Engine>) -> Self {
        Preview {
            frame,
            title,
            made,
            size: Size::Made,
            rotated: false,
            zoom: Zoom::Fit,
            framed: false,
            custom: None,
            engine,
            placed: None,
        }
    }

    /// The CSS size the page is laid out at, in a room `area` wide and high.
    pub fn css_size(&self, area: (f32, f32)) -> (u32, u32) {
        let (w, h) = match self.size {
            Size::Made => self.made,
            Size::Window => (
                area.0.floor().max(1.0) as u32,
                area.1.floor().max(1.0) as u32,
            ),
            Size::Device(i) => DEVICES.get(i).map_or(self.made, |d| (d.width, d.height)),
            Size::Custom(w, h) => (w, h),
        };
        match self.rotated && self.size != Size::Window {
            true => (h, w),
            false => (w, h),
        }
    }

    /// Whether the size is a phone's or a tablet's, which a device frame fits.
    pub fn frame_fits(&self) -> bool {
        if self.size == Size::Window {
            return false;
        }
        if let Size::Device(i) = self.size {
            return DEVICES
                .get(i)
                .is_some_and(|d| matches!(d.class, DeviceClass::Mobile | DeviceClass::Tablet));
        }
        // A size of its own: a phone's or a tablet's when its longer side is a tablet's at most.
        let (w, h) = self.css_size((1.0, 1.0));
        w.max(h) <= 1200
    }

    /// Whether the device frame is drawn: asked for, and it fits the size.
    pub fn frame_shown(&self) -> bool {
        self.framed && self.frame_fits()
    }

    /// The room under the bar in a canvas `canvas` big, less the device frame's bezel.
    pub fn area(&self, canvas: (f32, f32)) -> (f32, f32) {
        let bezel = if self.frame_shown() { 2.0 * BEZEL } else { 0.0 };
        (
            (canvas.0 - 2.0 * PAD - bezel).max(1.0),
            (canvas.1 - BAR - 2.0 * PAD - bezel - self.note_room()).max(1.0),
        )
    }

    /// The room under the bar a custom size's error takes, so the page never covers it.
    pub fn note_room(&self) -> f32 {
        match self.custom.as_ref().is_some_and(|c| c.error.is_some()) {
            true => NOTE,
            false => 0.0,
        }
    }

    /// Whether 100% fits a canvas `canvas` big across (a taller page scrolls inside the room).
    pub fn actual_fits(&self, canvas: (f32, f32)) -> bool {
        let area = self.area(canvas);
        self.css_size(area).0 as f32 <= area.0
    }

    /// Where the page goes in a canvas `canvas` wide and high, under the bar, and its scale.
    pub fn place(&self, canvas: (f32, f32)) -> (ViewFrame, f64) {
        let area = self.area(canvas);
        let size = self.css_size(area);
        let fitted = match self.zoom == Zoom::Actual && self.actual_fits(canvas) {
            // 100%: the page's own width; the room's height when the page is taller.
            true => Fit {
                scale: 1.0,
                width: size.0 as f32,
                height: (size.1 as f32).min(area.1.floor()),
            },
            false => fit(size, area),
        };
        let bezel = if self.frame_shown() { BEZEL } else { 0.0 };
        let frame = ViewFrame {
            x: PAD + bezel + ((area.0 - fitted.width) / 2.0).max(0.0).floor(),
            y: BAR
                + self.note_room()
                + PAD
                + bezel
                + ((area.1 - fitted.height) / 2.0).max(0.0).floor(),
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
        let found = (self.board().and_then(|b| b.session.ds().ok())).and_then(|ds| {
            let node = ds.doc.node(frame)?;
            Some((screens::screen_of(node)?, node.name.clone()))
        });
        let Some((data, title)) = found else {
            return;
        };
        let Some(web) = self.web.as_ref() else {
            return;
        };
        let to_view = web.to_view.clone();
        let name = data.screen.clone();
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
        let made = (
            data.viewport.w.round().max(1.0) as u32,
            data.viewport.h.round().max(1.0) as u32,
        );
        self.screen_ui.preview = Some(Preview::new(frame, title, made, Some(engine)));
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
            if preview.custom.is_none() {
                engine.focus();
            }
        }
        preview.placed = Some(target);
    }

    /// Changes Preview with `change`, and places its page again.
    pub(in crate::board) fn preview_change(
        &mut self,
        change: impl FnOnce(&mut Preview),
        cx: &mut Context<Self>,
    ) {
        if let Some(preview) = self.screen_ui.preview.as_mut() {
            change(preview);
            preview.placed = None;
        }
        cx.notify();
    }

    /// Reloads Preview's page.
    pub(in crate::board) fn preview_reload(&self) {
        if let Some(engine) = (self.screen_ui.preview.as_ref()).and_then(|p| p.engine.as_ref()) {
            engine.reload();
        }
    }

    /// Opens Preview's screen in the project's Browser tab, from its folder's `index.html`.
    pub(in crate::board) fn preview_in_browser(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(frame) = self.screen_ui.preview.as_ref().map(|p| p.frame) else {
            return;
        };
        let name = (self.board().and_then(|b| b.session.ds().ok()))
            .and_then(|ds| screens::screen_of(ds.doc.node(frame)?))
            .map(|d| d.screen);
        let Some(name) = name else {
            return;
        };
        let page = screens::folder(&self.screens_dir(), &name).join(screens::INDEX);
        let url = format!("file://{}", page.display());
        self.close_preview(cx);
        self.open_in_browser(url, window, cx);
    }

    /// Custom size: its two fields, filled with the size shown, the width's focused.
    pub(in crate::board) fn preview_custom_start(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = self.screen_ui.preview.as_ref() else {
            return;
        };
        let (w, h) = preview.css_size((1.0, 1.0));
        let wi = cx.new(|cx| InputState::new(window, cx).default_value(w.to_string()));
        let hi = cx.new(|cx| InputState::new(window, cx).default_value(h.to_string()));
        let enter = |view: &mut Self,
                     _: &Entity<InputState>,
                     e: &InputEvent,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            if let InputEvent::PressEnter { .. } = e {
                view.preview_custom_apply(window, cx);
            }
        };
        let events = [
            cx.subscribe_in(&wi, window, enter),
            cx.subscribe_in(&hi, window, enter),
        ];
        wi.update(cx, |input, cx| input.focus(window, cx));
        if let Some(preview) = self.screen_ui.preview.as_mut() {
            preview.custom = Some(CustomEdit {
                w: wi,
                h: hi,
                error: None,
                _events: events,
            });
        }
        cx.notify();
    }

    /// Custom size's Apply: the page takes the typed size, or the fields say what is wrong.
    pub(in crate::board) fn preview_custom_apply(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self
            .screen_ui
            .preview
            .as_ref()
            .and_then(|p| p.custom.as_ref())
        else {
            return;
        };
        let read = |input: &Entity<InputState>| input.read(cx).value().trim().parse::<u32>().ok();
        match custom_size(read(&edit.w), read(&edit.h)) {
            Ok((w, h)) => {
                self.preview_change(
                    |p| {
                        p.size = Size::Custom(w, h);
                        p.rotated = false;
                        p.custom = None;
                    },
                    cx,
                );
                window.focus(&self.focus, cx);
            }
            Err(error) => {
                let edit = self
                    .screen_ui
                    .preview
                    .as_mut()
                    .and_then(|p| p.custom.as_mut());
                if let Some(edit) = edit {
                    edit.error = Some(error);
                }
                cx.notify();
            }
        }
    }

    /// Custom size's Cancel: the fields go, the size stays.
    pub(in crate::board) fn preview_custom_cancel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preview_change(|p| p.custom = None, cx);
        window.focus(&self.focus, cx);
    }
}

/// The custom size typed, or what to type instead.
pub(in crate::board) fn custom_size(w: Option<u32>, h: Option<u32>) -> Result<(u32, u32), String> {
    let ok = |v: u32| (CUSTOM_MIN..=CUSTOM_MAX).contains(&v);
    match (w, h) {
        (Some(w), Some(h)) if ok(w) && ok(h) => Ok((w, h)),
        _ => Err(format!(
            "Enter a width and height from {CUSTOM_MIN} to {CUSTOM_MAX}"
        )),
    }
}
