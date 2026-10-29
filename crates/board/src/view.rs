//! The maths behind the canvas view, with no GPUI so plain `cargo test` covers it: which part of
//! the page to render, where to paint the last frame while the next one renders, the board's dot
//! grid, and the pixel conversion for the GPU.
//!
//! The view is FigCraft's [`ViewState`]: the page point at the centre of the canvas, the zoom and
//! the canvas's size. Lengths are logical px unless a name says device px.

use figcraft_color::Color;
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

/// The board's dot grid on a page of colour `page_rgb`, for the renderer to draw under the layers
/// (`Options::dots`): white dots on a dark page (luma under 128), black ones on a light page, about
/// a point and a half across on screen. `None` when `zoom` is not finite and above zero.
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

/// The colour to fill the canvas with for `page`: its background, without the alpha. White when
/// `page` is not a page.
pub fn page_rgb(doc: &Document, page: NodeId) -> [u8; 3] {
    match doc.node(page).map(|n| &n.kind) {
        Some(NodeKind::Page { background, .. }) => {
            let [r, g, b, _] = background.to_rgba8();
            [r, g, b]
        }
        _ => [255; 3],
    }
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
mod tests {
    use super::*;
    use crate::doc::BoardDoc;

    /// 800 × 600 logical px on a 2× screen.
    const VP: Viewport = Viewport {
        width: 800.0,
        height: 600.0,
        scale_factor: 2.0,
    };

    /// A view of `vw` × `vh` logical px centred on (`cx`, `cy`).
    fn view(cx: f64, cy: f64, zoom: f64, vw: f64, vh: f64) -> ViewState {
        ViewState {
            cx,
            cy,
            zoom,
            vw,
            vh,
            ..ViewState::default()
        }
    }

    /// The page origin in the middle of [`VP`], at 100%.
    fn centred() -> ViewState {
        view(0.0, 0.0, 1.0, VP.width, VP.height)
    }

    fn all_finite(v: &ViewState) -> bool {
        [v.cx, v.cy, v.zoom, v.vw, v.vh]
            .iter()
            .all(|f| f.is_finite())
    }

    /// A pixmap of one row of premultiplied RGBA pixels.
    fn pixmap(pixels: &[[u8; 4]]) -> Pixmap {
        let mut pixmap = Pixmap::new(pixels.len() as u32, 1).unwrap();
        pixmap.data_mut().copy_from_slice(pixels.as_flattened());
        pixmap
    }

    /// A document whose first page holds a rectangle at each (x, y, width, height).
    fn page_with(rects: &[(f64, f64, f64, f64)]) -> (Document, NodeId) {
        let mut doc = Document::new("File");
        let page = doc.first_page().unwrap();
        for &(x, y, w, h) in rects {
            doc.create(page, NodeKind::Rectangle, "Rectangle", x, y, w, h)
                .unwrap();
        }
        (doc, page)
    }

    /// What a trackpad, a layout pass or a damaged file can hand in: not numbers, zero, negative,
    /// tiny, huge, and ordinary ones.
    const NUMBERS: [f64; 12] = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -1.0,
        1e-300,
        1e300,
        f64::MAX,
        f64::MIN,
        0.5,
        2.0,
        800.0,
    ];

    #[test]
    fn render_view_adds_overscan() {
        let frame = render_view(&centred(), VP, 1.0).unwrap();
        assert_eq!(frame.origin, (-656.0, -556.0));
        assert_eq!(frame.scale, 2.0);
        assert_eq!((frame.width, frame.height), (2624, 2224));
    }

    #[test]
    fn render_view_is_none_for_empty_viewport() {
        let no_width = ViewState {
            vw: 0.0,
            ..centred()
        };
        assert!(render_view(&no_width, VP, 1.0).is_none());
        // The same for the view's height, and for the canvas's own size: under one logical px
        // either way, or not a number.
        for (vw, vh) in [(800.0, 0.0), (0.5, 600.0), (f64::NAN, 600.0)] {
            let v = view(0.0, 0.0, 1.0, vw, vh);
            assert!(render_view(&v, VP, 1.0).is_none(), "view {vw} x {vh}");
        }
        for (width, height) in [
            (0.0, 600.0),
            (800.0, 0.0),
            (0.5, 600.0),
            (800.0, 0.99),
            (f64::NAN, 600.0),
            (800.0, f64::NAN),
        ] {
            let vp = Viewport {
                width,
                height,
                ..VP
            };
            assert!(
                render_view(&centred(), vp, 1.0).is_none(),
                "canvas {width} x {height}"
            );
        }
        // One logical px is enough to draw.
        let one = Viewport {
            width: 1.0,
            height: 1.0,
            ..VP
        };
        let v = view(0.0, 0.0, 1.0, 1.0, 1.0);
        assert!(render_view(&v, one, 1.0).is_some());
    }

    #[test]
    fn render_view_is_none_for_a_scale_that_makes_no_frame() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
            let vp = Viewport {
                scale_factor: bad,
                ..VP
            };
            assert!(render_view(&centred(), vp, 1.0).is_none(), "sf {bad}");
            assert!(render_view(&centred(), VP, bad).is_none(), "quality {bad}");
            let v = ViewState {
                zoom: bad,
                ..centred()
            };
            assert!(render_view(&v, VP, 1.0).is_none(), "zoom {bad}");
        }
    }

    #[test]
    fn render_view_is_none_or_a_finite_frame_for_any_numbers() {
        let mut drawn = 0;
        for &zoom in &NUMBERS {
            for &size in &NUMBERS {
                for &scale_factor in &NUMBERS {
                    for &quality in &NUMBERS {
                        for &centre in &NUMBERS {
                            let v = view(centre, centre, zoom, size, size);
                            let vp = Viewport {
                                width: size,
                                height: size,
                                scale_factor,
                            };
                            let Some(frame) = render_view(&v, vp, quality) else {
                                continue;
                            };
                            drawn += 1;
                            assert!(
                                frame.origin.0.is_finite()
                                    && frame.origin.1.is_finite()
                                    && frame.scale.is_finite()
                                    && frame.scale > 0.0
                                    && frame.width >= 1
                                    && frame.height >= 1,
                                "zoom {zoom}, size {size}, sf {scale_factor}, quality {quality}, \
                                 centre {centre}: {frame:?}"
                            );
                        }
                    }
                }
            }
        }
        // The sweep does reach the frames.
        assert!(drawn > 0);
    }

    #[test]
    fn a_preview_frame_covers_the_same_logical_area_with_fewer_pixels() {
        let v = centred();
        let preview = render_view(&v, VP, PREVIEW_QUALITY).unwrap();
        assert_eq!(preview.origin, (-656.0, -556.0));
        assert_eq!(preview.scale, 1.0);
        assert_eq!((preview.width, preview.height), (1312, 1112));
        assert_eq!(placement(&preview, &v), [-256.0, -256.0, 1312.0, 1112.0]);
    }

    #[test]
    fn render_view_follows_the_zoom_and_rounds_sizes_up() {
        let v = view(10.0, 20.0, 2.0, 801.0, 601.0);
        let vp = Viewport {
            width: 801.0,
            height: 601.0,
            scale_factor: 1.5,
        };
        let frame = render_view(&v, vp, 1.0).unwrap();
        // Half the canvas is 200.25 page units at 200%, and the overscan 128.
        assert_eq!(frame.origin, (10.0 - 200.25 - 128.0, 20.0 - 150.25 - 128.0));
        assert_eq!(frame.scale, 3.0);
        // 1313 × 1.5 and 1113 × 1.5 are half a pixel short of a whole one.
        assert_eq!((frame.width, frame.height), (1970, 1670));
    }

    #[test]
    fn placement_identity() {
        let v = centred();
        let frame = render_view(&v, VP, 1.0).unwrap();
        assert_eq!(placement(&frame, &v), [-256.0, -256.0, 1312.0, 1112.0]);
    }

    #[test]
    fn placement_follows_pan() {
        let mut v = centred();
        let frame = render_view(&v, VP, 1.0).unwrap();
        pan(&mut v, 10.0, 0.0);
        assert_eq!(placement(&frame, &v)[0], -246.0);
    }

    #[test]
    fn placement_follows_zoom() {
        let mut v = centred();
        let frame = render_view(&v, VP, 1.0).unwrap();
        zoom_at(&mut v, 2.0, (400.0, 300.0));
        assert_eq!(placement(&frame, &v), [-912.0, -812.0, 2624.0, 2224.0]);
    }

    #[test]
    fn placement_is_always_finite_and_empty_for_a_frame_without_a_scale() {
        let no_scale = View {
            origin: (0.0, 0.0),
            scale: 0.0,
            width: 10,
            height: 10,
        };
        assert_eq!(placement(&no_scale, &centred()), [0.0; 4]);
        for &origin in &NUMBERS {
            for &scale in &NUMBERS {
                for &zoom in &NUMBERS {
                    for &centre in &NUMBERS {
                        let frame = View {
                            origin: (origin, origin),
                            scale,
                            width: 1312,
                            height: 1112,
                        };
                        let now = view(centre, centre, zoom, 800.0, 600.0);
                        assert!(
                            placement(&frame, &now).iter().all(|n| n.is_finite()),
                            "origin {origin}, scale {scale}, zoom {zoom}, centre {centre}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pan_moves_the_content_with_the_fingers() {
        let mut v = view(0.0, 0.0, 2.0, 800.0, 600.0);
        let before = v.page_to_screen(30.0, 40.0);
        pan(&mut v, 10.0, -6.0);
        let after = v.page_to_screen(30.0, 40.0);
        assert_eq!((after.0 - before.0, after.1 - before.1), (10.0, -6.0));
    }

    #[test]
    fn pan_ignores_deltas_that_are_not_finite_or_overflow() {
        // At 2%, a delta of f64::MAX is 50 times that in page units: more than a float holds.
        let mut v = view(5.0, 6.0, 0.02, 800.0, 600.0);
        let before = v.clone();
        for (dx, dy) in [
            (f64::NAN, 5.0),
            (5.0, f64::INFINITY),
            (f64::NEG_INFINITY, f64::NAN),
            (f64::MAX, 0.0),
            (0.0, f64::MIN),
        ] {
            pan(&mut v, dx, dy);
            assert_eq!(v, before, "pan({dx}, {dy})");
        }
    }

    #[test]
    fn zoom_at_keeps_the_page_point_under_the_cursor() {
        let mut v = view(120.0, -40.0, 0.5, 800.0, 600.0);
        let cursor = (100.0, 450.0);
        let under = v.screen_to_page(cursor.0, cursor.1);
        // In, out, and into both ends of the zoom range.
        for factor in [2.0, 3.0, 0.1, 1.5, 1e9, 1e-9, 4.0] {
            zoom_at(&mut v, factor, cursor);
            let now = v.screen_to_page(cursor.0, cursor.1);
            assert!(
                (now.0 - under.0).abs() < 1e-9 && (now.1 - under.1).abs() < 1e-9,
                "factor {factor}: {now:?}, not {under:?}"
            );
        }
        assert_eq!(v.zoom, 0.02 * 4.0);
    }

    #[test]
    fn extreme_input_leaves_the_view_finite() {
        let starts = [
            centred(),
            // Already far out, at the top zoom, and at the very edge of what a float holds.
            view(1e300, -1e300, 256.0, 800.0, 600.0),
            view(f64::MAX, f64::MIN, 0.02, 800.0, 600.0),
        ];
        for start in &starts {
            let from = (start.cx, start.cy, start.zoom);
            for &a in &NUMBERS {
                for &b in &NUMBERS {
                    let mut v = start.clone();
                    pan(&mut v, a, b);
                    assert!(all_finite(&v), "pan({a}, {b}) from {from:?}");
                    for &c in &NUMBERS {
                        let mut v = start.clone();
                        zoom_at(&mut v, a, (b, c));
                        assert!(all_finite(&v), "zoom_at({a}, ({b}, {c})) from {from:?}");
                        assert!(
                            (0.02..=256.0).contains(&v.zoom),
                            "zoom_at({a}, ({b}, {c})) from {from:?}: zoom {}",
                            v.zoom
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn zoom_to_fit_shows_the_content_and_stops_at_100_percent() {
        // Wide content: zoomed out until it fills the canvas but for a 64 px margin each side.
        let (doc, page) = page_with(&[(0.0, 0.0, 4000.0, 1000.0)]);
        let mut v = centred();
        zoom_to_fit(&mut v, &doc, page);
        assert!((v.zoom - 672.0 / 4000.0).abs() < 1e-12, "{}", v.zoom);
        assert_eq!((v.cx, v.cy), (2000.0, 500.0));
        let (left, _) = v.page_to_screen(0.0, 0.0);
        let (right, _) = v.page_to_screen(4000.0, 1000.0);
        assert!((left - 64.0).abs() < 1e-9 && (right - 736.0).abs() < 1e-9);

        // Small content: centred, and not magnified past 100%.
        let (doc, page) = page_with(&[(100.0, 100.0, 200.0, 100.0)]);
        zoom_to_fit(&mut v, &doc, page);
        assert_eq!((v.zoom, v.cx, v.cy), (1.0, 200.0, 150.0));
    }

    #[test]
    fn zoom_to_fit_an_empty_board_is_100_percent_on_the_origin() {
        let board = BoardDoc::new_board("Canvas").unwrap();
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let mut v = view(500.0, -300.0, 0.5, 800.0, 600.0);
        zoom_to_fit(&mut v, &doc, page);
        assert_eq!((v.zoom, v.cx, v.cy), (1.0, 0.0, 0.0));
    }

    #[test]
    fn zoom_to_fit_content_without_finite_bounds_is_100_percent_on_the_origin() {
        // An infinite width makes the bounds not a number.
        let infinite = page_with(&[(0.0, 0.0, f64::INFINITY, 10.0)]);
        // Finite bounds, but so far out that adding their edges for the centre overflows.
        let far = page_with(&[(1e308, 0.0, 7e307, 10.0)]);
        for (doc, page) in [infinite, far] {
            let mut v = view(500.0, -300.0, 0.5, 800.0, 600.0);
            zoom_to_fit(&mut v, &doc, page);
            assert_eq!((v.zoom, v.cx, v.cy), (1.0, 0.0, 0.0));
        }
    }

    #[test]
    fn zoom_at_clamps_and_ignores_non_finite() {
        let mut v = centred();
        zoom_at(&mut v, 1e9, (400.0, 300.0));
        assert_eq!(v.zoom, 256.0);
        let before = v.clone();
        zoom_at(&mut v, f64::NAN, (400.0, 300.0));
        assert_eq!(v.zoom, before.zoom);
        assert!(all_finite(&v));

        // The rest of what is no zoom at all changes nothing: a factor that is not finite or not
        // above zero, and a point that is not finite.
        for factor in [f64::INFINITY, f64::NEG_INFINITY, 0.0, -2.0] {
            zoom_at(&mut v, factor, (400.0, 300.0));
            assert_eq!(v, before, "factor {factor}");
        }
        for at in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            zoom_at(&mut v, 2.0, (at, 300.0));
            zoom_at(&mut v, 2.0, (400.0, at));
            assert_eq!(v, before, "screen point {at}");
        }

        // The far end the other way, and a factor so big that its product overflows is still
        // "as far as it goes".
        zoom_at(&mut v, 1e-9, (400.0, 300.0));
        assert_eq!(v.zoom, 0.02);
        zoom_at(&mut v, 100.0, (400.0, 300.0));
        zoom_at(&mut v, f64::MAX, (400.0, 300.0));
        assert_eq!(v.zoom, 256.0);
        assert!(all_finite(&v));
    }

    #[test]
    fn dots_double_until_far_enough() {
        assert_eq!(dots(0.25, [255; 3]).unwrap().step, 96.0);
        assert_eq!(dots(1.0, [255; 3]).unwrap().step, 24.0);
    }

    #[test]
    fn dots_are_dark_on_a_light_page_and_light_on_a_dark_one() {
        let light = dots(1.0, [255; 3]).unwrap();
        assert_eq!(light.color.to_rgba8(), [0, 0, 0, 46]); // black at 18%
        assert_eq!(light.faded.to_rgba8(), [0, 0, 0, 33]); // (24 - 14) / 14 of that
        assert_eq!(light.size, 1.5);
        let dark = dots(1.0, [30; 3]).unwrap();
        assert_eq!(dark.color.to_rgba8(), [255, 255, 255, 56]); // white at 22%
        assert_eq!(dark.faded.to_rgba8(), [255, 255, 255, 40]);
        // Luma decides, not the average of the channels: this green is light.
        assert_eq!(dots(1.0, [0, 200, 0]).unwrap().color.to_rgba8()[0], 0);
        assert_eq!(dots(1.0, [0, 0, 255]).unwrap().color.to_rgba8()[0], 255);
    }

    #[test]
    fn dots_size_is_a_point_to_three_on_screen_and_they_fade_in_between_gaps() {
        // Size in page units: 1.5√zoom device px, kept between 1 and 3, over the zoom.
        assert_eq!(dots(0.25, [255; 3]).unwrap().size, 4.0);
        assert_eq!(dots(16.0, [255; 3]).unwrap().size, 3.0 / 16.0);
        // From 28 px apart on screen the dots in between show in full.
        let far_apart = dots(2.0, [255; 3]).unwrap();
        assert_eq!(far_apart.faded, far_apart.color);
        // At the minimum gap they have just faded out.
        let nearest = dots(14.0 / 24.0, [255; 3]).unwrap();
        assert_eq!(nearest.step, 24.0);
        assert_eq!(nearest.faded.to_rgba8()[3], 0);
    }

    #[test]
    fn dots_are_none_without_a_usable_zoom() {
        for zoom in [
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1e-320,
        ] {
            assert!(dots(zoom, [255; 3]).is_none(), "zoom {zoom}");
        }
    }

    #[test]
    fn page_rgb_is_the_pages_background() {
        let (mut doc, page) = page_with(&[(0.0, 0.0, 10.0, 10.0)]);
        // A new page has FigCraft's default background, #F5F5F5.
        assert_eq!(page_rgb(&doc, page), [0xF5; 3]);
        if let Some(NodeKind::Page { background, .. }) = doc.node_mut(page).map(|n| &mut n.kind) {
            *background = Color::from_rgba8(30, 40, 50, 128);
        }
        assert_eq!(page_rgb(&doc, page), [30, 40, 50]);
        // Not a page, or no such node: white.
        let layer = doc.node(page).unwrap().children[0];
        assert_eq!(page_rgb(&doc, layer), [255; 3]);
        assert_eq!(page_rgb(&doc, NodeId(9999)), [255; 3]);
    }

    #[test]
    fn to_bgra_composites_and_swaps() {
        let opaque_red = pixmap(&[[255, 0, 0, 255]]);
        assert_eq!(to_bgra(&opaque_red, [10, 20, 30]), [0, 0, 255, 255]);
        let transparent = pixmap(&[[0, 0, 0, 0]]);
        assert_eq!(to_bgra(&transparent, [10, 20, 30]), [30, 20, 10, 255]);
    }

    #[test]
    fn to_bgra_shows_the_page_through_the_alpha_that_is_left() {
        // Premultiplied: red at half alpha is (128, 0, 0, 128), and half the page shows through.
        let row = pixmap(&[[128, 0, 0, 128], [0, 0, 0, 0], [10, 20, 30, 255]]);
        assert_eq!(
            to_bgra(&row, [255, 255, 255]),
            [127, 127, 255, 255, 255, 255, 255, 255, 30, 20, 10, 255]
        );
        // A pixel can't hold more than its alpha: a broken one is clamped, not wrapped.
        assert_eq!(to_bgra(&pixmap(&[[255, 255, 255, 0]]), [200; 3]), [255; 4]);
    }

    #[test]
    fn a_frame_renders_to_an_opaque_image_of_its_size_with_dots_on_the_page() {
        let (doc, page) = page_with(&[]);
        let rgb = page_rgb(&doc, page);
        let vp = Viewport {
            width: 100.0,
            height: 80.0,
            scale_factor: 1.0,
        };
        let frame = render_view(&view(0.0, 0.0, 1.0, 100.0, 80.0), vp, 1.0).unwrap();
        let opts = figcraft_render::Options {
            background: Some(Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255)),
            dots: dots(1.0, rgb),
            ..Default::default()
        };
        let pixmap = figcraft_render::render_view(&doc, page, &frame, &opts).unwrap();
        assert_eq!(
            (pixmap.width(), pixmap.height()),
            (frame.width, frame.height)
        );

        let bgra = to_bgra(&pixmap, rgb);
        assert_eq!(bgra.len(), (frame.width * frame.height * 4) as usize);
        let (pixels, _) = bgra.as_chunks::<4>();
        assert!(pixels.iter().all(|px| px[3] == 255));
        // An empty light page is the page colour with sparse, darker dots on it.
        let page_px = [rgb[2], rgb[1], rgb[0], 255];
        let darker = pixels.iter().filter(|px| px[2] < rgb[0]).count();
        let other = pixels.iter().filter(|px| **px != page_px).count();
        assert!(darker > 0, "no dots");
        assert_eq!(darker, other, "everything but the page colour is a dot");
        assert!(other < pixels.len() / 10, "{other} of {}", pixels.len());
    }
}
