//! Where the webview goes: built on the first layout, over the whole page area or framed to a
//! device's size (scaled to fit, under its caption), and shown only while its tab is on screen
//! with no menu over it.

use std::rc::Rc;

use gpui_kit::*;
use sawit_core::browser::{Device, ViewportMode, device_by_name, fit, page_script};
use serde_json::{Value, json};

use super::engine::{Engine, EngineSpec, Frame};
use super::{BrowserView, FromPage};

/// Around the page in device mode, and the strip above it for the caption.
const DEVICE_PADDING: f32 = 16.;
pub(super) const CAPTION_BAND: f32 = 24.;

impl BrowserView {
    pub(super) fn place(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.error.is_some() {
            return;
        }
        // This frame drew the caption with the zoom of the last bounds, and a notify during a
        // draw is lost: the view draws again once the frame is over.
        if caption_moves(self.bounds, bounds, &self.viewport) {
            cx.defer_in(window, |_, _, cx| cx.notify());
        }
        if self.engine.is_none() {
            self.start(bounds, window, cx);
            return;
        }
        if self.bounds != Some(bounds) {
            self.bounds = Some(bounds);
            self.apply_layout();
        }
    }

    /// Builds the web view at `bounds`: on the first layout, after the tab gave its view up
    /// ([`super::suspend`]), or off screen for an agent's op on a tab not on show. It has the
    /// keyboard's host only while on show; the ops sent meanwhile run once it is up.
    pub(super) fn start(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        match self.build(bounds, window) {
            Ok(engine) => {
                // The page shares the keyboard with this view only ([`engine::keyboard`]), and
                // only once the tab is on show: a tab woken off screen never takes it.
                if self.page_shown() {
                    engine.set_keyboard_host(Some(&self.focus));
                    self.suspended.keyboard_host = true;
                }
                if let Some(history) = self.suspended.history.take() {
                    engine.restore_history(&history);
                }
                self.engine = Some(Rc::new(engine));
                self.bounds = Some(bounds);
                self.apply_layout();
                for (op, reply) in std::mem::take(&mut self.queued) {
                    self.run(op, reply, cx);
                }
            }
            Err(e) => {
                self.error = Some(e.clone().into());
                for (_, reply) in self.queued.drain(..) {
                    let _ = reply.send(Err(format!("The browser couldn't start: {e}")));
                }
            }
        }
        self.bounds = Some(bounds);
    }

    fn build(&self, bounds: Bounds<Pixels>, window: &Window) -> Result<Engine, String> {
        let to_view = self.to_view.clone();
        Engine::build(
            EngineSpec {
                // A page coming back with its history goes to it from a blank start.
                url: match self.suspended.history {
                    Some(_) => "about:blank".into(),
                    None => self.url.clone(),
                },
                init_script: page_script(&self.token),
                data_store: self
                    .project
                    .as_deref()
                    .map(sawit_core::browser::data_store_id),
                visible: self.page_shown(),
                frame: page_frame(frame(bounds), &self.viewport).0,
                screen: None,
            },
            move |event| {
                let _ = to_view.unbounded_send(FromPage::Engine(event));
            },
            window,
        )
    }

    /// Sets how the page is sized in the tab.
    pub(super) fn set_viewport(&mut self, mode: ViewportMode, cx: &mut Context<Self>) {
        self.viewport = mode;
        self.apply_layout();
        cx.notify();
    }

    /// The CSS viewport the page sees, and the scale that makes it fit.
    pub(super) fn viewport_json(&self) -> Value {
        let (width, height, device, scale) = match (&self.viewport, self.bounds) {
            (
                ViewportMode::Fixed {
                    name,
                    width,
                    height,
                },
                Some(bounds),
            ) => {
                let (_, scale) = page_frame(frame(bounds), &self.viewport);
                (*width as f64, *height as f64, name.clone(), scale)
            }
            (
                ViewportMode::Fixed {
                    name,
                    width,
                    height,
                },
                None,
            ) => (*width as f64, *height as f64, name.clone(), 1.),
            (ViewportMode::Responsive, bounds) => {
                let (w, h) = bounds.map_or((0., 0.), |b| {
                    (
                        f64::from(f32::from(b.size.width)).round(),
                        f64::from(f32::from(b.size.height)).round(),
                    )
                });
                (w, h, Some("responsive".to_string()), 1.)
            }
        };
        json!({ "width": width, "height": height, "device": device, "scale": scale })
    }

