//! The maths behind the canvas view, with no GPUI so plain `cargo test` covers it: which part of
//! the page to render, where to paint the last frame while the next one renders, the board's dot
//! grid, and the pixel conversion for the GPU.
//!
//! The view is FigCraft's [`ViewState`]: the page point at the centre of the canvas, the zoom and
//! the canvas's size. Lengths are logical px unless a name says device px.

use figcraft_color::{Color, ColorProfile};
use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::ViewState;
use figcraft_render::{Dots, Pixmap, View};

/// How far past each edge of the canvas a frame is rendered (logical px), so that a pan shows
/// pixels that are already drawn while the next frame renders.
pub const OVERSCAN: f64 = 256.0;

/// The render quality while a zoom gesture is on: half the device pixels each way, stretched over
/// the same area, so frames keep up with the gesture. Full quality follows when it stops.
pub const PREVIEW_QUALITY: f64 = 0.5;

/// The canvas on screen: its size in logical px, and the window's scale factor (device px per
/// logical px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
}

/// The part of the page to render for `view` on `vp`: the canvas plus [`OVERSCAN`] on every side,
/// at `quality` (1 is one device pixel per device pixel, [`PREVIEW_QUALITY`] while zooming).
///
/// `view.vw` and `view.vh` must be the canvas's size too (the caller writes them from its bounds):
/// the page point at the canvas's top-left comes from them.
///
/// `None` when there is nothing to draw: a canvas under one logical px either way (a tab not laid
/// out yet, a minimised window), or numbers that make no frame (not finite, or a zoom, scale
/// factor or quality that is not above zero).
pub fn render_view(view: &ViewState, vp: Viewport, quality: f64) -> Option<View> {
    // Written as `>=` so that a NaN size counts as empty too.
    if ![view.vw, view.vh, vp.width, vp.height]
        .iter()
        .all(|size| *size >= 1.0)
    {
        return None;
    }
    let (left, top) = view.origin();
    let margin = OVERSCAN / view.zoom;
    let origin = (left - margin, top - margin);
    let scale = view.zoom * vp.scale_factor * quality;
    let pixels = |logical: f64| ((logical + 2.0 * OVERSCAN) * vp.scale_factor * quality).ceil();
    let (width, height) = (pixels(vp.width), pixels(vp.height));
    let usable = [origin.0, origin.1, scale, width, height]
        .iter()
        .all(|n| n.is_finite())
        && scale > 0.0
        && width >= 1.0
        && height >= 1.0;
    usable.then_some(View {
        origin,
        scale,
        width: width as u32,
        height: height as u32,
    })
}

/// Where to paint `frame`, an earlier render, now that the view is `now`: x, y, width and height
/// in logical px from the canvas's top-left. The frame keeps the page area it was rendered for, so
/// a pan moves it and a zoom scales it until a render of the new view arrives. A frame rendered at
/// [`PREVIEW_QUALITY`] covers the same area as a full one, with fewer pixels.
///
/// An empty rectangle when the numbers don't make one (a frame without a scale).
pub fn placement(frame: &View, now: &ViewState) -> [f64; 4] {
    let (left, top) = now.origin();
    let rect = [
        (frame.origin.0 - left) * now.zoom,
        (frame.origin.1 - top) * now.zoom,
        f64::from(frame.width) / frame.scale * now.zoom,
        f64::from(frame.height) / frame.scale * now.zoom,
    ];
    if rect.iter().all(|n| n.is_finite()) {
        rect
    } else {
        [0.0; 4]
    }
}

// Verbatim from FigCraft, crates/ui-egui/src/canvas/board_bar.rs: the dot grid's spacing.
/// The dot grid's spacing on the page at 100%, and the closest its dots come on screen (points):
/// zoomed out, every second, fourth… dot stays, the ones going fading out first.
const DOT_STEP: f64 = 24.0;
const DOT_MIN_GAP: f64 = 14.0;

/// The dot grid's spacing on the page at `zoom` (doubled until the dots are far enough apart on
/// screen), and how strongly the dots the next doubling drops still show (0 to 1).
fn dot_spacing(zoom: f64) -> Option<(f64, f32)> {
    if !(zoom.is_finite() && zoom > 0.0) {
        return None;
    }
    // Bounded: past about 1000 doublings the step is no longer finite, and there are no dots.
    let mut step = DOT_STEP;
    for _ in 0..1100 {
        if step * zoom >= DOT_MIN_GAP {
            break;
        }
        step *= 2.0;
    }
    let gap = step * zoom;
    // From one to two minimum gaps apart, the in-between dots fade in.
    (gap.is_finite() && gap >= DOT_MIN_GAP).then(|| {
        (
            step,
            ((gap - DOT_MIN_GAP) / DOT_MIN_GAP).clamp(0.0, 1.0) as f32,
        )
    })
}

