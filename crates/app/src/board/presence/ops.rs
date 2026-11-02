//! Agents' ops on the canvas: each runs at once, or once its placeholder has been painted, and
//! shows its agent there; the canvas redraws on its own while an agent shows.

use std::sync::Arc;
use std::time::Instant;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_board::ops::{agent_refusal, work_of};
use sawit_core::agents::{self as model, AgentIdentity, GONE_AFTER};
use serde_json::{Value, json};

use super::target::{ghost_rect, target};
use super::{DIAGRAM_GAP, PLACEHOLDER_LEAD, TICK, Waiting};
use crate::agents::Look;
use crate::board::BoardView;
use crate::workspace::Answer;

impl BoardView {
    /// Runs agent `identity`'s op on the canvas, as [`Self::run_op`] runs it, and shows the agent
    /// there: its pointer at the op's target, the component it builds, what it does. A slow op, on
    /// a canvas on screen with agents' cursors shown, shows a placeholder where its result will
    /// land at once, and runs once that has been painted: its answer comes later. So does the
    /// answer of an op that comes while one waits, which runs after it.
    ///
    /// The view and the page on show are the user's: a command that moves them is refused
    /// (`sawit_board::ops::agent_refusal`). A `progress` changes no document: the canvas shows it.
    pub fn agent_op(
        &mut self,
        op: Value,
        identity: Option<&AgentIdentity>,
        cx: &mut Context<Self>,
    ) -> Answer {
        let look = crate::agents::meet(identity, cx);
        if let Some(refused) = agent_refusal(&op) {
            return Answer::Now(json!({"ok": false, "error": refused}));
        }
        if op.get("op").and_then(Value::as_str) == Some("progress") {
            return Answer::Now(self.agent_progress(&op, &look, cx));
        }
        let waits = !self.agents.queue.is_empty();
        let slow = self.shown && crate::agents::shown(cx) && model::slow_canvas_op(&op);
        if !waits && !slow {
            return Answer::Now(self.run_agent_op(&op, &look, None, cx));
        }
        let now = cx.background_executor().now();
        let ghost = slow.then(|| self.place_ghost(&op, &look, now)).flatten();
        let (reply, answer) = oneshot::channel();
        self.agents.queue.push_back(Waiting {
            op,
            look,
            ghost,
            reply,
        });
        self.drain_agent_ops_later(cx);
        self.after_agents_changed(cx);
        Answer::Later(answer)
    }

