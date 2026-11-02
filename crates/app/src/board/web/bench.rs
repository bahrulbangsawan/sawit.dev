//! Painting the web frames' textures between the canvas's frame and its overlay, and the bench
//! that pans or zooms the canvas by itself for a number of frames and times each one.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use figcraft_engine::ViewState;
use gpui_kit::*;
use sawit_board::view;
use serde_json::{Value, json};

use super::{BoardView, WebFrames, ms};
use crate::board::ZOOM_SETTLE;

/// A texture to paint, in the window, and its corners.
pub(in crate::board) struct Item {
    pub image: Arc<RenderImage>,
    pub at: Bounds<Pixels>,
    pub radii: Corners<Pixels>,
}

/// What the web frames paint this frame.
#[derive(Default)]
pub(in crate::board) struct Painted {
    pub items: Vec<Item>,
    /// Replaced textures, out of the sprite atlas.
    pub stale: Vec<Arc<RenderImage>>,
    /// The bench's marker beside the live view, in GPUI's drawing: how far the view trails it.
    pub marker: Option<Bounds<Pixels>>,
    /// Where the bench wants the paint's times: whole paint, web textures (ms).
    pub times: Option<Rc<Cell<(f64, f64)>>>,
}

impl Painted {
    pub(in crate::board) fn times(&self) -> Option<Rc<Cell<(f64, f64)>>> {
        self.times.clone()
    }
}

/// Paints `painted`; how long it took, in ms.
pub(in crate::board) fn paint(painted: Painted, window: &mut Window) -> f64 {
    let started = Instant::now();
    for image in painted.stale {
        let _ = window.drop_image(image);
    }
    for item in painted.items {
        if let Err(error) = window.paint_image(item.at, item.at, item.radii, item.image, 0, false) {
            log::error!("a web frame couldn't be painted: {error}");
        }
    }
    if let Some(marker) = painted.marker {
        window.paint_quad(fill(marker, rgb(0xff0000)));
    }
    ms(started.elapsed())
}

/// A page rectangle (x0, y0, x1, y1) on screen, in the window, the canvas at `origin`.
pub(super) fn screen_bounds(
    rect: [f64; 4],
    view: &ViewState,
    origin: Point<Pixels>,
) -> Bounds<Pixels> {
    let (x0, y0) = view.page_to_screen(rect[0], rect[1]);
    let (x1, y1) = view.page_to_screen(rect[2], rect[3]);
    Bounds::new(
        point(origin.x + px(x0 as f32), origin.y + px(y0 as f32)),
        size(px((x1 - x0) as f32), px((y1 - y0) as f32)),
    )
}

