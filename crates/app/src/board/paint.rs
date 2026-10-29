//! Painting the canvas: the page colour over the whole canvas, then the newest frame the render
//! thread drew, wherever the view has moved it since, then the overlay over it (the selection,
//! handles, names, guides and previews: [`overlay_paint`]).

use std::sync::Arc;

use figcraft_engine::ViewState;
use figcraft_render::View;
use gpui_kit::{App, Bounds, Corners, Pixels, RenderImage, Window, fill, point, px, rgb, size};
use sawit_board::view::placement;

use super::overlay_paint::{self, Overlay};

/// One paint of the canvas, worked out in its prepaint.
pub(super) struct Painting {
    /// The page's colour on screen: it fills the canvas, so the page shows wherever the frame
    /// doesn't reach (before the first frame, past the overscan of a pan, around a zoom out).
    pub page: [u8; 3],
    /// The newest frame's image, and where it goes in the window.
    pub frame: Option<(Arc<RenderImage>, Bounds<Pixels>)>,
    /// The images frames have replaced since the last paint, to take out of the window's sprite
    /// atlas: each frame is a new image, and the atlas would otherwise keep them all.
    pub stale: Vec<Arc<RenderImage>>,
    /// What goes over the frame, as the view is now.
    pub overlay: Overlay,
}

/// Paints `painting` over the canvas at `bounds`.
pub(super) fn paint(bounds: Bounds<Pixels>, painting: Painting, window: &mut Window, cx: &mut App) {
    for image in painting.stale {
        let _ = window.drop_image(image);
    }
    let [r, g, b] = painting.page;
    window.paint_quad(fill(bounds, rgb(u32::from_be_bytes([0, r, g, b]))));
    if let Some((image, at)) = painting.frame
        && let Err(err) = window.paint_image(bounds, at, Corners::default(), image, 0, false)
    {
        log::error!("the canvas frame couldn't be painted: {err}");
    }
    overlay_paint::paint(bounds.origin, painting.overlay, window, cx);
}

/// Where in the window to paint `frame`, a render of the page area it names, now that the view
/// is `now` and the canvas is at `bounds` on a screen of `scale_factor`: its [`placement`] from
/// the canvas's top-left.
///
/// A frame at the screen's own density (one image pixel per device pixel) gets its top-left on a
/// whole device pixel, as FigCraft's canvas places it (ui-egui `presented_image_rect`): GPUI then
/// copies its pixels one for one, where a fraction of a pixel off would blur them all. A frame
/// stretched over another density, a zoom preview or a stale frame of an earlier zoom, is placed
/// as it is.
pub(super) fn image_bounds(
    bounds: Bounds<Pixels>,
    frame: &View,
    now: &ViewState,
    scale_factor: f64,
) -> Bounds<Pixels> {
    let [x, y, width, height] = placement(frame, now);
    let (mut left, mut top) = (
        f64::from(bounds.origin.x) + x,
        f64::from(bounds.origin.y) + y,
    );
    if native_density(frame.scale, now.zoom, scale_factor) {
        left = (left * scale_factor).round() / scale_factor;
        top = (top * scale_factor).round() / scale_factor;
    }
    Bounds::new(
        point(px(left as f32), px(top as f32)),
        size(px(width as f32), px(height as f32)),
    )
}

/// Whether a frame rendered at `scale` (device px per page unit) shows one of its pixels per
/// device pixel at `zoom` on a screen of `scale_factor`.
fn native_density(scale: f64, zoom: f64, scale_factor: f64) -> bool {
    let numbers = [scale, zoom, scale_factor];
    numbers.iter().all(|n| n.is_finite() && *n > 0.0)
        && (scale - zoom * scale_factor).abs() <= scale.max(1.0) * 1e-9
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page origin in the middle of an 800 × 600 canvas, at `zoom`.
    fn centred(zoom: f64) -> ViewState {
        ViewState {
            zoom,
            vw: 800.0,
            vh: 600.0,
            ..ViewState::default()
        }
    }

    /// A frame of the page area from (`x`, `y`), 1000 × 800 device px at `scale`.
    fn frame(x: f64, y: f64, scale: f64) -> View {
        View {
            origin: (x, y),
            scale,
            width: 1000,
            height: 800,
        }
    }

    #[test]
    fn a_native_frame_is_placed_on_whole_device_pixels() {
        let canvas = Bounds::new(point(px(10.), px(20.)), size(px(800.), px(600.)));
        // At 100% the canvas's top-left shows page point (-400, -300). A frame 256.3 and 256.2
        // page units further up and left, rendered 2 device px per page unit on a 2× screen.
        let native = frame(-656.3, -556.2, 2.0);
        let at = image_bounds(canvas, &native, &centred(1.0), 2.0);
        // 10 - 256.3 = -246.3 is device px -492.6, which rounds to -493; 20 - 256.2 = -236.2 is
        // -472.4, which rounds to -472. The size is kept.
        assert_eq!(at.origin, point(px(-246.5), px(-236.0)));
        assert_eq!(at.size, size(px(500.), px(400.)));

        // A preview at half the device pixels, and a frame of an earlier zoom, are stretched over
        // the screen's pixels anyway: placed as they are.
        let preview = frame(-656.3, -556.2, 1.0);
        let at = image_bounds(canvas, &preview, &centred(1.0), 2.0);
        assert_eq!(at.origin, point(px(-246.3), px(-236.2)));
        assert_eq!(at.size, size(px(1000.), px(800.)));
        let earlier = image_bounds(canvas, &native, &centred(1.5), 2.0);
        assert_eq!(earlier, {
            let [x, y, w, h] = placement(&native, &centred(1.5));
            Bounds::new(
                point(px((10.0 + x) as f32), px((20.0 + y) as f32)),
                size(px(w as f32), px(h as f32)),
            )
        });
    }

    #[test]
    fn a_frame_without_a_scale_is_not_rounded() {
        let canvas = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(600.)));
        assert!(!native_density(0.0, 0.0, 2.0));
        assert!(!native_density(f64::NAN, 1.0, 2.0));
        // `placement` makes an empty rectangle of a frame without a scale.
        let at = image_bounds(canvas, &frame(0.0, 0.0, 0.0), &centred(1.0), 2.0);
        assert_eq!(at.size, size(px(0.), px(0.)));
    }
}
