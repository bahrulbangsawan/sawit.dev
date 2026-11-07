//! Native captures at a chosen size (Part D): the page laid out as a device or a width × height
//! for the shot, drawn afresh by WebKit at the scale asked for (so a 3× phone shot is 3× on a 1×
//! screen too), the view, an element or a clip of it, as PNG or JPEG, written to the project's
//! screenshot folder with the device and size in its name. The tab's own size comes back after.
//! The numbers are `sawit_core::browser::capture`'s. Full-page captures are [`full`]'s.

mod full;
pub mod record;

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::capture::{
    INLINE_MAX_WIDTH, ImageFormat, capture_scale, file_name, pixel_size,
};
use sawit_core::browser::{Target, ViewportMode, base64_encode, device_by_name, resize_mode};
use serde_json::{Value, json};

use super::engine::Pixels;
use super::ops::{element_rect, rect_op};
use super::shots::{MesurerShot, shoot_ops, within};
use super::{BrowserView, OpResult};

/// How long the page is given to lay out at a new size.
const LAYOUT_WAIT: Duration = Duration::from_secs(3);
/// How long one WebKit capture may take.
const STEP: Duration = Duration::from_secs(20);

/// What a capture is asked for.
#[derive(Clone, Debug, Default)]
pub struct CaptureAsk {
    pub target: Target,
    pub full_page: bool,
    pub device: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub scale: Option<f64>,
    pub format: ImageFormat,
    pub quality: Option<f64>,
    pub clip: Option<[f64; 4]>,
    pub output_path: Option<String>,
    pub mesurer: MesurerShot,
}

impl CaptureAsk {
    /// The size the page is laid out at for the shot, when one is asked for.
    fn mode(&self) -> Result<Option<ViewportMode>, String> {
        if self.device.is_none() && self.width.is_none() && self.height.is_none() {
            return Ok(None);
        }
        resize_mode(self.device.as_deref(), self.width, self.height).map(Some)
    }
}

/// What the capture made.
#[derive(Clone, Debug)]
pub struct Captured {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub css_width: u32,
    pub css_height: u32,
    pub scale: f64,
    pub device: String,
    pub bytes: u64,
    pub preview_png: Option<Vec<u8>>,
    /// What the capture had to do differently (a page cut at the height cap).
    pub notes: Vec<String>,
}

impl Captured {
    pub fn json(&self) -> Value {
        json!({
            "path": self.path.display().to_string(),
            "width": self.width,
            "height": self.height,
            "css_width": self.css_width,
            "css_height": self.css_height,
            "scale": self.scale,
            "device": self.device,
            "bytes": self.bytes,
            "preview_png_base64": self.preview_png.as_deref().map(base64_encode),
            "notes": self.notes,
        })
    }
}

/// A file that doesn't replace another: `name`, else `name` with `-2`, `-3`… before the
/// extension.
pub(crate) fn unique_path(
    dir: &Path,
    url: &str,
    device: &str,
    scale: f64,
    full: bool,
    ext: &str,
) -> PathBuf {
    for n in 1..1000 {
        let path = dir.join(file_name(url, device, scale, full, ext, n));
        if !path.exists() {
            return path;
        }
    }
    dir.join(file_name(url, device, scale, full, ext, 1000))
}

