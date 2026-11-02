//! The canvas's side of agents' works: a work begun, an op of a work run (its pointer at the
//! work's corner, and its undo step joined to the work's), progress shown, a row dismissed, and the
//! view taken to a work or an agent at the user's click.
//!
//! **Undo.** FigCraft's undo history is a stack of whole documents, one per step; a group
//! (`Session::grouped`) makes one step of what runs inside one call, and can't span calls. So a
//! work's ops are joined after the fact: when an op of a work pushes one step, and the step before
//! it is the work's own, made by its last op with nothing between them, the new step is taken off
//! and the work's step then undoes both. One undo takes back the whole component while its agent
//! is the only one editing. Another agent's op or the user's edit in between starts a new step of
//! the work: undoing past it would take theirs back too.

use std::sync::Arc;
use std::time::Instant;

use figcraft_doc::{Document, NodeId};
use gpui_kit::*;
use sawit_board::ops::{work_frame, work_of, work_steps};
use sawit_core::agents::work::WorkStatus;
use serde_json::{Value, json};

use super::target::Target;
use super::work::UndoMark;
use super::{AgentActivity, Work};
use crate::agents::Look;
use crate::board::BoardView;

/// The document and the depth of its undo history, before an op.
pub(super) type UndoState = Option<(Arc<Document>, usize)>;

impl BoardView {
    /// The document on show and how many undo steps it has.
    pub(super) fn undo_state(&self) -> UndoState {
        let ds = self.board()?.session.ds().ok()?;
        Some((ds.doc.clone(), ds.history.undo.len()))
    }

    /// The frame of work `id` and its page, in the document on show.
    fn frame_of(&self, id: &str) -> Option<(NodeId, NodeId, Arc<Document>)> {
        let doc = self.board()?.session.ds().ok()?.doc.clone();
        let frame = work_frame(&doc, id)?;
        Some((frame, doc.page_of(frame)?, doc))
    }

    /// Makes sure the canvas shows work `id`, `look`'s: one begun before the app opened the
    /// canvas (with it closed, say) is found by its frame. Whether its frame is on the canvas.
    pub(super) fn know_work(&mut self, id: &str, look: &Look, now: Instant) -> bool {
        let Some((frame, page, doc)) = self.frame_of(id) else {
            return false;
        };
        if self.agents.work(id).is_none() {
            let name = doc.node(frame).map(|n| n.name.clone()).unwrap_or_default();
            let steps = work_steps(&doc, frame);
            let work = Work::new(id, &name, look, (frame, page), steps, now);
            self.agents.works.push(work);
        }
        if let Some(work) = self.agents.work_mut(id) {
            (work.frame, work.page) = (frame, page);
        }
        true
    }

    /// `look`'s agent's `begin` answered `reply`: the canvas shows the new work, its pointer at the
    /// frame's corner, and the work's undo step starts.
    pub(super) fn work_begun(&mut self, op: &Value, reply: &Value, look: &Look, now: Instant) {
        let Some(id) = reply.get("work").and_then(Value::as_str) else {
            return;
        };
        let Some((frame, page, _)) = self.frame_of(id) else {
            return;
        };
        let name = op
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let steps = (reply
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten())
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
        let mut work = Work::new(id, name, look, (frame, page), steps, now);
        work.undo = self
            .undo_state()
            .map(|(doc, depth)| UndoMark { doc, depth });
        self.label_undo(name);
        self.agents.saw(look, &format!("building {name}"), now);
        self.agents.begun(work, now);
        self.point_at_work(id, look, now);
    }

    /// An op of work `id`, `look`'s, ran: its agent was heard from, its pointer goes to the
    /// work's corner, and what the op changed joins the work's undo step when it can (`before` is
    /// the undo state before the op).
    pub(super) fn work_op_ran(&mut self, id: &str, look: &Look, before: UndoState, now: Instant) {
        if !self.know_work(id, look, now) {
            return;
        }
        if let Some(work) = self.agents.work_mut(id) {
            work.heard_at(now);
        }
        self.join_undo(id, before);
        self.point_at_work(id, look, now);
    }

    /// `look`'s agent's pointer rests at work `id`'s top-right corner.
    fn point_at_work(&mut self, id: &str, look: &Look, now: Instant) {
        let Some((frame, page, doc)) = self.frame_of(id) else {
            return;
        };
        if let Some(b) = doc.world_bounds(frame) {
            let corner = Target {
                point: (b.x1, b.y0),
                rects: Vec::new(),
            };
            self.agents.point_at(look, page, corner, Some(id), now);
        }
    }

    /// Joins the undo step an op of work `id` just pushed to the work's own step, when that is the
    /// step right before it ([`UndoMark`]), and names the step after the work.
    fn join_undo(&mut self, id: &str, before: UndoState) {
        let Some((was, depth)) = before else { return };
        let Some(work) = self.agents.works.iter_mut().find(|w| w.id == id) else {
            return;
        };
        let Some(ds) = (self.board.as_mut().ok()).and_then(|b| b.session.ds_mut().ok()) else {
            return;
        };
        if Arc::ptr_eq(&was, &ds.doc) || ds.history.undo.len() != depth + 1 {
            return;
        }
        let follows =
            (work.undo.as_ref()).is_some_and(|m| Arc::ptr_eq(&m.doc, &was) && m.depth == depth);
        if follows {
            ds.history.undo.pop();
        }
        if let Some(step) = ds.history.undo.last_mut() {
            step.label = work.name.to_string();
        }
        work.undo = Some(UndoMark {
            doc: ds.doc.clone(),
            depth: ds.history.undo.len(),
        });
    }

