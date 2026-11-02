//! The web frames' views at work: each frame, the bookkeeping ([`super::pool`]) and what to
//! paint; then, as their pages load, settling off the canvas, snapshots turned into textures off
//! the main thread, and the view hidden again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::NodeId;

use futures::channel::oneshot;
use gpui_kit::*;
use serde_json::{Value, json};

use super::bench::Painted;
use super::pool::frame_of;
use super::{BoardView, Phase, Settle, Texture, WebFrame, WebMsg, ms, web_frame, web_nodes};
use crate::browser::engine::EngineEvent;
use crate::browser::engine::web_frame::Host;

/// The band at the canvas's bottom that the toolbar floats in, with a little room above it.
const TOOLBAR_BAND: f32 = crate::board::toolbar::BOTTOM + crate::board::toolbar::HEIGHT + 8.0;

impl WebFrame {
    pub(super) fn new(spec: super::Spec, has_page: bool) -> WebFrame {
        WebFrame {
            spec,
            has_page,
            settle: 0,
            reload_after: false,
            engine: None,
            phase: Phase::Cold,
            texture: None,
            used: Instant::now(),
            built: None,
            loaded: None,
            repeat: 0,
            pid: 0,
            probe: None,
            refresh: false,
            storm: super::storm::Storm::default(),
            owed: false,
        }
    }
}

impl BoardView {
    /// Each frame, before the paint: keeps a frame for each web frame on the page, builds the
    /// next views the pool has room for, places the live view, and lists what to paint.
    pub(in crate::board) fn web_prepaint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Painted {
        let menus = self.menus_open();
        let preview = self.screen_ui.preview.is_some();
        let zooming =
            (self.zooming_until).is_some_and(|until| until > cx.background_executor().now());
        let Some(ds) = self.board().and_then(|board| board.session.ds().ok()) else {
            return Painted::default();
        };
        let (doc, page, view) = (ds.doc.clone(), ds.page, ds.view.clone());
        let Some(web) = self.web.as_mut() else {
            return Painted::default();
        };
        let nodes = web_nodes(&doc, page);
        web.canvas = bounds;
        web.window_width = f32::from(window.viewport_size().width);
        web.zoom = view.zoom;
        web.zooming = zooming;
        if web.host.is_none() {
            web.host = Host::new(window);
        }
        if let Some(host) = &web.host {
            // The toolbar floats over the canvas's bottom, under nothing: a live page stops above
            // it, unless Preview, which covers the toolbar, is open or the UI is hidden.
            let mut clip = frame_of(bounds);
            if !preview && !view.ui_hidden {
                clip.height = (clip.height - TOOLBAR_BAND).max(0.0);
            }
            host.place(clip);
            host.set_hidden(false);
        }
        web.sync(&nodes);
        web.keep_watching(&nodes);
        let servers = web.servers_due(Instant::now());
        web.build_next(&nodes, &view, window);
        let now = Instant::now();
        let placed = web.place_live(&nodes, &view, bounds, menus, now);
        if placed.again {
            window.request_animation_frame();
        }
        if let Some(preview) = &self.screen_ui.preview {
            // Its screen went (deleted, or undone): Preview ends with it.
            if nodes.iter().any(|node| node.id == preview.frame) {
                let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                self.place_preview(size);
            } else {
                self.close_preview(cx);
            }
        }
        let Some(web) = self.web.as_mut() else {
            return Painted::default();
        };
        let recapture = web.recapture(&nodes, &view, bounds);
        let painted = web.painted(&nodes, &view, bounds);
        for id in recapture {
            self.web_snapshot(id, "zoom", cx);
        }
        if let Some(id) = placed.snap_for_menu {
            self.web_snapshot(id, "menu", cx);
        }
        if let Some(wait) = placed.wake_in {
            self.web_wake_in(wait, cx);
        }
        self.web_check_servers(servers, cx);
        self.routes_rendered(cx);
        painted
    }

