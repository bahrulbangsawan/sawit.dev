//! The web frames' bookkeeping: a frame for each web frame on the page, the views built while
//! the pool has room ([`super::Config::pool`]) and let go of beyond it (their textures stay), the frames
//! to snapshot again at a settled zoom, and the textures to paint.

use std::collections::HashSet;
use std::time::Instant;

use figcraft_engine::ViewState;
use gpui_kit::*;
use sawit_board::screens;
use serde_json::json;

use super::bench::{Item, Painted, corners, screen_bounds};
use super::settle::INIT;
use super::{Phase, Settle, Source, ViewFrame, Viewport, WebFrame, WebFrames, WebMsg, WebNode, ms};
use crate::browser::engine::web_frame::ScreenSource;
use crate::browser::engine::{Engine, EngineSpec};

impl WebFrames {
    /// Points per CSS px for a snapshot now: the zoom, within 0.1–2, or the fixed scale.
    pub(super) fn snapshot_scale(&self) -> f64 {
        self.config.scale.unwrap_or(self.zoom.clamp(0.1, 2.0))
    }

    /// Where a view waits off the canvas, in the host, at its viewport's size.
    pub(super) fn parking(&self, viewport: Viewport) -> ViewFrame {
        let (width, height) = (viewport.w as f32, viewport.h as f32);
        match self.config.settle {
            // Beyond the window's right edge.
            Settle::OffWindow => ViewFrame {
                x: self.window_width - f32::from(self.canvas.origin.x) + 64.0,
                y: 0.0,
                width,
                height,
            },
            // Right above the canvas, over the top bar and the title bar: in the window, outside
            // the host's clip, so a view the clip let through would show there.
            Settle::OffCanvas | Settle::Hidden => ViewFrame {
                x: 0.0,
                y: -height,
                width,
                height,
            },
        }
    }

    /// Keeps a frame for each web frame on the page; one whose source changed starts over, and a
    /// screen whose frame was resized lays its page out again at the new size.
    pub(super) fn sync(&mut self, nodes: &[WebNode]) {
        let on_page: HashSet<_> = nodes.iter().map(|node| node.id).collect();
        let gone: Vec<_> = (self.frames.keys())
            .filter(|id| !on_page.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(frame) = self.frames.remove(&id) {
                self.stale
                    .extend(frame.texture.map(|texture| texture.image));
            }
            if self.live.as_ref().is_some_and(|live| live.id == id) {
                self.live = None;
            }
        }
        let mut resized = Vec::new();
        for node in nodes {
            let has_page = has_page(&self.dir, &node.spec.source);
            let frame = (self.frames)
                .entry(node.id)
                .or_insert_with(|| WebFrame::new(node.spec.clone(), has_page));
            if frame.spec.source != node.spec.source {
                let old = std::mem::replace(frame, WebFrame::new(node.spec.clone(), has_page));
                self.stale.extend(old.texture.map(|texture| texture.image));
            } else if frame.spec.viewport != node.spec.viewport {
                frame.spec = node.spec.clone();
                resized.push(node.id);
            }
        }
        for id in resized {
            self.reload(id);
        }
    }

    /// Loads frame `id`'s page again, which then settles and is snapshotted: now when its view is
    /// idle (back at its place off the canvas, at its viewport's size, unless it is live), once
    /// it is when it is busy, and with a view built anew when it has none.
    pub(super) fn reload(&mut self, id: figcraft_doc::NodeId) {
        let live = self.live.as_ref().is_some_and(|live| live.id == id);
        let Some(frame) = self.frames.get(&id) else {
            return;
        };
        let parking = self.parking(frame.spec.viewport);
        let has_page = has_page(&self.dir, &frame.spec.source);
        let host = self.host.as_ref();
        let Some(frame) = self.frames.get_mut(&id) else {
            return;
        };
        frame.has_page = has_page;
        match (&frame.engine, frame.phase) {
            (Some(engine), Phase::Idle) => {
                if let Some(host) = host.filter(|_| !live) {
                    engine.place_in(host, parking);
                }
                engine.reload();
                frame.phase = Phase::Loading;
                frame.built = Some(Instant::now());
            }
            (Some(_), _) => frame.reload_after = true,
            (None, _) => {
                frame.phase = Phase::Cold;
                frame.refresh = true;
            }
        }
        self.note(json!({"event": "reload", "id": id.0, "live": live}));
    }

