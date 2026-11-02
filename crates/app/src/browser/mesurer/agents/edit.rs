//! Sawit's changes to mesurer's workspace: an agent's `open`, `close`, `set` and `clear`, a comment
//! row removed in the panel (and put back by Undo), the threads `browser_annotations` read with
//! `clear`. Each is made to the workspace held, written to Sawit's files, and handed to mesurer
//! with the settings (`SawitMesurer.apply`), which shows it as it shows what it loads; mesurer
//! saves nothing back for it. (mesurer's own save, 250 ms after its last change, can land after
//! and undo the quarter second before: `page/mesurer/store.js`.)

use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::annotations::Thread;
use sawit_core::annotations::mesurer::edit::{self as change, CLEAR_WHAT};
use sawit_core::browser::mesurer::{Summary, calls, chrome_of, origin_key};
use serde_json::{Map, Value, json};

use super::super::super::annotate::ThreadChange;
use super::super::super::shots::within;
use super::super::super::{BrowserView, OpResult, Panel};
use super::super::CALL_TIMEOUT;
use super::{TAKEN_KEPT, here};

/// How often `open` looks whether mesurer is on the page yet.
const MOUNT_POLL: Duration = Duration::from_millis(50);

/// What `set` and `clear` say when there is no workspace to change.
const NOT_YET: &str = "mesurer hasn't been on this page yet: open it first (browser_mesurer open, \
     or Annotate in the Browser tab's toolbar).";

impl BrowserView {
    /// Writes `envelope` as `origin`'s workspace and, while mesurer is on a page of that origin,
    /// hands it to mesurer; the panel's rows and summary follow.
    fn mesurer_push(&mut self, origin: &str, envelope: Value, cx: &mut Context<Self>) {
        let project = self.project.clone();
        (self.mesurer.store).save_workspace(project.as_deref(), origin, Some(envelope.clone()), cx);
        let args = json!({
            "snapshot": { "settings": self.mesurer.store.settings(), "workspace": envelope },
            "source": { "workspace": true },
        });
        let here = origin == origin_key(&self.current_url());
        if here && self.mesurer.mounted {
            let applied = self.world_json(calls::APPLY, args.to_string());
            cx.spawn(async move |_, cx| {
                let timer = cx.background_executor().timer(CALL_TIMEOUT);
                if let Some(Ok(Err(e))) = within(applied, timer).await {
                    eprintln!("sawit: mesurer couldn't take Sawit's change: {e}");
                }
            })
            .detach();
        }
        self.mesurer.agents.last_apply = Some(args);
        if here {
            self.mesurer_summary_changed(cx);
        }
    }

    /// Changes the workspace of the page on show with `edit` (given the workspace and the page's
    /// key), then pushes it ([`Self::mesurer_push`]). Refused while no workspace of the page's
    /// origin is held: no mount has read it.
    fn mesurer_edit<T>(
        &mut self,
        edit: impl FnOnce(&mut Value, &str) -> Result<T, String>,
        cx: &mut Context<Self>,
    ) -> Result<T, String> {
        let (_, origin, page) = here(self);
        if !self.mesurer.store.holds(&origin) {
            return Err(NOT_YET.into());
        }
        let mut envelope =
            (self.mesurer.store.workspace(&origin).cloned()).unwrap_or_else(change::new_envelope);
        let out = edit(&mut envelope, &page)?;
        self.mesurer_push(&origin, envelope, cx);
        Ok(out)
    }

    /// Comment rows removed in the panel, or put back by Undo: their threads go from mesurer's
    /// workspace, or come back where they were. A thread of an origin not held now (the page
    /// moved to another site since) is left as it is.
    pub(in crate::browser) fn mesurer_threads_changed(
        &mut self,
        changes: Vec<ThreadChange>,
        cx: &mut Context<Self>,
    ) {
        for change in changes {
            match change {
                ThreadChange::Removed(thread) => {
                    let Some(mut envelope) = self.held(&thread) else {
                        continue;
                    };
                    let Some(taken) = change::take_thread(&mut envelope, &thread.page, &thread.id)
                    else {
                        continue;
                    };
                    let state = &mut self.mesurer.agents;
                    if state.taken.len() >= TAKEN_KEPT
                        && let Some(oldest) = state.taken.keys().next().cloned()
                    {
                        state.taken.remove(&oldest);
                    }
                    let at = (thread.origin.clone(), thread.page.clone(), taken);
                    state.taken.insert(thread.id.clone(), at);
                    self.mesurer_push(&thread.origin, envelope, cx);
                }
                ThreadChange::Restored(thread) => {
                    let Some((origin, page, taken)) = self.mesurer.agents.taken.remove(&thread.id)
                    else {
                        continue;
                    };
                    let Some(mut envelope) = self.held(&thread) else {
                        continue;
                    };
                    change::put_thread(&mut envelope, &page, taken);
                    self.mesurer_push(&origin, envelope, cx);
                }
            }
        }
    }

    /// The workspace `thread` is in, as held; `None` (and said why) while another origin's is.
    fn held(&self, thread: &Thread) -> Option<Value> {
        if !self.mesurer.store.holds(&thread.origin) {
            eprintln!(
                "sawit: comment {} is on {}, whose workspace isn't open; mesurer keeps it",
                thread.id, thread.origin
            );
            return None;
        }
        Some(
            (self.mesurer.store.workspace(&thread.origin).cloned())
                .unwrap_or_else(change::new_envelope),
        )
    }

