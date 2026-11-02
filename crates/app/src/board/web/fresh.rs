//! Route frames staying current. A dev server's page at rest, its view hidden off the canvas,
//! still runs: a hot update changes its DOM, and the page agent ([`super::INIT`]) says so once
//! things are quiet (`sawit-web:dirty`), or the page reloads itself. Either way that frame alone
//! settles again and is snapshotted ([`BoardView::web_resettle`]). Frames of the same server whose
//! views the pool let go of are built again too, since a hot update reaches every page.
//!
//! Each refresh the page starts counts towards its storm guard ([`super::storm`]): past it the
//! frame pauses, keeping its picture, and takes the refresh it missed once the pause is over. A
//! refresh while its server is stopped waits for the server ([`super::servers`]).

use std::time::{Duration, Instant};

use figcraft_doc::NodeId;
use gpui_kit::*;
use serde_json::json;

use super::servers::origin;
use super::{BoardView, Phase, Settle, Source};

impl BoardView {
    /// Frame `id`'s page says its DOM changed while at rest: it refreshes ([`Self::web_refresh`]).
    /// One that is being settled or snapshotted already takes the refresh once its picture is in.
    /// Screens follow their files instead ([`super::watch`]), and a live frame its own changes.
    pub(super) fn web_dirty(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let live = web.live.as_ref().is_some_and(|live| live.id == id);
        let Some(frame) = web.frames.get(&id) else {
            return;
        };
        let Source::Url(url) = &frame.spec.source else {
            return;
        };
        if live || frame.engine.is_none() {
            return;
        }
        let server = origin(url);
        // The server's other pages changed too: those whose views went are built again.
        for other in web.frames.values_mut() {
            let same = other.spec.source.url().is_some_and(|u| origin(u) == server);
            if same && other.engine.is_none() && other.texture.is_some() {
                other.refresh = true;
            }
        }
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        if frame.phase != Phase::Idle {
            frame.owed = true;
            return;
        }
        self.web_refresh(id, "dirty", cx);
    }

    /// Route frame `id`, at rest, settles again and is snapshotted: unless its server stopped or
    /// it is paused, when the refresh is owed, or this refresh is one too many, which pauses it.
    pub(super) fn web_refresh(&mut self, id: NodeId, why: &'static str, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let down = (web.frames.get(&id)).is_some_and(|f| web.server_down(&f.spec.source));
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        if down || frame.storm.paused(now) {
            frame.owed = true;
            return;
        }
        if !frame.storm.allow(now) {
            self.web_hold(id, why, cx);
            return;
        }
        frame.owed = false;
        self.web_resettle(id, why, cx);
    }

    /// Frame `id`'s page loaded by itself, not at Sawit's asking (a reload, a navigation): whether
    /// it settles now. Past the storm guard, or with its server stopped, it waits hidden instead;
    /// while its snapshot is being taken, it settles once that is in.
    pub(super) fn web_reloaded_itself(&mut self, id: NodeId, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let Some(web) = self.web.as_mut() else {
            return true;
        };
        let down = (web.frames.get(&id)).is_some_and(|f| web.server_down(&f.spec.source));
        let Some(frame) = web.frames.get_mut(&id) else {
            return true;
        };
        if !matches!(frame.spec.source, Source::Url(_)) {
            return true;
        }
        if frame.phase == Phase::Snapping {
            frame.owed = true;
            return false;
        }
        if down || frame.storm.paused(now) {
            frame.owed = true;
            frame.phase = Phase::Idle;
            return false;
        }
        if frame.storm.allow(now) {
            return true;
        }
        self.web_hold(id, "reload", cx);
        false
    }

    /// Frame `id` is paused: its picture stays, its view stays hidden, the refresh it missed is
    /// owed, and it resumes once the pause is over.
    fn web_hold(&mut self, id: NodeId, why: &str, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let settle = web.config.settle;
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        frame.owed = true;
        frame.phase = Phase::Idle;
        if let Some(engine) = frame.engine.as_ref().filter(|_| settle != Settle::Hidden) {
            engine.set_visible(false);
        }
        let left = frame.storm.left(now).unwrap_or(Duration::ZERO);
        web.note(json!({"event": "paused", "id": id.0, "why": why, "for_ms": left.as_millis()}));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(left).await;
            let _ = this.update(cx, |this, cx| this.web_resume(id, cx));
        })
        .detach();
    }

    /// Frame `id`'s pause is over: it takes the refresh it was owed, if its view is at rest.
    fn web_resume(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let down = (web.frames.get(&id)).is_some_and(|f| web.server_down(&f.spec.source));
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        if !frame.storm.resume(now) {
            return;
        }
        let ready = frame.owed && !down && frame.phase == Phase::Idle && frame.engine.is_some();
        if ready {
            frame.owed = false;
        }
        web.note(json!({"event": "resumed", "id": id.0, "refresh": ready}));
        cx.notify();
        if ready {
            self.web_resettle(id, "resume", cx);
        }
    }

    /// Frame `id`'s page settles again where it is and is snapshotted, as after a load: shown off
    /// the canvas so its animations run, unless [`Settle::Hidden`].
    pub(super) fn web_resettle(&mut self, id: NodeId, why: &'static str, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let config = web.config;
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        let Some(engine) = &frame.engine else {
            return;
        };
        frame.phase = Phase::Settling;
        frame.loaded = Some(Instant::now());
        frame.settle = frame.settle.wrapping_add(1);
        let token = frame.settle;
        if config.settle != Settle::Hidden {
            engine.set_visible(true);
        }
        engine.eval(&format!(
            "window.__sawitSettle && window.__sawitSettle({token})"
        ));
        web.note(json!({"event": "refresh", "id": id.0, "why": why}));
        self.web_settle_later(id, token, config.settle_ms, cx);
    }
}

impl BoardView {
    /// Reloads route frame `id` by hand (its bar's Reload): a pause ends, and its page loads,
    /// settles and is snapshotted again.
    pub(in crate::board) fn web_reload_frame(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        if let Some(frame) = web.frames.get_mut(&id) {
            frame.storm.clear();
            frame.owed = false;
        }
        web.reload(id);
        cx.notify();
    }
}
