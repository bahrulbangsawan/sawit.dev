//! Full-page captures: the whole scrollable document, not just the part on show. The page is
//! scrolled through once first so lazy images load (and the network is given a moment), the
//! CSS `vh` unit is held at the device's height, the view is made as tall as the document
//! (fixed and sticky headers then show once, at the top), and WebKit draws it: in one snapshot,
//! or in tiles of at most `MAX_SNAPSHOT_SIDE` pixels stitched into one PNG a strip at a time. A
//! PDF is WebKit's vector rendering of the same tall view. The view's size, the scroll position
//! and `vh` are put back afterwards.

use std::path::PathBuf;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::capture::{
    ImageFormat, MAX_PAGE_HEIGHT, MAX_SNAPSHOT_SIDE, PngStrips, Tile, capture_scale, pixel_size,
    tiles,
};
use sawit_core::browser::device_by_name;
use serde_json::Value;

use super::super::BrowserView;
use super::super::engine::Pixels;
use super::super::shots::within;
use super::{
    CaptureAsk, Captured, capture_dir, css_size, hide_overlays, preview, show_overlays, size_name,
    unique_path,
};

/// The page's height and scroll position, read once.
const MEASURE_JS: &str = "JSON.stringify({h: Math.max(document.documentElement.scrollHeight, document.body ? document.body.scrollHeight : 0, innerHeight), x: scrollX, y: scrollY, vh: innerHeight})";
/// Whether the images have arrived.
const IMAGES_JS: &str = "String(Array.from(document.images).every(i => i.complete))";
/// How long lazy content is waited for, all told.
const LAZY_WAIT: Duration = Duration::from_secs(6);
/// One WebKit capture's limit.
const STEP: Duration = Duration::from_secs(45);

async fn eval(this: &WeakEntity<BrowserView>, script: String, cx: &mut AsyncApp) -> Option<String> {
    let (tx, rx) = oneshot::channel();
    let asked = this.update(cx, |this, _| match &this.engine {
        Some(engine) => {
            engine.eval_text(&script, move |r| {
                let _ = tx.send(r.ok().flatten());
            });
            true
        }
        None => false,
    });
    if !matches!(asked, Ok(true)) {
        return None;
    }
    rx.await.ok().flatten()
}

async fn measure(
    this: &WeakEntity<BrowserView>,
    cx: &mut AsyncApp,
) -> Option<(f64, f64, f64, f64)> {
    let v: Value = serde_json::from_str(&eval(this, MEASURE_JS.into(), cx).await?).ok()?;
    let f = |k: &str| v[k].as_f64().unwrap_or(0.);
    Some((f("h"), f("x"), f("y"), f("vh")))
}

/// Scrolls through the page, a screen at a time, so lazy content loads; then waits (briefly)
/// for the images, and goes back to the top.
async fn load_lazy(this: &WeakEntity<BrowserView>, height: f64, screen: f64, cx: &mut AsyncApp) {
    let step = (screen * 0.85).max(200.);
    let mut y = 0.;
    let started = std::time::Instant::now();
    while y < height.min(f64::from(MAX_PAGE_HEIGHT)) && started.elapsed() < LAZY_WAIT {
        eval(this, format!("scrollTo(0, {y}); 'ok'"), cx).await;
        cx.background_executor()
            .timer(Duration::from_millis(90))
            .await;
        y += step;
    }
    while started.elapsed() < LAZY_WAIT {
        if eval(this, IMAGES_JS.into(), cx).await.as_deref() == Some("true") {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(150))
            .await;
    }
    eval(this, "scrollTo(0, 0); 'ok'".into(), cx).await;
}

/// Before a full-page recording: the page's height, the screen's, and where it was scrolled,
/// with lazy content loaded and the page back at its top.
pub(super) async fn prepare_scroll(
    this: &WeakEntity<BrowserView>,
    cx: &mut AsyncApp,
) -> Option<(f64, f64, f64, f64)> {
    let (h, x, y, vh) = measure(this, cx).await?;
    load_lazy(this, h, vh, cx).await;
    let (h, ..) = measure(this, cx).await.unwrap_or((h, x, y, vh));
    Some((h.min(f64::from(MAX_PAGE_HEIGHT)), vh, x, y))
}