/// The board's dot grid on a page of colour `page_rgb` as the screen shows it ([`screen_rgb`]), for
/// the renderer to draw under the layers (`Options::dots`): white dots on a dark page (luma under
/// 128), black ones on a light page, about a point and a half across on screen. `None` when `zoom`
/// is not finite and above zero.
pub fn dots(zoom: f64, page_rgb: [u8; 3]) -> Option<Dots> {
    let (step, fade) = dot_spacing(zoom)?;
    let [r, g, b] = page_rgb.map(f32::from);
    let dark_page = 0.2126 * r + 0.7152 * g + 0.0722 * b < 128.0;
    let ink = if dark_page { 255 } else { 0 };
    let alpha = if dark_page { 0.22 } else { 0.18 };
    let color = |a: f32| Color::from_rgba8(ink, ink, ink, (a * 255.0).round() as u8);
    let size = (1.5 * zoom.sqrt()).clamp(1.0, 3.0) / zoom;
    Some(Dots {
        step,
        size,
        color: color(alpha),
        faded: color(alpha * fade),
    })
}

/// The colour of `page` as the document holds it: its background without the alpha, in the
/// document's colour profile (`doc.color_profile`). White when `page` is not a page. The renderer
/// takes it as it is (`Options::background`).
///
/// The screen shows sRGB. The page's colour there, which fills the canvas and which the dot grid's
/// light/dark test reads ([`dots`]), is [`screen_rgb`]`(doc.color_profile, page_rgb)`.
pub fn page_rgb(doc: &Document, page: NodeId) -> [u8; 3] {
    match doc.node(page).map(|n| &n.kind) {
        Some(NodeKind::Page { background, .. }) => {
            let [r, g, b, _] = background.to_rgba8();
            [r, g, b]
        }
        _ => [255; 3],
    }
}

/// `rgb`, a colour of a document in `profile`, as the screen shows it: in sRGB. An sRGB colour is
/// returned as it is. One in another profile is converted, clipping what sRGB can't show, as
/// FigCraft's own canvas converts the document colours it paints (ui-egui `doc_c32`).
pub fn screen_rgb(profile: ColorProfile, rgb: [u8; 3]) -> [u8; 3] {
    if profile.is_srgb() {
        return rgb;
    }
    let [r, g, b] = rgb;
    let [r, g, b, _] = profile
        .conversion_to(ColorProfile::Srgb)
        .apply(Color::from_rgba8(r, g, b, 255))
        .to_rgba8();
    [r, g, b]
}

/// `pixmap` as BGRA bytes, the order of a GPU image: its premultiplied RGBA pixels composited over
/// `page_rgb`, so every pixel comes out opaque. A render with the page colour as its background is
/// opaque already, and then this is only the swap of red and blue.
pub fn to_bgra(pixmap: &Pixmap, page_rgb: [u8; 3]) -> Vec<u8> {
    let [page_r, page_g, page_b] = page_rgb.map(u32::from);
    let (pixels, _) = pixmap.data().as_chunks::<4>();
    // Sized up front and written in place: a frame is millions of pixels.
    let mut bgra = vec![0; pixels.len() * 4];
    let (out, _) = bgra.as_chunks_mut::<4>();
    for (out, &[r, g, b, a]) in out.iter_mut().zip(pixels) {
        // The colour is premultiplied, so the page fills in what alpha leaves uncovered.
        let behind = 255 - u32::from(a);
        let over = |colour: u8, page: u32| {
            (u32::from(colour) + (page * behind + 127) / 255).min(255) as u8
        };
        *out = [over(b, page_b), over(g, page_g), over(r, page_r), 255];
    }
    bgra
}

/// What soft-proof sRGB paints over a colour the screen can't show: a mid grey, opaque, BGRA.
pub const PROOF_GREY: [u8; 4] = [128, 128, 128, 255];

/// Soft-proof sRGB: diagonal stripes of [`PROOF_GREY`] over the pixels of `bgra` whose colours
/// sRGB can't show, as FigCraft's canvas marks them (ui-egui `mark_outside_srgb`). `bgra` is
/// `pixmap` made ready for the screen ([`to_bgra`]), and `pixmap` a render of a document whose
/// colours are in `profile`; nothing of an sRGB document is marked.
pub fn mark_outside_srgb(bgra: &mut [u8], pixmap: &Pixmap, profile: ColorProfile) {
    let width = (pixmap.width() as usize).max(1);
    let outside = profile
        .conversion_to(ColorProfile::Srgb)
        .clipped_pixels(pixmap.data());
    let (pixels, _) = bgra.as_chunks_mut::<4>();
    for (at, (pixel, outside)) in pixels.iter_mut().zip(outside).enumerate() {
        if outside && (at % width + at / width) % 8 < 4 {
            *pixel = PROOF_GREY;
        }
    }
}