    /// Draws the canvas again in `wait`: the live view comes back once the canvas is still.
    fn web_wake_in(&mut self, wait: Duration, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// A message from a view's handlers, a page or the screens' watcher.
    pub(super) fn web_message(&mut self, message: WebMsg, cx: &mut Context<Self>) {
        match message {
            WebMsg::Engine(id, EngineEvent::Load { started: false, .. }) => self.web_loaded(id, cx),
            WebMsg::Engine(id, EngineEvent::Ipc(body)) => self.web_ipc(id, &body, cx),
            WebMsg::Probe(id, json) => {
                let Some(web) = self.web.as_mut() else {
                    return;
                };
                // WebKit hands back the string JSON.stringify made, itself as JSON.
                let text = serde_json::from_str::<String>(&json).unwrap_or(json);
                let probe = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
                web.note(json!({"event": "probe", "id": id.0, "probe": probe}));
                if let Some(frame) = web.frames.get_mut(&id) {
                    frame.probe = Some(probe);
                }
            }
            WebMsg::Changed(names) => self.web_files_changed(names, cx),
            WebMsg::Server(origin, up) => self.web_server(origin, up, cx),
            WebMsg::Preview(EngineEvent::Ipc(body)) if body == "sawit-web:escape" => {
                self.close_preview(cx);
            }
            WebMsg::Preview(_) => {}
            WebMsg::Engine(..) => {}
        }
    }

    /// What a page posted ([`super::settle::INIT`]): Escape, that it has settled, or that the
    /// user changed it while it is live.
    fn web_ipc(&mut self, id: NodeId, body: &str, cx: &mut Context<Self>) {
        if body == "sawit-web:escape" {
            self.web_exit(cx);
        } else if let Some(token) = body.strip_prefix("sawit-web:settled:") {
            let token = token.parse().unwrap_or(u32::MAX);
            self.web_settled(id, token, "settled", cx);
        } else if body == "sawit-web:dirty" {
            self.web_dirty(id, cx);
        } else if body == "sawit-web:changed" {
            let live = (self.web.as_ref())
                .and_then(|web| web.live.as_ref())
                .is_some_and(|live| live.id == id && !live.hidden);
            let idle = (self.web.as_ref())
                .and_then(|web| web.frames.get(&id))
                .is_some_and(|frame| frame.phase == Phase::Idle);
            if live && idle {
                self.web_snapshot(id, "changed", cx);
            }
        }
    }

    /// Snapshots frame `id`'s view: at its live size while it is live or leaving, else its
    /// viewport at the snapshot scale. The BGRA conversion runs off the main thread, then
    /// [`Self::web_texture`] takes the image.
    pub(super) fn web_snapshot(
        &mut self,
        id: figcraft_doc::NodeId,
        why: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let scale = web.snapshot_scale();
        let live_width = [web.live.as_ref(), web.leaving.as_ref()]
            .into_iter()
            .flatten()
            .find(|live| live.id == id)
            .and_then(|live| live.placed)
            .map(|(frame, _)| f64::from(frame.width));
        let space = web.space.clone();
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        let Some(engine) = &frame.engine else {
            return;
        };
        let width = live_width.unwrap_or(frame.spec.viewport.w * scale).max(1.0);
        let points = width / frame.spec.viewport.w;
        frame.phase = Phase::Snapping;
        let (reply, answer) = oneshot::channel();
        let asked = Instant::now();
        engine.snapshot_cg(width, reply);
        cx.spawn(async move |this, cx| {
            let (image, entry) = match answer.await {
                Ok(Ok(snap)) => {
                    let ((w, h), space_name) = (snap.size(), snap.color_space());
                    let (took, unwrap) = (ms(snap.took), ms(snap.unwrap));
                    let converted = cx
                        .background_spawn(async move {
                            let started = Instant::now();
                            let (w, h, bgra) = web_frame::to_bgra(&snap, space.as_deref()?)?;
                            let pixels = image::RgbaImage::from_raw(w, h, bgra)?;
                            let image = RenderImage::new([image::Frame::new(pixels)]);
                            Some((Arc::new(image), ms(started.elapsed())))
                        })
                        .await;
                    let convert = converted.as_ref().map(|(_, took)| *took);
                    let entry = json!({"event": "snapshot", "why": why, "id": id.0, "px": [w, h],
                        "points": width, "snapshot_ms": took, "cgimage_ms": unwrap,
                        "bgra_ms": convert, "total_ms": ms(asked.elapsed()),
                        "color_space": space_name});
                    (converted.map(|(image, _)| image), entry)
                }
                Ok(Err(error)) => (
                    None,
                    json!({"event": "snapshot_failed", "id": id.0, "error": error}),
                ),
                Err(_) => (
                    None,
                    json!({"event": "snapshot_failed", "id": id.0, "error": "dropped"}),
                ),
            };
            let _ = this.update(cx, |this, cx| {
                this.web_texture(id, image, points, entry, cx)
            });
        })
        .detach();
    }

    /// A snapshot of frame `id` is in: its texture replaces the old one, whose image leaves the
    /// atlas at the next paint. Then the view hides (off the canvas), unless it is live; a view
    /// leaving live mode goes back off the canvas now, the texture under it.
    fn web_texture(
        &mut self,
        id: figcraft_doc::NodeId,
        image: Option<Arc<RenderImage>>,
        points: f64,
        entry: Value,
        cx: &mut Context<Self>,
    ) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        web.note(entry);
        let leaving = web.leaving.as_ref().is_some_and(|live| live.id == id);
        let live = web.live.as_ref().is_some_and(|live| live.id == id);
        let parking = web
            .frames
            .get(&id)
            .map(|frame| web.parking(frame.spec.viewport));
        let (settle, wanted) = (web.config.settle, web.want_live == Some(id));
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        if let Some(image) = image
            && let Some(old) = frame.texture.replace(Texture {
                image,
                scale: points,
            })
        {
            web.stale.push(old.image);
        }
        frame.phase = Phase::Idle;
        frame.used = Instant::now();
        let reload = std::mem::take(&mut frame.reload_after);
        let again = frame.repeat > 0 && !reload;
        if again {
            frame.repeat -= 1;
        } else if let Some(engine) = &frame.engine {
            if leaving {
                engine.set_visible(false);
                engine.set_zoom(1.0);
                engine.set_magnification(1.0);
                engine.set_view_scale(1.0);
                if let (Some(host), Some(parking)) = (&web.host, parking) {
                    engine.place_in(host, parking);
                }
                web.leaving = None;
            } else if !live && settle != Settle::Hidden {
                engine.set_visible(false);
            }
        }
        if let Some(live) = web.live.as_mut().filter(|live| live.id == id) {
            // A menu waited for this picture: the live view now hides under it.
            live.menu_since = live
                .menu_since
                .map(|_| Instant::now() - super::live::MENU_WAIT);
        }
        // A refresh it was owed while it was busy (its page changed, or reloaded itself).
        let owed = !reload && !live && !again && frame.owed;
        if reload {
            web.reload(id);
        }
        if !again && !wanted {
            web.trim_pool();
        }
        cx.notify();
        if again {
            self.web_snapshot(id, "repeat", cx);
        } else if wanted {
            let _ = self.web_enter(id, cx);
        } else if owed {
            self.web_refresh(id, "owed", cx);
        }
    }

    /// The frames on the page on show that have a picture: their skeletons give way to it.
    pub(in crate::board) fn web_pictured(&self) -> std::collections::HashSet<NodeId> {
        (self.web.as_ref())
            .map(|web| {
                (web.frames.iter())
                    .filter(|(_, frame)| frame.texture.is_some())
                    .map(|(id, _)| *id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether a menu, a flyout or a picker is open over the canvas: the live view, which draws
    /// above GPUI, hides meanwhile.
    fn menus_open(&self) -> bool {
        self.menus.any_open()
            || self.open_flyout.is_some()
            || self.picker.is_some()
            || self.routes_ui.dialog.is_some()
    }
}
