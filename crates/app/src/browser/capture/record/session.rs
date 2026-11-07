//! A recording's run: the page laid out at the size asked for, the box recorded, the writer
//! started, and the frames taken in their own task until the recording stops.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::capture::{
    VideoFormat, capture_scale, resolution, video_format_for, video_size,
};
use sawit_core::browser::{ViewportMode, device_by_name};
use serde_json::{Value, json};

use super::super::super::engine::video::{Frame, VideoSpec, VideoWriter, Written};
use super::super::super::ops::{element_rect, rect_op};
use super::super::super::shots::{MesurerShot, within};
use super::super::super::{BrowserView, OpResult};
use super::super::{
    capture_dir, css_size, hide_overlays, settle_width, show_overlays, size_name, unique_path,
};
use super::{FRAME_WAIT, GIF_MAX_SECONDS, GIF_MAX_WIDTH, RecordAsk, RecordMode, Recording, Scroll};

pub(super) async fn start(
    this: &WeakEntity<BrowserView>,
    ask: RecordAsk,
    mode: Option<ViewportMode>,
    cx: &mut AsyncApp,
) -> OpResult {
    let old = match &mode {
        Some(m) => Some(
            this.update(cx, |this, cx| this.borrow_viewport(m.clone(), cx))
                .map_err(|_| "The browser closed.")?,
        ),
        None => None,
    };
    let want = mode.as_ref().and_then(|m| match m {
        ViewportMode::Fixed { width, .. } => Some(*width),
        _ => None,
    });
    settle_width(this, want, cx).await;
    // The box recorded, in CSS px: an element's, a clip, or the view.
    let rect = match ask.mode {
        RecordMode::Element => {
            let rx = this
                .update(cx, |this, _| this.page_op(rect_op(&ask.target)))
                .map_err(|_| "The browser closed.")?;
            Some(element_rect(
                &rx.await.map_err(|_| "No answer from the page.")??,
            )?)
        }
        RecordMode::Region => ask.clip.map(|[x, y, w, h]| (x, y, w, h)),
        _ => None,
    };
    let setup = this.update(cx, |this, _| {
        let engine = this.engine.clone()?;
        let (vw, vh) = css_size(this);
        let device = size_name(&this.viewport);
        let view_scale = this.scale();
        let backing = engine.pixels_per_point();
        Some((
            vw,
            vh,
            device,
            view_scale,
            backing,
            this.project.clone(),
            this.current_url(),
        ))
    });
    let Ok(Some((vw, vh, device, view_scale, backing, project, url))) = setup else {
        return Err("The browser isn't open.".into());
    };
    let (cw, ch) = match rect {
        Some((_, _, w, h)) => (w.round().max(2.) as u32, h.round().max(2.) as u32),
        None => (vw, vh),
    };
    let scale = capture_scale(
        None,
        Some(device.as_str()).filter(|d| device_by_name(d).is_some()),
    );
    let max_width = match ask.format {
        VideoFormat::Gif => Some(ask.max_width.unwrap_or(GIF_MAX_WIDTH).min(GIF_MAX_WIDTH)),
        _ => ask.max_width,
    };
    let (w, h) = video_size(
        cw,
        ch,
        scale,
        ask.resolution.as_deref().and_then(resolution),
        max_width,
    );
    let (format, note) = video_format_for(ask.format, w);
    let max_seconds = match format {
        VideoFormat::Gif => ask.max_seconds.min(GIF_MAX_SECONDS),
        _ => ask.max_seconds,
    };
    let path = match &ask.output_path {
        Some(p) => PathBuf::from(p).with_extension(format.extension()),
        None => unique_path(
            &capture_dir(project.as_deref())?,
            &url,
            &device,
            f64::from(w) / f64::from(cw.max(1)),
            ask.mode == RecordMode::FullPage,
            format.extension(),
        ),
    };
    let spec = VideoSpec {
        path: path.clone(),
        format,
        width: w,
        height: h,
        fps: ask.fps,
        bitrate: None,
    };
    let writer = cx
        .background_executor()
        .spawn({
            let spec = spec.clone();
            async move { VideoWriter::start(spec) }
        })
        .await?;
    let (show, show_mesurer) = hide_overlays(this, MesurerShot::Hide, cx).await;
    let full = if ask.mode == RecordMode::FullPage {
        super::super::full::prepare_scroll(this, cx).await
    } else {
        None
    };
    let scroll = full.map(|(height, screen, _, _)| {
        Scroll::plan(
            (height - screen).max(0.),
            ask.scroll_speed,
            ask.duration,
            f64::from(max_seconds),
        )
    });
    let (stop_tx, stop_rx) = oneshot::channel();
    let started = Instant::now();
    let recording = Recording {
        started,
        path: path.clone(),
        width: w,
        height: h,
        fps: ask.fps,
        format,
        mode: ask.mode,
        max: Duration::from_secs(u64::from(max_seconds)),
        stop: Some(stop_tx),
        waiters: Vec::new(),
    };
    let status = recording.status();
    this.update(cx, |this, cx| {
        this.recorder.current = Some(recording);
        cx.notify();
    })
    .map_err(|_| "The browser closed.")?;
    // The frames, in their own task: it outlives the tab, so the file is finished whatever.
    let points = rect.map(|(x, y, rw, rh)| {
        (
            x * view_scale,
            y * view_scale,
            rw * view_scale,
            rh * view_scale,
        )
    });
    let width_pts = f64::from(w) / backing.max(0.5);
    let this2 = this.clone();
    let restore = (old, show, show_mesurer, full.map(|(_, _, x, y)| (x, y)));
    cx.spawn(async move |cx| {
        let ended = run(
            &this2,
            writer,
            stop_rx,
            started,
            Duration::from_secs(u64::from(max_seconds)),
            ask.fps,
            points,
            width_pts,
            scroll,
            cx,
        )
        .await;
        let (old, show, show_mesurer, scrolled) = restore;
        if let Some((x, y)) = scrolled {
            super::super::full::scroll_to(&this2, x, y, cx).await;
        }
        show_overlays(&this2, show, show_mesurer, cx);
        let result = ended
            .map(|(written, dropped)| done_json(&written, w, h, format, dropped, note.clone()));
        let _ = this2.update(cx, |this, cx| {
            if let Some(old) = old {
                this.set_viewport(old, cx);
            }
            if let Some(r) = this.recorder.current.take() {
                for waiter in r.waiters {
                    let _ = waiter.send(result.clone());
                }
            }
            this.recorder.last = Some(result);
            cx.notify();
        });
    })
    .detach();
    Ok(status)
}

