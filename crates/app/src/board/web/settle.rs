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
/// - `__sawitSettle(token)` waits for the page's fonts, then for its animations that end (an
///   endless spinner doesn't count; at most 2 s), then for two animation frames, and posts
///   `sawit-web:settled:<token>`;
/// - a change the user makes in a live page (typing, a click, a scroll, a change of its DOM) posts
///   `sawit-web:changed` once things are quiet for 250 ms, so its picture stays up to date.
pub(in crate::board) const INIT: &str = r#"(() => {
  const post = (m) => { if (window.ipc) window.ipc.postMessage(m); };
  addEventListener('keydown', e => { if (e.key === 'Escape') post('sawit-web:escape'); }, true);
  window.__sawitSettle = async (token) => {
    try { await document.fonts.ready; } catch (_) {}
    const ending = (document.getAnimations ? document.getAnimations() : []).filter(a => {
      const t = a.effect && a.effect.getComputedTiming ? a.effect.getComputedTiming() : null;
      return t && Number.isFinite(t.endTime);
    });
    const ended = Promise.all(ending.map(a => a.finished.catch(() => {})));
    await Promise.race([ended, new Promise(r => setTimeout(r, 2000))]);
    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
    post('sawit-web:settled:' + token);
  };
  let quiet = 0;
  const changed = () => { clearTimeout(quiet); quiet = setTimeout(() => post('sawit-web:changed'), 250); };
  for (const type of ['input', 'change', 'click', 'keyup', 'scroll', 'transitionend', 'animationend'])
    addEventListener(type, changed, true);
  new MutationObserver(changed).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
})();"#;
/// Read before each settled snapshot: what ran in the page (the test pages' `__probe`).
pub(super) const PROBE: &str = "JSON.stringify(Object.assign({visibility: document.visibilityState, \
                     innerWidth: innerWidth, dpr: devicePixelRatio}, window.__probe || {}))";

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
        let load_ms = frame.built.map(|built| ms(now - built));
        let entry = json!({"event": "loaded", "id": id.0, "source": format!("{:?}", frame.spec.source),
                           "pid": frame.pid, "build_to_load_ms": load_ms, "live": live});
        if live {
            web.note(entry);
            return;
        }
        frame.phase = Phase::Settling;
        frame.settle = frame.settle.wrapping_add(1);
        let token = frame.settle;
        if config.settle != Settle::Hidden {
            engine.set_visible(true);
        }
        engine.eval(&format!(
            "window.__sawitSettle && window.__sawitSettle({token})"
        ));
        web.note(entry);
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
        if frame.engine.is_none() || frame.phase != Phase::Settling || frame.settle != token {
            return;
        }
        let waited = frame.loaded.map(|at| ms(at.elapsed()));
        web.note(json!({"event": "settled", "id": id.0, "how": why, "after_load_ms": waited}));
        if let Some(engine) = web.frames.get(&id).and_then(|f| f.engine.as_ref()) {
            let _ = engine.eval_with_callback(PROBE, move |json| {
                let _ = to_view.unbounded_send(WebMsg::Probe(id, json));
            });
        }
        self.web_snapshot(id, why, cx);
    }
}