    /// Deletes `threads` from mesurer for good (an agent read them with `clear`); answers how
    /// many went.
    pub(in crate::browser) fn mesurer_delete_threads(
        &mut self,
        threads: &[Thread],
        cx: &mut Context<Self>,
    ) -> usize {
        let mut deleted = 0;
        let mut origins: Vec<&str> = Vec::new();
        for thread in threads {
            if !origins.contains(&thread.origin.as_str()) {
                origins.push(&thread.origin);
            }
        }
        for origin in origins {
            let Some(mut envelope) = threads
                .iter()
                .find(|t| t.origin == origin)
                .and_then(|t| self.held(t))
            else {
                continue;
            };
            let before = deleted;
            for thread in threads.iter().filter(|t| t.origin == origin) {
                if change::take_thread(&mut envelope, &thread.page, &thread.id).is_some() {
                    deleted += 1;
                }
            }
            if deleted > before {
                self.mesurer_push(origin, envelope, cx);
            }
        }
        deleted
    }
}

/// `open`: Annotate opens, which puts mesurer on the page; answers `state` once it is there, or
/// after [`CALL_TIMEOUT`] (a page still loading mounts it when it has loaded).
pub(super) fn open(
    view: &mut BrowserView,
    cx: &mut Context<BrowserView>,
) -> oneshot::Receiver<OpResult> {
    let (tx, rx) = oneshot::channel();
    if let Err(e) = view.panel_op(Some("annotate"), None, cx) {
        let _ = tx.send(Err(e));
        return rx;
    }
    cx.spawn(async move |this, cx| {
        let mut waited = Duration::ZERO;
        loop {
            let ready = this.update(cx, |this, _| this.mesurer.mounted || this.engine.is_none());
            match ready {
                Ok(true) => break,
                Ok(false) if waited < CALL_TIMEOUT => {}
                Ok(false) => break,
                Err(_) => return,
            }
            cx.background_executor().timer(MOUNT_POLL).await;
            waited += MOUNT_POLL;
        }
        let state = this.update(cx, |this, cx| this.mesurer_apply("state", &Map::new(), cx));
        let answer = match state {
            Ok(state) => state.await.unwrap_or_else(|_| Err("No answer.".into())),
            Err(_) => Err("The browser was closed.".into()),
        };
        let _ = tx.send(answer);
    })
    .detach();
    rx
}

/// `close`: Annotate closes, which takes mesurer off the page (it saves first).
pub(super) fn close(view: &mut BrowserView, cx: &mut Context<BrowserView>) -> Value {
    if view.panel == Some(Panel::Annotate) {
        view.toggle_panel(Panel::Annotate, cx);
    }
    json!({ "mounted": false, "panel": view.panel.map(Panel::as_str) })
}

/// What `set` and `clear` answer besides what they did: whether mesurer took it now, and the
/// page as it is.
fn after(view: &BrowserView) -> Value {
    let (_, origin, page) = here(view);
    let envelope = view.mesurer.store.workspace(&origin);
    json!({
        "applied": view.mesurer.mounted,
        "page_key": page,
        "chrome": envelope.map_or(Value::Null, chrome_of),
        "summary": envelope.map(|e| Summary::of(e, &page)).unwrap_or_default(),
    })
}

/// `set {xray?, rulers?, tool?, minimized?, guides?, layout_guides?}`. Layout guides set while
/// mesurer is on the page are shown too (`layout.show`): mesurer hides them until asked.
pub(super) fn set(
    view: &mut BrowserView,
    args: &Map<String, Value>,
    cx: &mut Context<BrowserView>,
) -> oneshot::Receiver<OpResult> {
    let (tx, rx) = oneshot::channel();
    let answer = view
        .mesurer_edit(|envelope, page| change::set(envelope, page, args), cx)
        .map(|set| {
            let mut answer = after(view);
            answer["set"] = json!(set);
            answer
        });
    let layout = args
        .get("layout_guides")
        .and_then(Value::as_array)
        .is_some_and(|l| !l.is_empty());
    let mut answer = match answer {
        Ok(answer) if layout && view.mesurer.mounted => answer,
        answer => {
            let _ = tx.send(answer);
            return rx;
        }
    };
    let call = json!({ "name": "layout.show", "args": {} }).to_string();
    let shown = view.world_json(calls::CALL, call);
    cx.spawn(async move |_, cx| {
        let timer = cx.background_executor().timer(CALL_TIMEOUT);
        answer["layout_guides_shown"] = match within(shown, timer).await {
            Some(Ok(Ok(shown))) => shown,
            Some(Ok(Err(e))) => json!({ "shown": false, "reason": e }),
            _ => json!({ "shown": false, "reason": "the page didn't answer" }),
        };
        let _ = tx.send(Ok(answer));
    })
    .detach();
    rx
}

/// `clear {what}`: the page's drawings, measurements, guides, layout guides, comments or all.
pub(super) fn clear(
    view: &mut BrowserView,
    args: &Map<String, Value>,
    cx: &mut Context<BrowserView>,
) -> OpResult {
    let what = args
        .get("what")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("clear needs what: {}.", CLEAR_WHAT.join(", ")))?;
    if let Some(other) = args.keys().find(|k| *k != "what") {
        return Err(format!("clear takes what, not “{other}”."));
    }
    let cleared = view.mesurer_edit(|envelope, page| change::clear(envelope, page, what), cx)?;
    let mut answer = after(view);
    answer["cleared"] = json!({
        "drawings": cleared.drawings,
        "measurements": cleared.measurements,
        "guides": cleared.guides,
        "layout_guides": cleared.layout_guides,
        "comments": cleared.comments.len(),
    });
    Ok(answer)
}
