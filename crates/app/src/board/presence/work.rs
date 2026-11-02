//! The components agents build on this canvas, as they say they do (`sawit_core::agents::work`):
//! each in its own frame of the document, with what the canvas shows of it kept here, never in
//! the document: its step, its status, when its agent was last heard of, and whether the user
//! dismissed it.

use std::sync::Arc;
use std::time::Instant;

use figcraft_doc::{Document, NodeId};
use gpui_kit::SharedString;
use sawit_core::agents::GONE_AFTER;
use sawit_core::agents::work::{self as model, RowLook, Tally, WorkStatus};

use super::{Presence, SHIMMER};
use crate::agents::Look;

/// What one work's undo step is: the document after its last op, and the undo history's length
/// then. The next op of the work joins that step while nothing else changed the document since.
#[derive(Clone)]
pub(in crate::board) struct UndoMark {
    pub doc: Arc<Document>,
    pub depth: usize,
}

/// A component an agent builds on the canvas.
pub(in crate::board) struct Work {
    pub id: String,
    pub name: SharedString,
    pub look: Look,
    /// Its frame, and the page that holds it.
    pub frame: NodeId,
    pub page: NodeId,
    pub steps: Vec<String>,
    /// The step running, as its agent last said: its first step until then.
    pub step: Option<String>,
    pub status: WorkStatus,
    /// When `status` was last given, and when its agent last did anything for it.
    pub changed: Instant,
    pub heard: Instant,
    /// When it was begun: its skeleton's shimmer runs from then.
    pub begun: Instant,
    /// The user dismissed its row (a failed or stalled one): it shows again when its agent does
    /// something for it.
    pub dismissed: bool,
    /// Its stall was announced.
    pub stall_said: bool,
    pub undo: Option<UndoMark>,
}

impl Work {
    pub fn new(
        id: &str,
        name: &str,
        look: &Look,
        (frame, page): (NodeId, NodeId),
        steps: Vec<String>,
        now: Instant,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string().into(),
            look: look.clone(),
            frame,
            page,
            step: steps.first().cloned(),
            steps,
            status: WorkStatus::Working,
            changed: now,
            heard: now,
            begun: now,
            dismissed: false,
            stall_said: false,
            undo: None,
        }
    }

    /// How its row looks at `now`.
    pub fn row(&self, now: Instant) -> RowLook {
        if self.dismissed {
            return RowLook::Gone;
        }
        let since = now.saturating_duration_since(self.changed);
        model::row_look(
            &self.status,
            since,
            now.saturating_duration_since(self.heard),
        )
    }

    /// Whether it is running and not stalled at `now`.
    pub fn building(&self, now: Instant) -> bool {
        matches!(self.row(now), RowLook::Working { stalled: false })
    }

    /// Whether its row or skeleton moves at `now`: a shimmer or a spinner while it runs, a check
    /// fading once it is done.
    pub fn animating(&self, now: Instant, reduce: bool) -> bool {
        match self.row(now) {
            RowLook::Working { stalled } => !reduce && !stalled,
            RowLook::Done { name, .. } => !name || !reduce,
            RowLook::Failed | RowLook::Gone => false,
        }
    }

    /// Its skeleton's shimmer at `now`, from 0 to 1, none under reduced motion.
    pub fn shimmer(&self, now: Instant, reduce: bool) -> Option<f32> {
        (!reduce).then(|| {
            let into = now.saturating_duration_since(self.begun).as_secs_f32();
            (into / SHIMMER.as_secs_f32()).fract()
        })
    }

    /// Its agent did something for it at `now`: a failed or dismissed one is back at work, its
    /// agent retrying.
    pub fn heard_at(&mut self, now: Instant) {
        self.heard = now;
        self.dismissed = false;
        self.stall_said = false;
        if matches!(self.status, WorkStatus::Failed(_)) {
            self.status = WorkStatus::Working;
            self.changed = now;
        }
    }
}

impl Presence {
    /// The work `id`, if the canvas shows it.
    pub fn work(&self, id: &str) -> Option<&Work> {
        self.works.iter().find(|w| w.id == id)
    }

    pub fn work_mut(&mut self, id: &str) -> Option<&mut Work> {
        self.works.iter_mut().find(|w| w.id == id)
    }

    /// Says `sentence` about `look`'s agent, after what is said before it.
    fn say(&mut self, look: &Look, sentence: String, now: Instant) {
        self.announcer.say(&look.id, sentence.into(), now);
    }