/// Scrolls the page to `x`, `y` CSS px.
pub(super) async fn scroll_to(this: &WeakEntity<BrowserView>, x: f64, y: f64, cx: &mut AsyncApp) {
    eval(this, format!("scrollTo({x}, {y}); 'ok'"), cx).await;
}

/// A full-page capture of the page as it is laid out now.
pub(super) async fn full_page(
    this: &WeakEntity<BrowserView>,
    ask: &CaptureAsk,
    cx: &mut AsyncApp,
) -> Result<Captured, String> {
    let (h0, sx, sy, vh) = measure(this, cx)
        .await
        .ok_or("The page didn't say how tall it is (it may still be loading).")?;
    load_lazy(this, h0, vh, cx).await;
    let (h1, ..) = measure(this, cx).await.unwrap_or((h0, sx, sy, vh));
    let mut notes = Vec::new();
    let css_h = if h1 > f64::from(MAX_PAGE_HEIGHT) {
        notes.push(format!(
            "The page is {} px tall: captured to {MAX_PAGE_HEIGHT} px.",
            h1.round()
        ));
        f64::from(MAX_PAGE_HEIGHT)
    } else {
        h1.max(1.)
    };
    let (show, show_mesurer) = hide_overlays(this, ask.mesurer, cx).await;
    // The view made as tall as the page, `vh` held at the screen's height.
    let set = this.update(cx, |this, _| {
        let engine = this.engine.clone()?;
        let (css_w, screen_h) = css_size(this);
        let view_scale = this.scale();
        engine.hold_viewport_units(f64::from(css_w), f64::from(screen_h));
        let frame = this.page_frame_now()?;
        engine.set_frame(super::super::engine::Frame {
            height: (css_h * view_scale).ceil() as f32,
            ..frame
        });
        let device = size_name(&this.viewport);
        let scale = capture_scale(
            ask.scale,
            Some(device.as_str()).filter(|d| device_by_name(d).is_some()),
        );
        let backing = engine.pixels_per_point();
        Some((
            engine,
            css_w,
            view_scale,
            backing,
            device,
            scale,
            this.project.clone(),
            this.current_url(),
        ))
    });
    let Ok(Some((engine, css_w, view_scale, backing, device, scale, project, url))) = set else {
        show_overlays(this, show, show_mesurer, cx);
        return Err("The browser isn't open.".into());
    };
    cx.background_executor()
        .timer(Duration::from_millis(400))
        .await;
    let css_h_px = css_h.round() as u32;
    let path = match &ask.output_path {
        Some(p) => PathBuf::from(p),
        None => unique_path(
            &capture_dir(project.as_deref())?,
            &url,
            &device,
            scale,
            true,
            ask.format.extension(),
        ),
    };
    let result = match ask.format {
        ImageFormat::Pdf => pdf(&engine, &path, cx)
            .await
            .map(|bytes| (bytes, None, 0, 0)),
        format => {
            let (pw, ph) = pixel_size(css_w, css_h_px, scale, None, false);
            let width_pts = f64::from(pw) / backing.max(0.5);
            let plan = tiles(css_h_px, scale, MAX_SNAPSHOT_SIDE);
            let mut shots: Vec<(Tile, Pixels)> = Vec::new();
            let mut failed = None;
            for tile in &plan {
                let rect = (plan.len() > 1).then(|| {
                    (
                        0.,
                        f64::from(tile.top) * view_scale,
                        f64::from(css_w) * view_scale,
                        f64::from(tile.height) * view_scale,
                    )
                });
                let (tx, rx) = oneshot::channel();
                engine.capture_rect(rect, width_pts, tx);
                let timer = cx.background_executor().timer(STEP);
                match within(rx, timer).await {
                    Some(Ok(Ok(p))) => {
                        // Exactly the tile's size: WebKit rounds to its own pixel grid.
                        let th = if plan.len() > 1 {
                            (f64::from(tile.height) * scale).round() as u32
                        } else {
                            ph
                        };
                        let p = p.resized(pw, th).unwrap_or(p);
                        shots.push((*tile, p))
                    }
                    Some(Ok(Err(e))) => {
                        failed = Some(e);
                        break;
                    }
                    _ => {
                        failed = Some(format!(
                            "WebKit didn't draw the page within {} s.",
                            STEP.as_secs()
                        ));
                        break;
                    }
                }
            }
            match failed {
                Some(e) => Err(e),
                None => {
                    let ph: u32 = shots.iter().map(|(_, p)| p.size().1).sum();
                    let quality = ask.quality;
                    let path = path.clone();
                    cx.background_executor()
                        .spawn(async move { encode(shots, format, quality, pw, ph, &path) })
                        .await
                        .map(|(bytes, preview)| (bytes, preview, pw, ph))
                }
            }
        }
    };
    // Everything put back.
    let _ = this.update(cx, |this, _| {
        this.relayout();
    });
    eval(this, format!("scrollTo({sx}, {sy}); 'ok'"), cx).await;
    show_overlays(this, show, show_mesurer, cx);
    let (bytes, preview_png, w, h) = result?;
    Ok(Captured {
        path,
        width: w,
        height: h,
        css_width: css_w,
        css_height: css_h_px,
        scale,
        device,
        bytes,
        preview_png,
        notes,
    })
}