    /// Puts the web view where the viewport mode wants it, at the scale that keeps the CSS
    /// viewport at the device size, and with the device's pixel ratio (a computer's size drawn
    /// smaller, the screen's: never under 1, which would blur it).
    ///
    /// The scale is the view's (`Engine::set_view_scale`, as the canvas's web frames do): the
    /// page lays out at the view's size over the scale and only its drawing shrinks. A page zoom
    /// would lay it out the same, but WebKit then holds text to its minimum font size in device
    /// pixels, 9 px over the zoom: 20 CSS px at 0.44, so a 12 px paragraph overlapped its own
    /// lines. The page zoom stays at 1, and is the fallback only where WebKit lacks a view scale.
    /// Puts the view back where the viewport mode wants it (after a capture changed its frame).
    pub(super) fn relayout(&self) {
        self.apply_layout();
    }

    /// Where the view sits now, for a capture that makes it taller for a moment.
    pub(super) fn page_frame_now(&self) -> Option<Frame> {
        self.bounds.map(|b| page_frame(frame(b), &self.viewport).0)
    }

    fn apply_layout(&self) {
        if let (Some(engine), Some(bounds)) = (&self.engine, self.bounds) {
            let (frame, scale) = page_frame(frame(bounds), &self.viewport);
            engine.set_frame(frame);
            if scale < 1. {
                if engine.set_view_scale(scale) {
                    engine.set_zoom(1.);
                } else {
                    engine.set_zoom(scale);
                }
            } else {
                engine.clear_view_scale();
                engine.set_zoom(1.);
            }
            let ratio = device_ratio(&self.viewport).or_else(|| {
                // A computer's size drawn smaller keeps the screen's density.
                (scale < 1.).then(|| engine.screen_scale())
            });
            engine.set_device_scale(ratio.map(|r| device_scale(r, scale)));
            // vh and vw follow the page's viewport (a full-page capture holds them there too).
            let (w, h) = match &self.viewport {
                ViewportMode::Fixed { width, height, .. } => {
                    (f64::from(*width), f64::from(*height))
                }
                ViewportMode::Responsive => (f64::from(frame.width), f64::from(frame.height)),
            };
            engine.hold_viewport_units(w, h);
            // The overlay keeps its size on screen: it counter-scales by the page's scale.
            drop(self.page_op(ui_scale_op(scale)));
            self.mesurer_ui_scale(scale);
        }
    }

    /// The scale the page is drawn at (1 unless a device is scaled down): CSS px to the view's
    /// points.
    pub(super) fn scale(&self) -> f64 {
        self.bounds
            .map_or(1., |b| page_frame(frame(b), &self.viewport).1)
    }

    /// Shows or hides the webview; it sits above GPUI's drawing, so it must
    /// be hidden whenever another tab or pane takes its place. Hidden, it gives
    /// AppKit's keyboard back to the window. GPUI's, if the view or a field in it
    /// had it (after a press in the page), goes to the workspace once the view is
    /// no longer drawn (`workspace::KeyboardHome`). Shown again, the address
    /// field has GPUI's back if it had it; the page has neither until a click in it.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        let was = self.page_shown();
        self.visible = visible;
        // A menu left open goes with the toolbar that drew it, which says nothing then.
        if !visible {
            self.menu_open = false;
            self.forget_cover();
        }
        // A page built off screen gets the keyboard's host once its tab is on show.
        if visible
            && !self.suspended.keyboard_host
            && let Some(engine) = &self.engine
        {
            engine.set_keyboard_host(Some(&self.focus));
            self.suspended.keyboard_host = true;
        }
        self.show_page(was);
    }

    /// The device menu opened or closed. It opens over the page, and the webview draws above
    /// GPUI's drawing, menus too: the menu would open unseen behind the page, the button alone
    /// showing it open. So the webview is hidden while the menu is open, and comes back, at the
    /// size picked, when it closes.
    pub(super) fn set_menu_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.menu_open == open {
            return;
        }
        let was = self.page_shown();
        self.menu_open = open;
        self.show_page(was);
        cx.notify();
    }

    /// Whether the webview shows: its tab is on screen, and no menu is open over it, nor
    /// anything else GPUI draws ([`super::cover`]).
    pub(super) fn page_shown(&self) -> bool {
        self.visible && !self.menu_open && !self.frozen.hidden
    }

    /// Shows or hides the webview, when [`Self::page_shown`] is no longer `was`.
    pub(super) fn show_page(&self, was: bool) {
        let shown = self.page_shown();
        if shown == was {
            return;
        }
        if let Some(engine) = &self.engine {
            engine.set_visible(shown);
            if !shown {
                engine.focus_parent();
            }
        }
    }
}