    /// Names the newest undo step `name`: the Edit menu's Undo then says what it takes back.
    fn label_undo(&mut self, name: &str) {
        if let Some(step) = (self.board.as_mut().ok())
            .and_then(|b| b.session.ds_mut().ok())
            .and_then(|ds| ds.history.undo.last_mut())
        {
            step.label = name.to_string();
        }
    }

    /// `look`'s agent's `progress` op: the work's row shows its step and status, and each change
    /// of status is announced. It answers `{ok, work, status, step, shown: true}`.
    pub(super) fn agent_progress(
        &mut self,
        op: &Value,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> Value {
        let failure = |error: String| json!({"ok": false, "error": format!("progress: {error}")});
        let now = cx.background_executor().now();
        let id = match work_of(op) {
            Ok(Some(id)) => id.to_string(),
            Ok(None) => return failure("it needs the `work` canvas_begin answered".into()),
            Err(error) => return failure(error),
        };
        let message = op.get("message").and_then(Value::as_str);
        let status = op.get("status").and_then(Value::as_str);
        let status = match status.map(|s| WorkStatus::parse(s, message)).transpose() {
            Ok(status) => status,
            Err(error) => return failure(error),
        };
        if !self.know_work(&id, look, now) {
            return failure(format!(
                "work “{id}” isn't on the canvas: its frame was deleted, or it was never begun. \
                 canvas_begin starts one"
            ));
        }
        let step = op.get("step").and_then(Value::as_str).map(str::to_string);
        self.agents
            .progressed(&id, step.clone(), status.clone(), now);
        let name = self
            .agents
            .work(&id)
            .map(|w| w.name.clone())
            .unwrap_or_default();
        self.agents.saw(look, &format!("building {name}"), now);
        self.point_at_work(&id, look, now);
        self.after_agents_changed(cx);
        json!({
            "ok": true, "work": id, "status": status.map(|s| s.name()), "step": step,
            "shown": true,
        })
    }

    /// The user dismissed work `id`'s row: a failed or stalled one goes until its agent does
    /// something for it again. Its frame and what was drawn in it stay.
    pub(in crate::board) fn dismiss_work(&mut self, id: &str, cx: &mut Context<Self>) {
        self.agents.dismiss(id);
        self.after_agents_changed(cx);
    }

    /// Takes the view to `point` on page `page`, at the user's click on the roster or an edge chip:
    /// to that page first if another is on show. The zoom stays as it is.
    pub(in crate::board) fn go_to(
        &mut self,
        page: NodeId,
        point: (f64, f64),
        cx: &mut Context<Self>,
    ) {
        let shown = self
            .board()
            .and_then(|b| b.session.ds().ok())
            .map(|ds| ds.page);
        if shown.is_some_and(|shown| shown != page) {
            self.choose("page.select", json!({"id": page.0}), cx);
        }
        if let Some(view) = self.view_mut() {
            view.cx = point.0;
            view.cy = point.1;
        }
        cx.notify();
    }

    /// Takes the view to work `id`'s frame.
    pub(in crate::board) fn go_to_work(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some((frame, page, doc)) = self.frame_of(id)
            && let Some(b) = doc.world_bounds(frame)
        {
            self.go_to(page, ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0), cx);
        }
    }

    /// The user's pointer moved to `position` (in the window): the agent's pointer under it, if
    /// any, shows its name.
    pub(in crate::board) fn hover_agents(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let local = position - self.bounds.origin;
        let point = (f64::from(local.x), f64::from(local.y));
        let now = cx.background_executor().now();
        let hovered = (self.board().and_then(|b| b.session.ds().ok()))
            .and_then(|ds| self.agents.pointer_at(&ds.view, ds.page, point, now));
        if hovered != self.agents.hovered {
            self.agents.hovered = hovered;
            cx.notify();
        }
    }

    /// Redraws, keeps the canvas ticking while agents show, and tells the tab when they start or
    /// stop work.
    pub(super) fn after_agents_changed(&mut self, cx: &mut Context<Self>) {
        self.keep_agents_ticking(cx);
        let now = cx.background_executor().now();
        let at_work = self.agents.at_work(now).is_some();
        if at_work != self.agents.told_at_work {
            self.agents.told_at_work = at_work;
            cx.emit(AgentActivity);
        }
        cx.notify();
    }

    /// The colour of the agent most recently at work on this canvas, while one is: its tab's dot.
    pub fn agents_at_work(&self, cx: &App) -> Option<Hsla> {
        let now = cx.background_executor().now();
        crate::agents::shown(cx)
            .then(|| self.agents.at_work(now).map(|look| look.color))
            .flatten()
    }
}