    /// Builds views for the frames that need one, nearest the view's centre first, while fewer
    /// than [`Config::pool`] are loading, settling or snapshotting.
    pub(super) fn build_next(&mut self, nodes: &[WebNode], view: &ViewState, window: &Window) {
        let busy = (self.frames.values())
            .filter(|frame| {
                matches!(
                    frame.phase,
                    Phase::Loading | Phase::Settling | Phase::Snapping
                )
            })
            .count();
        let room = self.config.pool.saturating_sub(busy);
        let mut wanted: Vec<(f64, figcraft_doc::NodeId)> = nodes
            .iter()
            .filter(|node| {
                self.frames.get(&node.id).is_some_and(|frame| {
                    frame.phase == Phase::Cold
                        && frame.has_page
                        && (frame.texture.is_none()
                            || frame.refresh
                            || self.want_live == Some(node.id))
                })
            })
            .map(|node| {
                let (x, y) = (
                    (node.rect[0] + node.rect[2]) / 2.0,
                    (node.rect[1] + node.rect[3]) / 2.0,
                );
                ((x - view.cx).hypot(y - view.cy), node.id)
            })
            .collect();
        wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, id) in wanted.into_iter().take(room) {
            self.build(id, window);
        }
    }

    fn build(&mut self, id: figcraft_doc::NodeId, window: &Window) {
        let to_view = self.to_view.clone();
        let Some(frame) = self.frames.get(&id) else {
            return;
        };
        let parking = self.parking(frame.spec.viewport);
        let (url, screen) = match &frame.spec.source {
            Source::Url(url) => (url.clone(), None),
            Source::Screen(name) => {
                let folder = self.folder(name);
                let screen = ScreenSource {
                    name: name.clone(),
                    folder,
                };
                (screens::url(name), Some(screen))
            }
        };
        let spec = EngineSpec {
            url,
            init_script: INIT.into(),
            data_store: None,
            visible: false,
            frame: parking,
            screen,
        };
        let started = Instant::now();
        let built = Engine::build(
            spec,
            move |event| {
                let _ = to_view.unbounded_send(WebMsg::Engine(id, event));
            },
            window,
        );
        let build_ms = ms(started.elapsed());
        let entry = match &built {
            Ok(_) => json!({"event": "built", "id": id.0, "build_ms": build_ms}),
            Err(error) => json!({"event": "build_failed", "id": id.0, "error": error}),
        };
        let host = self.host.as_ref();
        if let Some(frame) = self.frames.get_mut(&id) {
            frame.refresh = false;
            match built {
                Ok(engine) => {
                    if let Some(host) = host {
                        engine.adopt_into(host);
                        engine.place_in(host, parking);
                    }
                    frame.engine = Some(engine);
                    frame.phase = Phase::Loading;
                    frame.built = Some(started);
                }
                // Not tried again.
                Err(_) => frame.phase = Phase::Idle,
            }
        }
        self.note(entry);
    }

    /// Lets go of the views least recently used beyond the pool: idle ones, not live.
    pub(super) fn trim_pool(&mut self) {
        let keep = |web: &WebFrames, id| {
            web.live.as_ref().is_some_and(|live| live.id == id)
                || web.leaving.as_ref().is_some_and(|live| live.id == id)
                || web.want_live == Some(id)
        };
        loop {
            let alive = self
                .frames
                .values()
                .filter(|frame| frame.engine.is_some())
                .count();
            if alive <= self.config.pool {
                return;
            }
            let oldest = (self.frames.iter())
                .filter(|(id, frame)| {
                    frame.engine.is_some() && frame.phase == Phase::Idle && !keep(self, **id)
                })
                .min_by_key(|(_, frame)| frame.used)
                .map(|(id, _)| *id);
            let Some(id) = oldest else {
                return;
            };
            if let Some(frame) = self.frames.get_mut(&id) {
                frame.engine = None;
                frame.phase = Phase::Cold;
            }
            self.note(json!({"event": "pool_drop", "id": id.0}));
        }
    }

    /// The frames on screen whose texture is far from the density the settled zoom wants, and
    /// whose view is still there: snapshotted again. One whose view went is rebuilt.
    pub(super) fn recapture(
        &mut self,
        nodes: &[WebNode],
        view: &ViewState,
        bounds: Bounds<Pixels>,
    ) -> Vec<figcraft_doc::NodeId> {
        if self.zooming || self.config.scale.is_some() || self.bench.is_some() {
            return Vec::new();
        }
        let wanted = self.snapshot_scale();
        let mut again = Vec::new();
        for node in nodes {
            let shown = screen_bounds(node.rect, view, bounds.origin).intersects(&bounds);
            let Some(frame) = self.frames.get_mut(&node.id) else {
                continue;
            };
            let Some(texture) = &frame.texture else {
                continue;
            };
            let off = texture.scale < wanted / 1.5 || texture.scale > wanted * 2.5;
            if !shown || !off || self.live.as_ref().is_some_and(|live| live.id == node.id) {
                continue;
            }
            match (&frame.engine, frame.phase) {
                (Some(_), Phase::Idle) => again.push(node.id),
                (None, Phase::Cold) => frame.refresh = true,
                _ => {}
            }
        }
        again
    }

    /// The textures to paint, in the window, clipped to their frames and corners.
    pub(super) fn painted(
        &mut self,
        nodes: &[WebNode],
        view: &ViewState,
        bounds: Bounds<Pixels>,
    ) -> Painted {
        let textures = self.bench.as_ref().is_none_or(|bench| bench.textures);
        let items = nodes
            .iter()
            .filter(|_| textures)
            .filter_map(|node| {
                let texture = self.frames.get(&node.id)?.texture.as_ref()?;
                let at = screen_bounds(node.rect, view, bounds.origin);
                at.intersects(&bounds).then(|| Item {
                    image: texture.image.clone(),
                    at,
                    radii: corners(node.radii, view.zoom),
                })
            })
            .collect();
        Painted {
            items,
            stale: std::mem::take(&mut self.stale),
            marker: self.marker(),
            times: self.bench.is_some().then(|| self.paint_times.clone()),
        }
    }
}

/// `bounds` as a frame in the window.
pub(super) fn frame_of(bounds: Bounds<Pixels>) -> ViewFrame {
    ViewFrame {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}

/// Whether `source` has a page to show: a screen once its folder has an `index.html`.
pub(super) fn has_page(dir: &std::path::Path, source: &Source) -> bool {
    match source {
        Source::Url(_) => true,
        Source::Screen(name) => screens::folder(dir, name).join(screens::INDEX).is_file(),
    }
}
