//! Scrolls and pinches on the canvas: a scroll pans the view, and a pinch or a scroll with ⌘ held
//! zooms it about the pointer, at preview quality until the zoom settles.

use gpui_kit::*;
use sawit_board::view;

use super::{BoardView, ZOOM_SETTLE};

/// How far a mouse wheel's scroll of one line pans the canvas.
const SCROLL_LINE: Pixels = px(20.);

impl BoardView {
    /// Pans by a scroll's delta: the content follows the fingers. With ⌘ held it zooms about the
    /// pointer instead, by e^(-dy/100) for a scroll of dy px.
    pub(super) fn on_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(SCROLL_LINE);
        let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
        if event.modifiers.platform {
            self.zoom_by((-dy * 0.01).exp(), event.position, cx);
        } else if let Some(view) = self.view_mut() {
            view::pan(view, dx, dy);
            cx.notify();
        }
    }

    /// Zooms by a pinch's delta (0.1 is 10% closer) about the pointer.
    pub(super) fn on_pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        self.zoom_by(1.0 + f64::from(event.delta), event.position, cx);
    }

    /// Zooms by `factor` about `position` (in the window). Frames render at preview quality until
    /// the zoom has been still for [`ZOOM_SETTLE`], then at full quality.
    fn zoom_by(&mut self, factor: f64, position: Point<Pixels>, cx: &mut Context<Self>) {
        let pointer = position - self.bounds.origin;
        let Some(view) = self.view_mut() else {
            return;
        };
        view::zoom_at(view, factor, (f64::from(pointer.x), f64::from(pointer.y)));
        self.zooming_until = Some(cx.background_executor().now() + ZOOM_SETTLE);
        self.zoom_settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ZOOM_SETTLE).await;
            let _ = this.update(cx, |this, cx| {
                this.zooming_until = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
}