/// Where the web view goes in `bounds`, and the scale the page is drawn at: the page itself
/// in responsive mode, else the device size scaled to fit and centered
/// below the caption band.
pub(super) fn page_frame(bounds: Frame, mode: &ViewportMode) -> (Frame, f64) {
    let ViewportMode::Fixed { width, height, .. } = mode else {
        return (bounds, 1.);
    };
    let area = (
        (bounds.width - 2. * DEVICE_PADDING).max(1.),
        (bounds.height - 2. * DEVICE_PADDING - CAPTION_BAND).max(1.),
    );
    let fit = fit((*width, *height), area);
    // Whole points, as `fit` gives the size, so WebKit doesn't round the frame.
    let frame = Frame {
        x: (bounds.x + DEVICE_PADDING + (area.0 - fit.width) / 2.).round(),
        y: (bounds.y + CAPTION_BAND + DEVICE_PADDING + (area.1 - fit.height) / 2.).round(),
        width: fit.width,
        height: fit.height,
    };
    (frame, fit.scale)
}

/// Whether placing the page at `new` (it was at `old`) changes what the view draws: a device's
/// caption says the zoom its page area gives it, which follows the area.
pub(super) fn caption_moves(
    old: Option<Bounds<Pixels>>,
    new: Bounds<Pixels>,
    mode: &ViewportMode,
) -> bool {
    old != Some(new) && matches!(mode, ViewportMode::Fixed { .. })
}

/// `iPhone 15 · 393 × 852 · 75%`: no name when there is none, no percentage
/// at full size.
pub(super) fn caption(name: Option<&str>, width: u32, height: u32, scale: f64) -> String {
    let mut parts: Vec<String> = name.map(str::to_string).into_iter().collect();
    parts.push(format!("{width} × {height}"));
    let percent = (scale * 100.).round() as u32;
    if percent < 100 {
        parts.push(format!("{percent}%"));
    }
    parts.join(" · ")
}

pub(super) fn frame(bounds: Bounds<Pixels>) -> Frame {
    Frame {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}

/// The device `mode` imitates, and whether it is turned on its side (the size swapped):
/// `None` in responsive mode, or for a size of no device's.
pub(super) fn device_of(mode: &ViewportMode) -> Option<(&'static Device, bool)> {
    let ViewportMode::Fixed {
        name: Some(name),
        width,
        height,
    } = mode
    else {
        return None;
    };
    let device = device_by_name(name)?;
    match (*width, *height) {
        size if size == (device.width, device.height) => Some((device, false)),
        size if size == (device.height, device.width) => Some((device, true)),
        _ => None,
    }
}

/// The pixel ratio the page gets in `mode`: its device's (a phone's or a tablet's, either way
/// up), else the screen's own (`None`), for computers, sizes of no device's and responsive mode.
pub(super) fn device_ratio(mode: &ViewportMode) -> Option<f64> {
    device_of(mode).and_then(|(device, _)| device.dpr)
}

/// The op that tells the page's overlay the scale the page is drawn at.
pub(super) fn ui_scale_op(scale: f64) -> Value {
    json!({ "op": "ui_scale", "scale": scale })
}

/// The device scale factor WebKit draws at so a page drawn at `scale` reads `ratio` as its
/// `devicePixelRatio` (WebKit gives the page the factor times the view scale), and draws `ratio`
/// pixels per CSS px, as the device does.
pub(super) fn device_scale(ratio: f64, scale: f64) -> f64 {
    ratio / scale.max(0.01)
}

/// The same size with width and height swapped; nothing in responsive mode.
pub(super) fn rotated(mode: &ViewportMode) -> Option<ViewportMode> {
    match mode {
        ViewportMode::Responsive => None,
        ViewportMode::Fixed {
            name,
            width,
            height,
        } => Some(ViewportMode::Fixed {
            name: name.clone(),
            width: *height,
            height: *width,
        }),
    }
}