    /// An agent's diagram op, whose Pele render and conversion `prepare` runs off the UI thread:
    /// its placeholder shows at once (on a canvas on screen, with agents' cursors shown), and the
    /// diagram is drawn once it is ready, with the agent shown as for any op
    /// ([`Self::run_agent_op`]). Its answer comes then.
    pub fn agent_diagram_op(
        &mut self,
        op: Value,
        identity: Option<&AgentIdentity>,
        prepare: impl std::future::Future<Output = sawit_board::ops::PreparedDiagram> + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Answer {
        let look = crate::agents::meet(identity, cx);
        let now = cx.background_executor().now();
        let ghost = (self.shown && crate::agents::shown(cx))
            .then(|| self.place_ghost(&op, &look, now))
            .flatten();
        let ready = cx.background_executor().spawn(prepare);
        let (reply, answer) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            let prepared = ready.await;
            let drawn = this.update(cx, |this, cx| {
                this.run_agent_op_with(
                    &op,
                    &look,
                    ghost,
                    |this, cx| this.run_prepared_op(prepared, cx),
                    cx,
                )
            });
            let _ = reply.send(drawn.map_err(|_| "The canvas closed.".to_string()));
        })
        .detach();
        self.after_agents_changed(cx);
        Answer::Later(answer)
    }

    /// A placeholder for `look`'s slow op `op` on the page on show ([`ghost_rect`]).
    fn place_ghost(&mut self, op: &Value, look: &Look, now: Instant) -> Option<u64> {
        let ds = self.board()?.session.ds().ok()?;
        let (page, centre) = (ds.page, (ds.view.cx, ds.view.cy));
        // A diagram whose op names no place goes right of what the page holds.
        let corner = (op.get("op").and_then(Value::as_str) == Some("mermaid")).then(|| {
            let held = ds.doc.page_content_bounds(page);
            held.map_or((0.0, 0.0), |held| (held.x1 + DIAGRAM_GAP, held.y0))
        });
        let rect = ghost_rect(op, centre, corner);
        self.agents.saw(look, &model::canvas_action(op).doing, now);
        Some(self.agents.add_ghost(look, page, rect, now))
    }

    /// Runs `look`'s op `op`, and shows the agent where it landed; the placeholder `ghost`, if it
    /// has one, waits for the frame with the result, or shows its error mark.
    fn run_agent_op(
        &mut self,
        op: &Value,
        look: &Look,
        ghost: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Value {
        self.run_agent_op_with(op, look, ghost, |this, cx| this.run_op(op, cx), cx)
    }

    /// [`Self::run_agent_op`], with `run` running the op: [`Self::run_op`], or for a diagram
    /// rendered off the UI thread, [`BoardView::run_prepared_op`].
    ///
    /// A `begin` shows its new work; an op of a work rests the pointer on the work's corner and
    /// joins the work's undo step; any other op rests it on the corner of what it touched, which is
    /// outlined a moment, and is announced. The text the op wrote is revealed at reading speed.
    fn run_agent_op_with(
        &mut self,
        op: &Value,
        look: &Look,
        ghost: Option<u64>,
        run: impl FnOnce(&mut Self, &mut Context<Self>) -> Value,
        cx: &mut Context<Self>,
    ) -> Value {
        let action = model::canvas_action(op);
        let before = self.document();
        let undo = self.undo_state();
        let reply = run(self, cx);
        let after = self.document();
        let now = cx.background_executor().now();
        let ok = reply.get("ok") == Some(&Value::Bool(true));
        let doing = match ok {
            true => action.doing.clone(),
            false => format!("couldn't finish {}", action.doing),
        };
        self.agents.saw(look, &doing, now);
        let page = (self.board()).and_then(|b| b.session.ds().ok().map(|ds| ds.page));
        let changed = match (before, after) {
            (Some(before), Some(after)) if !Arc::ptr_eq(&before, &after) => Some((before, after)),
            _ => None,
        };
        let name = op.get("op").and_then(Value::as_str);
        let work = work_of(op).ok().flatten().map(str::to_string);
        if let Some((_, after)) = &changed {
            self.agents.keep_works_in(after);
        }
        if name == Some("begin") && ok {
            self.work_begun(op, &reply, look, now);
        } else if let Some(work) = work.filter(|_| ok) {
            self.work_op_ran(&work, look, undo, now);
        } else if let (Some((before, after)), Some(page)) = (&changed, page) {
            if let Some(target) = target(before, after, page) {
                self.agents.point_at(look, page, target, None, now);
            }
            let sentence = crate::agents::sentence(look, &action.done);
            self.agents.announcer.op(&look.id, sentence, now);
        }
        let reveal = self.shown && crate::agents::shown(cx) && !crate::agents::reduce_motion(cx);
        if let Some((before, after)) = changed.as_ref().filter(|_| reveal && reveals(name)) {
            self.agents.reveals.wrote(before, after, now);
        }
        if let Some(ghost) = ghost {
            match changed.filter(|_| ok) {
                Some((_, after)) => self.agents.land(ghost, after, now),
                None => self.agents.fail(ghost, !ok, now),
            }
        }
        self.after_agents_changed(cx);
        reply
    }

    /// Runs the waiting ops once [`PLACEHOLDER_LEAD`] is over, so the placeholder is on screen
    /// before a slow op holds the UI thread.
    fn drain_agent_ops_later(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.agents.draining, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PLACEHOLDER_LEAD).await;
            let _ = this.update(cx, |this, cx| this.drain_agent_ops(cx));
        })
        .detach();
    }

    /// Runs the waiting ops in the order they came, and answers each.
    fn drain_agent_ops(&mut self, cx: &mut Context<Self>) {
        self.agents.draining = false;
        while let Some(waiting) = self.agents.queue.pop_front() {
            let reply = self.run_agent_op(&waiting.op, &waiting.look, waiting.ghost, cx);
            let _ = waiting.reply.send(Ok(reply));
        }
    }

    /// Redraws the canvas every [`TICK`] while an agent shows on it, so its pointer fades and goes
    /// on time, works stall, announcements move on, and its tab hears when the work stops.
    pub(super) fn keep_agents_ticking(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.agents.ticking, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let mut every = TICK;
            loop {
                cx.background_executor().timer(every).await;
                let going = this.update(cx, |this, cx| {
                    let now = cx.background_executor().now();
                    // A work whose frame the user deleted, or undid, is over.
                    if let Some(doc) = this.document() {
                        this.agents.keep_works_in(&doc);
                    }
                    let any = this.agents.prune(now);
                    this.agents.ticking = any;
                    every = this.agents.tick_every();
                    let at_work = this.agents.at_work(now).is_some();
                    if at_work != this.agents.told_at_work {
                        this.agents.told_at_work = at_work;
                        cx.emit(super::AgentActivity);
                    }
                    cx.notify();
                    any
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The render thread handed over the frame of request `key`: placeholders whose result it
    /// shows go.
    pub(in crate::board) fn agents_rendered(&mut self, key: u64) {
        if let Some((asked, doc)) = &self.requested
            && *asked == key
        {
            self.agents.rendered(doc);
        }
    }

    /// The agents at work on this canvas, as `app_state.agents` lists them, with the works they
    /// build.
    pub fn describe_agents(&self, cx: &App) -> Vec<Value> {
        let now = cx.background_executor().now();
        (self.agents.agents.iter())
            .filter(|a| now.saturating_duration_since(a.seen) < GONE_AFTER)
            .map(|a| {
                let works: Vec<Value> = (self.agents.works.iter())
                    .filter(|w| w.look.id == a.look.id)
                    .map(|w| {
                        json!({
                            "work": w.id, "name": w.name, "status": w.status.name(),
                            "step": w.step, "steps": w.steps,
                        })
                    })
                    .collect();
                json!({
                    "id": a.look.id,
                    "name": a.look.label,
                    "color": a.look.hex,
                    "surface": "canvas",
                    "file": self.path(),
                    "action": a.doing,
                    "seconds_ago": now.saturating_duration_since(a.seen).as_secs(),
                    "works": works,
                })
            })
            .collect()
    }
}

/// Whether the text an op named `name` writes is revealed at reading speed: what an agent draws,
/// changes or runs, not a canvas loaded in its place.
fn reveals(name: Option<&str>) -> bool {
    matches!(name, Some("add" | "update" | "mermaid" | "command"))
}
