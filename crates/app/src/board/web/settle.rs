//! How a frame's page settles before its snapshot: once it has loaded it is shown off the canvas
//! (outside the host's clip, so its animations and frames run), and asked to say when it is still:
//! its fonts are ready, its animations that end have ended, and two frames have been drawn
//! ([`INIT`]'s `__sawitSettle`). A page that hasn't said so within [`super::Config::settle_ms`] is
//! snapshotted anyway.

use std::time::{Duration, Instant};

use gpui_kit::*;
use serde_json::json;

use super::{BoardView, Phase, Settle, WebMsg, ms};

/// Injected into every web frame's page, before its own scripts:
/// - Escape in the page leaves live mode and Preview;
/// - `__sawitSettle(token)` waits for the page's fonts and its images (decoded), then for its
///   animations and transitions that end (an endless spinner doesn't count) for up to 1.2 s; what
///   still runs is then finished, so the picture shows the page as it ends, not halfway through a
///   staggered entrance (B17); then it waits two animation frames, finishes what those set off,
///   waits two more and posts `sawit-web:settled:<token>`;
/// - a change the user makes in a live page (typing, a click, a scroll, a change of its DOM) posts
///   `sawit-web:changed` once things are quiet for 250 ms, so its picture stays up to date;
/// - a change of its DOM at rest (a hot update), or a dev server's update or reload message on its
///   hot-update socket, posts `sawit-web:dirty` once things are quiet for 300 ms (1.5 s at most
///   after the first change). What the canvas itself sets off is let be: the DOM changing while it
///   settles, and for a moment after it is shown or hidden.
pub(in crate::board) const INIT: &str = r#"(() => {
  const post = (m) => { if (window.ipc) window.ipc.postMessage(m); };
  addEventListener('keydown', e => { if (e.key === 'Escape') post('sawit-web:escape'); }, true);
  let hush = 0;
  const hushFor = (ms) => { hush = Math.max(hush, Date.now() + ms); };
  addEventListener('visibilitychange', () => hushFor(600), true);
  const within = (p, ms) => Promise.race([p, new Promise(r => setTimeout(r, ms))]);
  const frames = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
  const ending = () => (document.getAnimations ? document.getAnimations() : []).filter(a => {
    const t = a.effect && a.effect.getComputedTiming ? a.effect.getComputedTiming() : null;
    return t && Number.isFinite(t.endTime) && a.playState !== 'finished' && a.playState !== 'idle';
  });
  const finish = () => { for (const a of ending()) { try { a.finish(); } catch (_) {} } };
  window.__sawitSettle = async (token) => {
    hushFor(60000);
    try { await within(document.fonts.ready, 3000); } catch (_) {}
    await within(Promise.all([...document.images].map(i => i.decode ? i.decode().catch(() => {}) : null)), 2500);
    await within(Promise.all(ending().map(a => a.finished.catch(() => {}))), 1200);
    finish();
    await frames();
    finish();
    await frames();
    post('sawit-web:settled:' + token);
    hush = Date.now() + 400;
  };
  let quiet = 0;
  const changed = () => { clearTimeout(quiet); quiet = setTimeout(() => post('sawit-web:changed'), 250); };
  for (const type of ['input', 'change', 'click', 'keyup', 'scroll', 'transitionend', 'animationend'])
    addEventListener(type, changed, true);
  let dirtyAt = 0, first = 0;
  const dirty = () => {
    const now = Date.now();
    if (now < hush) return;
    if (!first) first = now;
    clearTimeout(dirtyAt);
    dirtyAt = setTimeout(() => { first = 0; if (Date.now() >= hush) post('sawit-web:dirty'); },
      Math.max(0, Math.min(300, first + 1500 - now)));
  };
  new MutationObserver(() => { changed(); dirty(); }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  const Socket = window.WebSocket;
  if (Socket) window.WebSocket = class extends Socket {
    constructor(url, protocols) {
      super(url, protocols);
      if (/hmr|vite|webpack|turbopack|_next|reload/i.test(String(url) + ' ' + String(protocols || '')))
        this.addEventListener('message', e => {
          if (typeof e.data === 'string' && /update|reload|built|sync/i.test(e.data)) setTimeout(dirty, 150);
        });
    }
  };
})();"#;
/// Read before each settled snapshot: what ran in the page (the test pages' `__probe`), and how
/// tall the page is (`pageHeight`) in its viewport (`innerHeight`), which a full-page route frame
/// grows to ([`super::fit`]), and how far down the viewport its text, pictures and fields go
/// (`contentBottom`, whatever their opacity), which tells a half-drawn snapshot ([`super::drawn`]).
pub(super) const PROBE: &str = "JSON.stringify(Object.assign({visibility: document.visibilityState, \
                     innerWidth: innerWidth, innerHeight: innerHeight, dpr: devicePixelRatio, \
                     pageHeight: Math.max(document.documentElement.scrollHeight, \
                     document.body ? document.body.scrollHeight : 0), \
                     contentBottom: (() => { let b = 0; \
                       const all = document.body ? document.body.querySelectorAll('*') : []; \
                       for (let i = 0; i < all.length && i < 4000; i++) { const el = all[i]; \
                         const media = /^(img|svg|video|canvas|picture|input|textarea|select)$/i.test(el.tagName); \
                         const own = !media && [...el.childNodes].some(c => c.nodeType === 3 && c.textContent.trim()); \
                         if (!media && !own) continue; \
                         const r = el.getBoundingClientRect(); if (r.width < 2 || r.height < 2) continue; \
                         const cs = getComputedStyle(el); if (cs.display === 'none' || cs.visibility === 'hidden') continue; \
                         b = Math.max(b, r.bottom); } \
                       return Math.round(Math.min(b, innerHeight)); })()}, window.__probe || {}))";