/// The render behind pixel preview: the page area of `screen` (a view at the screen's density,
/// [`render_view`]) at `pixels` pixels per page unit, one more each way. [`magnify`] lays it over
/// the screen's pixels. `None` when that wouldn't be coarser than the screen (FigCraft shows pixel
/// preview only zoomed in past its resolution), or for numbers that make no view.
pub fn pixel_view(screen: &View, pixels: f64) -> Option<View> {
    let coarser = pixels.is_finite() && pixels > 0.0 && pixels < screen.scale;
    if !(coarser && screen.scale.is_finite()) {
        return None;
    }
    let size = |screen_px: u32| (f64::from(screen_px) / screen.scale * pixels).ceil() + 1.0;
    let (width, height) = (size(screen.width), size(screen.height));
    (width.is_finite() && height.is_finite()).then_some(View {
        origin: screen.origin,
        scale: pixels,
        width: width as u32,
        height: height as u32,
    })
}

/// `bgra`, an image of the page area `from`, laid over the pixels of `to`, the screen's, which
/// are finer: each screen pixel takes the image's pixel under its centre, so every pixel of the
/// image shows as a block with hard edges, as in FigCraft's pixel preview (a texture sampled
/// nearest). GPUI smooths an image it stretches, so the canvas stretches this one itself. Screen
/// pixels past the image take its nearest edge.
pub fn magnify(bgra: &[u8], from: &View, to: &View) -> Vec<u8> {
    let (width, height) = (to.width as usize, to.height as usize);
    let mut out = vec![0; width * height * 4];
    let from_width = from.width as usize;
    if width == 0 || from_width == 0 {
        return out;
    }
    // The image pixel under the centre of the screen's pixel `at`, along one axis.
    let under = |at: usize, screen_origin: f64, origin: f64, size: u32| {
        let page = screen_origin + (at as f64 + 0.5) / to.scale;
        let pixel = ((page - origin) * from.scale).floor();
        if pixel.is_finite() && pixel > 0.0 {
            (pixel as usize).min(size.saturating_sub(1) as usize)
        } else {
            0
        }
    };
    let columns: Vec<usize> = (0..width)
        .map(|x| under(x, to.origin.0, from.origin.0, from.width))
        .collect();
    let (image, _) = bgra.as_chunks::<4>();
    let (pixels, _) = out.as_chunks_mut::<4>();
    for (y, row) in pixels.chunks_exact_mut(width).enumerate() {
        let line = under(y, to.origin.1, from.origin.1, from.height);
        let Some(line) = image.get(line * from_width..(line + 1) * from_width) else {
            continue;
        };
        for (pixel, &column) in row.iter_mut().zip(&columns) {
            if let Some(value) = line.get(column) {
                *pixel = *value;
            }
        }
    }
    out
}

/// Whether the centre and zoom of `view` are numbers.
fn is_finite(view: &ViewState) -> bool {
    view.cx.is_finite() && view.cy.is_finite() && view.zoom.is_finite()
}

/// Runs `edit` on `view`, and puts the view back as it was when that leaves it not finite (a huge
/// delta overflows).
fn edit_finite(view: &mut ViewState, edit: impl FnOnce(&mut ViewState)) {
    let (cx, cy, zoom) = (view.cx, view.cy, view.zoom);
    edit(view);
    if !is_finite(view) {
        (view.cx, view.cy, view.zoom) = (cx, cy, zoom);
    }
}

/// Pans by (`dx`, `dy`) screen px: the content follows the fingers. Deltas that are not finite, or
/// so large that the view would overflow, change nothing.
pub fn pan(view: &mut ViewState, dx: f64, dy: f64) {
    edit_finite(view, |v| v.scroll(dx, dy, false));
}

/// Zooms by `factor` about `screen`, a point of the canvas in logical px: the page point under it
/// stays under it. The zoom stays between [`ViewState::MIN_ZOOM`] and [`ViewState::MAX_ZOOM`]. A
/// factor that is not finite and above zero, or a point that is not finite, changes nothing.
pub fn zoom_at(view: &mut ViewState, factor: f64, screen: (f64, f64)) {
    if !(factor.is_finite() && factor > 0.0 && screen.0.is_finite() && screen.1.is_finite()) {
        return;
    }
    let page = view.screen_to_page(screen.0, screen.1);
    // Clamped here as well, so that a product that overflows still means "as far as it goes".
    let zoom = (view.zoom * factor).clamp(ViewState::MIN_ZOOM, ViewState::MAX_ZOOM);
    edit_finite(view, |v| v.set_zoom_at(zoom, page));
}

/// Fits the content of `page` in the view, with a margin and no closer than 100%. An empty page,
/// or content too far out to fit, is shown at 100% on the page origin.
pub fn zoom_to_fit(view: &mut ViewState, doc: &Document, page: NodeId) {
    if let Some(bounds) = doc.page_content_bounds(page) {
        view.fit(bounds, 1.0);
        // Bounds that are not finite, or so far out that `fit`'s centre overflows, leave it not.
        if is_finite(view) {
            return;
        }
    }
    (view.cx, view.cy, view.zoom) = (0.0, 0.0, 1.0);
}

#[cfg(test)]
mod tests;