fn done_json(
    w: &Written,
    width: u32,
    height: u32,
    format: VideoFormat,
    dropped: u32,
    note: Option<String>,
) -> Value {
    let secs = w.duration.as_secs_f64().max(0.001);
    json!({
        "path": w.path.display().to_string(),
        "duration_seconds": (secs * 100.).round() / 100.,
        "width": width,
        "height": height,
        "frames": w.frames,
        "dropped": dropped,
        "fps_achieved": ((f64::from(w.frames) / secs) * 10.).round() / 10.,
        "codec": format.codec(),
        "format": format.extension(),
        "bytes": w.bytes,
        "note": note,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run(
    this: &WeakEntity<BrowserView>,
    writer: VideoWriter,
    mut stop: oneshot::Receiver<()>,
    started: Instant,
    max: Duration,
    fps: u32,
    rect: Option<(f64, f64, f64, f64)>,
    width_pts: f64,
    scroll: Option<Scroll>,
    cx: &mut AsyncApp,
) -> Result<(Written, u32), String> {
    let tick = Duration::from_micros(1_000_000 / u64::from(fps.max(1)));
    let mut dropped = 0u32;
    let mut next = Instant::now();
    loop {
        if !matches!(stop.try_recv(), Ok(None)) || started.elapsed() >= max {
            break;
        }
        if let Some(s) = scroll {
            let (y, over) = s.at(started.elapsed().as_secs_f64());
            if over {
                break;
            }
            super::super::full::scroll_to(this, 0., y, cx).await;
        }
        let (tx, rx) = oneshot::channel();
        let asked = this.update(cx, |this, _| match &this.engine {
            Some(engine) => {
                engine.record_frame(rect, width_pts, tx);
                true
            }
            None => false,
        });
        // The tab went away: the file is finished with what there is.
        if !matches!(asked, Ok(true)) {
            break;
        }
        let at = started.elapsed();
        let timer = cx.background_executor().timer(FRAME_WAIT);
        match within(rx, timer).await {
            Some(Ok(Ok(pixels))) => {
                if !writer.push(Frame { pixels, at }) {
                    dropped += 1;
                }
            }
            _ => dropped += 1,
        }
        next += tick;
        let now = Instant::now();
        if next > now {
            cx.background_executor().timer(next - now).await;
        } else {
            next = now;
        }
    }
    let written = cx
        .background_executor()
        .spawn(async move { writer.finish() })
        .await?;
    Ok((written, dropped))
}