/// The folder captures go to: the project's day's screenshot folder, else the app cache's.
pub(crate) fn capture_dir(project: Option<&Path>) -> Result<PathBuf, String> {
    let cache = sawit_core::instances::AppPaths::default_macos().cache_dir();
    let dir =
        sawit_core::annotations::screenshot_dir(project, &cache, &sawit_core::date::local_date());
    match project {
        Some(project) => sawit_core::nyawit::ensure(project, &dir),
        None => std::fs::create_dir_all(&dir),
    }
    .map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Encodes `pixels` and writes them to `path`; a downscaled PNG preview for agents' inline view.
fn write(
    pixels: &Pixels,
    format: ImageFormat,
    quality: Option<f64>,
    path: &Path,
) -> Result<(u64, Option<Vec<u8>>), String> {
    let bytes = match format {
        ImageFormat::Jpeg => pixels.jpeg(quality.unwrap_or(0.9))?,
        _ => pixels.png()?,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    }
    std::fs::write(path, &bytes).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok((bytes.len() as u64, preview(&bytes)))
}

/// A PNG at most [`INLINE_MAX_WIDTH`] wide (and 4000 tall) of an encoded picture, for agents
/// whose image limits are smaller than the file.
pub(crate) fn preview(encoded: &[u8]) -> Option<Vec<u8>> {
    let picture = image::load_from_memory(encoded).ok()?;
    let (w, h) = (picture.width(), picture.height());
    let picture = if w > INLINE_MAX_WIDTH || h > 4000 {
        picture.resize(
            INLINE_MAX_WIDTH,
            4000,
            image::imageops::FilterType::Triangle,
        )
    } else {
        picture
    };
    let mut out = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

impl BrowserView {
    /// `Capture`: the shot asked for, in the screenshots' turn.
    pub(in crate::browser) fn capture_op(
        &mut self,
        ask: CaptureAsk,
        reply: oneshot::Sender<OpResult>,
        cx: &mut Context<Self>,
    ) {
        let mode = match ask.mode() {
            Ok(m) => m,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        if ask.format == ImageFormat::Pdf && !ask.full_page {
            let _ = reply.send(Err("PDF is for full_page captures.".into()));
            return;
        }
        if ask.full_page && (!ask.target.is_empty() || ask.clip.is_some()) {
            let _ = reply.send(Err(
                "full_page captures the whole page: leave ref, selector and clip out.".into(),
            ));
            return;
        }
        let turn = self.shots.enter();
        cx.spawn(async move |this, cx| {
            if let Some(turn) = turn {
                let _ = turn.await;
            }
            let result = capture(&this, ask, mode, cx).await;
            let _ = this.update(cx, |this, _| this.shots.leave());
            let _ = reply.send(result.map(|c| c.json()));
        })
        .detach();
    }

    /// The page laid out at `mode` for a shot: the mode it had, to put back.
    pub(in crate::browser) fn borrow_viewport(
        &mut self,
        mode: ViewportMode,
        cx: &mut Context<Self>,
    ) -> ViewportMode {
        let old = self.viewport.clone();
        if old != mode {
            self.set_viewport(mode, cx);
        }
        old
    }
}

/// Waits until the page's `innerWidth` is `width` CSS px (or [`LAYOUT_WAIT`] passes), two frames
/// more for it to paint.
pub(in crate::browser) async fn settle_width(
    this: &WeakEntity<BrowserView>,
    width: Option<u32>,
    cx: &mut AsyncApp,
) {
    let started = std::time::Instant::now();
    loop {
        let (tx, rx) = oneshot::channel();
        let asked = this.update(cx, |this, _| match &this.engine {
            Some(engine) => {
                engine.eval_text("String(innerWidth)", move |r| {
                    let _ = tx.send(r.ok().flatten());
                });
                true
            }
            None => false,
        });
        if !matches!(asked, Ok(true)) {
            return;
        }
        let now = rx.await.ok().flatten().and_then(|w| w.parse::<u32>().ok());
        if width.is_none() || now == width || started.elapsed() > LAYOUT_WAIT {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(60))
            .await;
    }
    cx.background_executor()
        .timer(Duration::from_millis(120))
        .await;
}

/// Hides the overlay and mesurer as `mesurer` says; the ops that show them again.
pub(in crate::browser) async fn hide_overlays(
    this: &WeakEntity<BrowserView>,
    mesurer: MesurerShot,
    cx: &mut AsyncApp,
) -> (Value, Option<&'static str>) {
    let keep = this.update(cx, |_, cx| crate::agents::in_screenshots(cx));
    let [hide, show] = shoot_ops(keep.unwrap_or(false));
    let mesurer = this
        .update(cx, |this, _| mesurer.hides(this.mesurer.mounted))
        .ok()
        .flatten();
    let mut show_mesurer = None;
    if let Some([hide_mesurer, show]) = mesurer {
        show_mesurer = Some(show);
        if let Ok(Some(hidden)) = this.update(cx, |this, _| this.mesurer_hide(hide_mesurer)) {
            let timer = cx.background_executor().timer(Duration::from_secs(5));
            let _ = within(hidden, timer).await;
        }
    }
    if let Ok(hidden) = this.update(cx, |this, _| this.page_op(hide)) {
        let timer = cx.background_executor().timer(Duration::from_secs(5));
        let _ = within(hidden, timer).await;
    }
    (show, show_mesurer)
}

pub(in crate::browser) fn show_overlays(
    this: &WeakEntity<BrowserView>,
    show: Value,
    mesurer: Option<&'static str>,
    cx: &mut AsyncApp,
) {
    let _ = this.update(cx, |this, _| {
        drop(this.page_op(show));
        if let Some(m) = mesurer {
            drop(this.mesurer_hide(m));
        }
    });
}

/// The CSS size the page lays out at now: the device's, else the tab's.
pub(super) fn css_size(view: &BrowserView) -> (u32, u32) {
    match &view.viewport {
        ViewportMode::Fixed { width, height, .. } => (*width, *height),
        ViewportMode::Responsive => view.bounds.map_or((1024, 768), |b| {
            (
                f32::from(b.size.width).round() as u32,
                f32::from(b.size.height).round() as u32,
            )
        }),
    }
}

/// The name a capture gives its size: the device's, else `responsive` or `W x H`.
pub(super) fn size_name(mode: &ViewportMode) -> String {
    match mode {
        ViewportMode::Fixed { name: Some(n), .. } => n.clone(),
        ViewportMode::Fixed { width, height, .. } => format!("{width}x{height}"),
        ViewportMode::Responsive => "responsive".into(),
    }
}

async fn capture(
    this: &WeakEntity<BrowserView>,
    ask: CaptureAsk,
    mode: Option<ViewportMode>,
    cx: &mut AsyncApp,
) -> Result<Captured, String> {
    let old = match &mode {
        Some(mode) => Some(
            this.update(cx, |this, cx| this.borrow_viewport(mode.clone(), cx))
                .map_err(|_| "The browser closed.")?,
        ),
        None => None,
    };
    let want_width = mode.as_ref().and_then(|m| match m {
        ViewportMode::Fixed { width, .. } => Some(*width),
        _ => None,
    });
    settle_width(this, want_width, cx).await;
    let result = if ask.full_page {
        full::full_page(this, &ask, cx).await
    } else {
        shot(this, &ask, cx).await
    };
    if let Some(old) = old {
        let _ = this.update(cx, |this, cx| this.set_viewport(old, cx));
    }
    result
}

async fn shot(
    this: &WeakEntity<BrowserView>,
    ask: &CaptureAsk,
    cx: &mut AsyncApp,
) -> Result<Captured, String> {
    // The element's box first, scrolled into view.
    let rect = if !ask.target.is_empty() {
        let rx = this
            .update(cx, |this, _| this.page_op(rect_op(&ask.target)))
            .map_err(|_| "The browser closed.")?;
        let r = rx.await.map_err(|_| "No answer from the page.")??;
        Some(element_rect(&r)?)
    } else {
        ask.clip.map(|[x, y, w, h]| (x, y, w, h))
    };
    let (show, show_mesurer) = hide_overlays(this, ask.mesurer, cx).await;
    let started = this.update(cx, |this, _| {
        let engine = this.engine.clone()?;
        let (css_w, css_h) = css_size(this);
        let device = size_name(&this.viewport);
        let view_scale = this.scale();
        let backing = engine.pixels_per_point();
        let scale = capture_scale(
            ask.scale,
            Some(&device)
                .filter(|d| device_by_name(d).is_some())
                .map(String::as_str),
        );
        let (cw, ch) = match rect {
            Some((_, _, w, h)) => (w.round().max(1.) as u32, h.round().max(1.) as u32),
            None => (css_w, css_h),
        };
        let (pw, ph) = pixel_size(cw, ch, scale, None, false);
        let points = rect.map(|(x, y, w, h)| {
            (
                x * view_scale,
                y * view_scale,
                w * view_scale,
                h * view_scale,
            )
        });
        let (tx, rx) = oneshot::channel();
        engine.capture_rect(points, f64::from(pw) / backing.max(0.5), tx);
        Some((
            rx,
            cw,
            ch,
            pw,
            ph,
            scale,
            device,
            this.project.clone(),
            this.current_url(),
        ))
    });
    let Ok(Some((rx, cw, ch, pw, ph, scale, device, project, url))) = started else {
        show_overlays(this, show, show_mesurer, cx);
        return Err("The browser isn't open.".into());
    };
    let timer = cx.background_executor().timer(STEP);
    let pixels = match within(rx, timer).await {
        Some(Ok(p)) => p,
        Some(Err(_)) => Err("The capture was cancelled.".to_string()),
        None => Err(format!(
            "WebKit didn't capture the page within {} s.",
            STEP.as_secs()
        )),
    };
    show_overlays(this, show, show_mesurer, cx);
    // WebKit rounds to its own pixel grid: the picture is brought to the exact size asked for.
    let pixels = pixels?;
    let pixels = pixels.resized(pw, ph).unwrap_or(pixels);
    let path = match &ask.output_path {
        Some(p) => PathBuf::from(p),
        None => unique_path(
            &capture_dir(project.as_deref())?,
            &url,
            &device,
            scale,
            false,
            ask.format.extension(),
        ),
    };
    let (format, quality) = (ask.format, ask.quality);
    let written = cx
        .background_executor()
        .spawn(async move {
            let (w, h) = pixels.size();
            write(&pixels, format, quality, &path)
                .map(|(bytes, preview)| (path, w, h, bytes, preview))
        })
        .await?;
    Ok(Captured {
        path: written.0,
        width: written.1,
        height: written.2,
        css_width: cw,
        css_height: ch,
        scale,
        device,
        bytes: written.3,
        preview_png: written.4,
        notes: Vec::new(),
    })
}