impl BoardView {
    /// A frame's page has loaded (or loaded again): it settles, shown off the canvas unless
    /// [`Settle::Hidden`], then is snapshotted ([`Self::web_settled`]).
    pub(super) fn web_loaded(&mut self, id: figcraft_doc::NodeId, cx: &mut Context<Self>) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let live = web.live.as_ref().is_some_and(|live| live.id == id);
        let config = web.config;
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        let Some(engine) = &frame.engine else {
            return;
        };
        let now = Instant::now();
        frame.loaded = Some(now);
        frame.pid = engine.web_process();
        // A load Sawit asked for (a view built, a reload) finds the frame loading; any other is the
        // page's own doing.
        let asked = frame.phase == Phase::Loading;
        let load_ms = frame.built.map(|built| ms(now - built));
        let entry = json!({"event": "loaded", "id": id.0, "source": format!("{:?}", frame.spec.source),
                           "pid": frame.pid, "build_to_load_ms": load_ms, "live": live, "asked": asked});
        web.note(entry);
        if live || (!asked && !self.web_reloaded_itself(id, cx)) {
            return;
        }
        let Some(web) = self.web.as_mut() else {
            return;
        };
        web.measure_first(id);
        let Some(frame) = web.frames.get_mut(&id) else {
            return;
        };
        let Some(engine) = &frame.engine else {
            return;
        };
        frame.phase = Phase::Settling;
        frame.settle = frame.settle.wrapping_add(1);
        frame.retakes = 0;
        let token = frame.settle;
        if config.settle != Settle::Hidden {
            engine.set_visible(true);
        }
        engine.eval(&format!(
            "window.__sawitSettle && window.__sawitSettle({token})"
        ));
        self.web_settle_later(id, token, config.settle_ms, cx);
    }

    /// Snapshots frame `id` after `wait` ms if its page hasn't said by then that settle `token`
    /// is over: a page whose script never answers is still pictured.
    pub(super) fn web_settle_later(
        &mut self,
        id: figcraft_doc::NodeId,
        token: u32,
        wait: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(wait))
                .await;
            let _ = this.update(cx, |this, cx| this.web_settled(id, token, "timeout", cx));
        })
        .detach();
    }

    /// Frame `id`'s page settled (settle `token`; `why` says how we know): its probe is read, then
    /// it is snapshotted. An answer to an older settle, or for a page no longer settling, is let
    /// be.
    pub(super) fn web_settled(
        &mut self,
        id: figcraft_doc::NodeId,
        token: u32,
        why: &'static str,
        cx: &mut Context<Self>,
    ) {
        let Some(web) = self.web.as_mut() else {
            return;
        };
        let to_view = web.to_view.clone();
        let Some(frame) = web.frames.get(&id) else {
            return;
        };
        // The page's answer came after the settle timed out and its snapshot was taken: the page
        // has finished now, so it is pictured again (B17: a picture taken at the timeout may be
        // half-drawn).
        let late = why == "settled"
            && frame.engine.is_some()
            && frame.phase == Phase::Idle
            && frame.settle == token
            && frame.timed_out;
        if late {
            if let Some(frame) = web.frames.get_mut(&id) {
                frame.timed_out = false;
            }
            web.note(json!({"event": "settled_late", "id": id.0}));
            self.web_snapshot(id, "late", cx);
            return;
        }
        if frame.engine.is_none() || frame.phase != Phase::Settling || frame.settle != token {
            return;
        }
        let measuring = frame.measuring;
        let waited = frame.loaded.map(|at| ms(at.elapsed()));
        let screen = match &frame.spec.source {
            super::Source::Screen(name) => Some(name.clone()),
            _ => None,
        };
        web.note(
            json!({"event": "settled", "id": id.0, "how": why, "after_load_ms": waited,
                        "measuring": measuring}),
        );
        if let Some(frame) = web.frames.get_mut(&id) {
            frame.timed_out = why == "timeout";
        }
        if measuring {
            // A full-page frame's page at its viewport's height: measured, then it takes the
            // page's height and settles again there before its snapshot ([`super::fit`]).
            if let Some(engine) = web.frames.get(&id).and_then(|f| f.engine.as_ref()) {
                let _ = engine.eval_with_callback(PROBE, move |json| {
                    let _ = to_view.unbounded_send(WebMsg::Measured(id, token, json));
                });
            }
            return;
        }
        if let Some(engine) = web.frames.get(&id).and_then(|f| f.engine.as_ref()) {
            let probe = to_view.clone();
            let _ = engine.eval_with_callback(PROBE, move |json| {
                let _ = probe.unbounded_send(WebMsg::Probe(id, json));
            });
            // A screen an agent just wrote: its blueprint, for the build to play (30e).
            let now = cx.background_executor().now();
            if screen.is_some_and(|name| self.agents.screen_writer(&name, now).is_some()) {
                let script = crate::board::presence::replay::blueprint::SCRIPT;
                let _ = engine.eval_with_callback(script, move |json| {
                    let _ = to_view.unbounded_send(WebMsg::Blueprint(id, json));
                });
            }
        }
        self.web_snapshot(id, why, cx);
    }
}
