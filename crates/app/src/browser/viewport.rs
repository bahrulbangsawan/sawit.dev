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
            match self.build(bounds, window) {
                Ok(engine) => {
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
            return;
        }
        if self.bounds != Some(bounds) {
            self.bounds = Some(bounds);
            self.apply_layout();
        }
    }

    fn build(&self, bounds: Bounds<Pixels>, window: &Window) -> Result<Engine, String> {
        let to_view = self.to_view.clone();
        Engine::build(
            EngineSpec {
                url: self.url.clone(),
                init_script: page_script(&self.token),
                data_store: self
                    .project
                    .as_deref()
                    .map(sawit_core::browser::data_store_id),
                visible: self.page_shown(),
                frame: page_frame(frame(bounds), &self.viewport).0,
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

    /// The CSS viewport the page sees, and the zoom that makes it fit.
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

    /// Puts the web view where the viewport mode wants it, with the zoom
    /// that keeps the CSS viewport at the device size.
    fn apply_layout(&self) {
        if let (Some(engine), Some(bounds)) = (&self.engine, self.bounds) {
            let (frame, scale) = page_frame(frame(bounds), &self.viewport);
            engine.set_frame(frame);
            engine.set_zoom(scale);
        }
    }

    /// The page zoom in force (1 unless a device is scaled down).
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

    /// Whether the webview shows: its tab is on screen, and no menu is open over it.
    pub(super) fn page_shown(&self) -> bool {
        self.visible && !self.menu_open
    }

    /// Shows or hides the webview, when [`Self::page_shown`] is no longer `was`.
    fn show_page(&self, was: bool) {
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

/// Where the web view goes in `bounds`, and the page zoom: the page itself
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

fn frame(bounds: Bounds<Pixels>) -> Frame {
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