/// The tiles stitched into one file: PNG in strips (no whole picture in memory), JPEG through
/// one bitmap (capped at JPEG's 65,535 px).
fn encode(
    shots: Vec<(Tile, Pixels)>,
    format: ImageFormat,
    quality: Option<f64>,
    width: u32,
    height: u32,
    path: &std::path::Path,
) -> Result<(u64, Option<Vec<u8>>), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if shots.len() == 1 {
        let (_, pixels) = &shots[0];
        let bytes = match format {
            ImageFormat::Jpeg => pixels.jpeg(quality.unwrap_or(0.9))?,
            _ => pixels.png()?,
        };
        std::fs::write(path, &bytes).map_err(|e| e.to_string())?;
        return Ok((bytes.len() as u64, preview(&bytes)));
    }
    match format {
        ImageFormat::Jpeg => {
            if height > 65_535 {
                return Err("The page is too tall for a JPEG: use png.".into());
            }
            let mut all = image::RgbaImage::new(width, height);
            let mut top = 0;
            for (_, pixels) in &shots {
                let (w, h) = pixels.size();
                let rgba = pixels.rgba().ok_or("A tile couldn't be read.")?;
                let tile =
                    image::RgbaImage::from_raw(w, h, rgba).ok_or("A tile couldn't be read.")?;
                image::imageops::replace(&mut all, &tile, 0, i64::from(top));
                top += h;
            }
            let mut bytes = Vec::new();
            let rgb = image::DynamicImage::ImageRgba8(all).to_rgb8();
            let q = (quality.unwrap_or(0.9) * 100.).clamp(10., 100.) as u8;
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, q)
                .encode_image(&rgb)
                .map_err(|e| e.to_string())?;
            std::fs::write(path, &bytes).map_err(|e| e.to_string())?;
            Ok((bytes.len() as u64, preview(&bytes)))
        }
        _ => {
            let mut out = PngStrips::create(path, width, height)?;
            for (_, pixels) in &shots {
                let rgba = pixels.rgba().ok_or("A tile couldn't be read.")?;
                out.rows(&rgba)?;
            }
            out.finish()?;
            let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let preview = std::fs::read(path).ok().and_then(|b| preview(&b));
            Ok((bytes, preview))
        }
    }
}

async fn pdf(
    engine: &std::rc::Rc<super::super::engine::Engine>,
    path: &std::path::Path,
    cx: &mut AsyncApp,
) -> Result<u64, String> {
    let (tx, rx) = oneshot::channel();
    engine.create_pdf(tx);
    let timer = cx.background_executor().timer(STEP);
    let data = match within(rx, timer).await {
        Some(Ok(r)) => r?,
        _ => return Err("WebKit didn't make the PDF in time.".into()),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, &data).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok(data.len() as u64)
}