    /// `look`'s agent began `work` at `now`.
    pub fn begun(&mut self, work: Work, now: Instant) {
        let sentence = model::started(work.look.subject(), &work.name);
        let look = work.look.clone();
        self.works.retain(|w| w.id != work.id);
        self.works.push(work);
        self.say(&look, sentence, now);
    }

    /// Work `id` is at `step` and `status`, as its agent says at `now`: each change of status is
    /// announced.
    pub fn progressed(
        &mut self,
        id: &str,
        step: Option<String>,
        status: Option<WorkStatus>,
        now: Instant,
    ) {
        let Some(work) = self.work_mut(id) else {
            return;
        };
        work.heard_at(now);
        if let Some(step) = step.filter(|s| !s.trim().is_empty()) {
            work.step = Some(step);
        }
        let Some(status) = status.filter(|s| *s != work.status) else {
            return;
        };
        work.status = status.clone();
        work.changed = now;
        let (subject, name, look) = (
            work.look.subject().clone(),
            work.name.clone(),
            work.look.clone(),
        );
        let sentence = match &status {
            WorkStatus::Working => model::started(&subject, &name),
            WorkStatus::Done => model::finished(&subject, &name),
            WorkStatus::Failed(reason) => model::failed(&subject, &name, reason),
        };
        self.say(&look, sentence, now);
    }

    /// The user dismissed work `id`'s row.
    pub fn dismiss(&mut self, id: &str) {
        if let Some(work) = self.work_mut(id) {
            work.dismissed = true;
        }
    }

    /// Lets go of the works that are over at `now`: done for [`GONE_AFTER`], or dismissed; and
    /// announces each stall once.
    pub(super) fn prune_works(&mut self, now: Instant) {
        self.works.retain(|w| {
            let over = matches!(w.status, WorkStatus::Done)
                && now.saturating_duration_since(w.changed) >= GONE_AFTER;
            !over && !w.dismissed
        });
        let stalls: Vec<(Look, SharedString)> = (self.works.iter_mut())
            .filter(|w| !w.stall_said && w.row(now) == RowLook::Working { stalled: true })
            .map(|w| {
                w.stall_said = true;
                (w.look.clone(), w.name.clone())
            })
            .collect();
        for (look, name) in stalls {
            self.say(&look, model::stalled(look.subject(), &name), now);
        }
    }

    /// The works whose frame `doc` no longer holds (the user deleted it, or undid its begin) go.
    pub fn keep_works_in(&mut self, doc: &Document) {
        self.works.retain(|w| doc.node(w.frame).is_some());
    }

    /// The overlay's `shapes` without the names of the frames whose work's row shows at `now`: the
    /// row names the frame there.
    pub fn without_work_names(
        &self,
        mut shapes: Vec<sawit_board::overlay::Shape>,
        now: Instant,
    ) -> Vec<sawit_board::overlay::Shape> {
        if self.works.is_empty() {
            return shapes;
        }
        shapes.retain(|shape| match shape {
            sawit_board::overlay::Shape::Label { node: Some(id), .. } => {
                !(self.works.iter()).any(|w| w.frame == *id && w.row(now) != RowLook::Gone)
            }
            _ => true,
        });
        shapes
    }

    /// How many works and agents are in each state at `now`, for the roster.
    pub fn tally(&self, now: Instant) -> Tally {
        let mut tally = Tally::default();
        for work in &self.works {
            match work.row(now) {
                RowLook::Working { stalled: false } => tally.building += 1,
                RowLook::Working { stalled: true } => tally.stalled += 1,
                RowLook::Done { .. } => tally.done += 1,
                RowLook::Gone if matches!(work.status, WorkStatus::Done) && !work.dismissed => {
                    tally.done += 1;
                }
                RowLook::Failed => tally.failed += 1,
                RowLook::Gone => {}
            }
        }
        for agent in &self.agents {
            let busy = (self.works.iter()).any(|w| {
                w.look.id == agent.look.id
                    && matches!(w.row(now), RowLook::Working { .. } | RowLook::Failed)
            });
            let recent = now.saturating_duration_since(agent.seen) < sawit_core::agents::FADE_AFTER;
            match (busy, recent) {
                (true, _) => {}
                (false, true) => tally.active += 1,
                (false, false) => tally.idle += 1,
            }
        }
        tally
    }
}