/// A frame's corner radii (top-left, top-right, bottom-right, bottom-left) on screen at `zoom`.
pub(super) fn corners(radii: [f64; 4], zoom: f64) -> Corners<Pixels> {
    let at = |r: f64| px((r * zoom) as f32);
    Corners {
        top_left: at(radii[0]),
        top_right: at(radii[1]),
        bottom_right: at(radii[2]),
        bottom_left: at(radii[3]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::board) enum Kind {
    Pan,
    Zoom,
    Still,
}

/// A bench run: `total` frames, the view panned `speed` points a frame (or zoomed by `speed`% a
/// frame), one way for the first half and back for the second.
pub(in crate::board) struct Bench {
    pub kind: Kind,
    pub total: u32,
    pub done: u32,
    pub speed: f64,
    /// Paint the textures (off: the canvas alone, to compare).
    pub textures: bool,
    /// Paint the marker beside the live view.
    pub marker: bool,
    last: Option<Instant>,
    prepaint: f64,
    /// Per frame: interval since the last, prepaint, paint, web paint (ms).
    samples: Vec<[f64; 4]>,
}

impl Bench {
    pub(super) fn new(kind: Kind, total: u32, speed: f64, textures: bool, marker: bool) -> Bench {
        Bench {
            kind,
            total,
            done: 0,
            speed,
            textures,
            marker,
            last: None,
            prepaint: 0.0,
            samples: Vec::new(),
        }
    }

    /// The run's numbers: mean, median, 95th percentile and maximum of each column, and the
    /// frames that took longer than 1.5 median intervals.
    fn report(&self, frames: usize) -> Value {
        let column = |i: usize| {
            let mut v: Vec<f64> = self.samples.iter().map(|s| s[i]).collect();
            stats(&mut v)
        };
        let mut intervals: Vec<f64> = self.samples.iter().map(|s| s[0]).collect();
        let median = stats(&mut intervals)["p50"].as_f64().unwrap_or(0.0);
        let long = intervals.iter().filter(|i| **i > median * 1.5).count();
        json!({"event": "bench", "kind": format!("{:?}", self.kind), "frames": self.samples.len(),
            "speed": self.speed, "textures": self.textures, "web_frames": frames,
            "interval_ms": column(0), "prepaint_ms": column(1), "paint_ms": column(2),
            "web_paint_ms": column(3), "long_frames": long})
    }
}

/// Mean, p50, p95 and max of `values`, sorted in place.
pub(super) fn stats(values: &mut [f64]) -> Value {
    if values.is_empty() {
        return json!(null);
    }
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let round = |v: f64| (v * 100.0).round() / 100.0;
    json!({"mean": round(mean), "p50": round(at(0.5)), "p95": round(at(0.95)), "max": round(at(1.0))})
}

impl WebFrames {
    /// The marker for the live-view bench: a bar right against the live frame's left edge as GPUI
    /// draws it this frame (where the live view goes, or with the lag where it goes next frame).
    pub(super) fn marker(&self) -> Option<Bounds<Pixels>> {
        self.bench.as_ref().filter(|bench| bench.marker)?;
        let live = self.live.as_ref()?;
        let (frame, _) = live.next.or(live.placed)?;
        let origin = self.canvas.origin;
        Some(Bounds::new(
            point(origin.x + px(frame.x - 12.0), origin.y + px(frame.y)),
            size(px(12.0), px(frame.height)),
        ))
    }
}

impl BoardView {
    /// Before each prepaint while a bench runs: notes the last frame's times, then moves the view
    /// one step and asks for the next frame. At the end the report goes to the log.
    pub(in crate::board) fn web_bench_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let frames = web.frames.len();
        let (paint, web_paint) = web.paint_times.get();
        let Some(bench) = web.bench.as_mut() else {
            return;
        };
        let now = Instant::now();
        if let Some(last) = bench.last {
            bench
                .samples
                .push([ms(now - last), bench.prepaint, paint, web_paint]);
        }
        bench.last = Some(now);
        if bench.done >= bench.total {
            let report = bench.report(frames);
            web.bench = None;
            web.last_bench = Some(report.clone());
            web.note(report);
            return;
        }
        let way = if bench.done < bench.total / 2 {
            1.0
        } else {
            -1.0
        };
        bench.done += 1;
        let (kind, speed) = (bench.kind, bench.speed);
        let centre = (
            f64::from(self.bounds.size.width) / 2.0,
            f64::from(self.bounds.size.height) / 2.0,
        );
        if let Some(view_state) = self.view_mut() {
            match kind {
                Kind::Pan => view::pan(view_state, -way * speed, 0.0),
                Kind::Zoom => view::zoom_at(view_state, (way * speed / 100.0).exp(), centre),
                Kind::Still => {}
            }
        }
        if kind == Kind::Zoom {
            self.zooming_until = Some(cx.background_executor().now() + ZOOM_SETTLE);
        }
        window.request_animation_frame();
    }

    /// How long the prepaint that just ran took, for the bench.
    pub(in crate::board) fn web_prepaint_took(&mut self, started: Instant) {
        if let Some(bench) = self.web.as_mut().and_then(|web| web.bench.as_mut()) {
            bench.prepaint = ms(started.elapsed());
        }
    }
}
