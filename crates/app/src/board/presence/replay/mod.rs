//! The replay engine (30c): an agent's op is played over the canvas as a designer would make it,
//! after the fact. An MCP call arrives whole and is applied to the document at once (saved,
//! undoable, seen by `canvas_get`, exports and other agents); the replay is only what is drawn.
//!
//! Each lane (an agent on screen) has its own queue, so lanes play at once, never in turn. An op
//! becomes a script of element tasks ([`script`]), each played in time: the layer is hidden from
//! the drawn copy of the document until its task places it, its text is typed a word at a time,
//! and the moving parts (the growing box, its size badge, the rubber band, the caret, the copy
//! being dragged) are overlay marks ([`marks`]). The copy is made again only when an element
//! reaches a milestone (placed, a word typed), never every frame.
//!
//! **Never behind.** A lane that gets a new op while it plays speeds what is left up, 2× then 4×,
//! then snaps it. A layer the document no longer has as the op left it (undone, edited by the
//! user, changed again) snaps at once, and so does one the user selects; off-screen layers don't
//! play at all. Nothing waits on the replay: the op's answer went before it began.

pub(in crate::board) mod blueprint;
mod covers;
#[cfg(test)]
pub(in crate::board) use covers::COVER_LIMIT;
pub(in crate::board) use covers::cover_expired;
pub(in crate::board) mod marks;
pub(in crate::board) mod script;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Instant;

use figcraft_doc::{Document, NodeId, NodeKind};
use gpui_kit::SharedString;

use super::pointer::{Tool, trip_time};
use crate::agents::Look;
use script::{Kind, Task};

/// How fast replays play: as designed, twice as fast, or not at all (the setting, 30f).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::board) enum Speed {
    #[default]
    Full,
    Fast,
    Off,
}

impl Speed {
    /// The replays' speed for the user's setting.
    pub fn of(setting: crate::agents::BuildAnimation) -> Self {
        match setting {
            crate::agents::BuildAnimation::Full => Speed::Full,
            crate::agents::BuildAnimation::Fast => Speed::Fast,
            crate::agents::BuildAnimation::Off => Speed::Off,
        }
    }

    pub fn factor(self) -> f64 {
        match self {
            Speed::Full => 1.0,
            Speed::Fast => 2.0,
            Speed::Off => f64::INFINITY,
        }
    }
}

/// How long a screen element's cover takes to fade to the live page once it has played.
pub(in crate::board) const GO_LIVE: std::time::Duration = std::time::Duration::from_millis(220);

/// How long after its end a task still draws: a screen element's cover fading out.
fn linger(t: &Timed) -> std::time::Duration {
    if t.task.cover {
        GO_LIVE
    } else {
        std::time::Duration::ZERO
    }
}

/// How many times a lane speeds up before it snaps.
const SNAP_AFTER: u8 = 3;

/// A task placed in time: the pointer travels from `from` to the task's start, then the element
/// plays until `end`.
#[derive(Clone, Debug)]
pub(in crate::board) struct Timed {
    pub task: Task,
    /// When its op was played: a cover lasts [`COVER_LIMIT`] from here at most.
    pub made: Instant,
    pub from: (f64, f64),
    pub travel: Instant,
    pub start: Instant,
    pub end: Instant,
}

impl Timed {
    /// How far the element is into its play at `now`, 0 to 1; `None` before its pointer arrives.
    pub fn progress(&self, now: Instant) -> Option<f64> {
        if now < self.start {
            return None;
        }
        let total = self.end.saturating_duration_since(self.start).as_secs_f64();
        if total <= 0.0 {
            return Some(1.0);
        }
        Some((now.saturating_duration_since(self.start).as_secs_f64() / total).min(1.0))
    }

    /// Where the element's play begins: the corner the pointer goes to.
    pub fn start_point(&self) -> (f64, f64) {
        match &self.task.kind {
            Kind::Duplicate { from } | Kind::Update { from } => (from[0], from[1]),
            _ => (self.task.rect[0], self.task.rect[1]),
        }
    }
}

/// One lane's replay: its look, the page it plays on, the document its last op left, the tasks
/// in time, and how often it has been sped up.
pub(in crate::board) struct Lane {
    pub look: Look,
    pub page: NodeId,
    pub after: Arc<Document>,
    pub tasks: Vec<Timed>,
    pub level: u8,
}

/// How far each task stands at a moment, for the drawn copy: hidden, typed to a char, or whole.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Hidden,
    Typed(usize),
}

/// A drawn copy: the document it was made from, each task's stage, and the copy.
type Drawn = (Arc<Document>, Vec<(NodeId, Stage)>, Arc<Document>);

/// The canvas's replays, one per lane, and the copy of the document drawn with them.
#[derive(Default)]
pub(in crate::board) struct Replays {
    pub lanes: Vec<Lane>,
    pub speed: Speed,
    /// When [`Replays::sounds`] last looked, and the snaps since.
    pub(super) heard: Option<Instant>,
    pub(super) snapped: Vec<(super::sound::Sound, (f64, f64))>,
    drawn: Option<Drawn>,
}

impl Replays {
    /// `look`'s op changed `before` into `after` on `page`: its script plays after what the lane
    /// still plays, which speeds up (2×, 4×) or snaps. `visible` is the page rect on screen:
    /// what lies outside it doesn't play. Under reduced motion or with the setting Off, nothing
    /// plays.
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &mut self,
        look: &Look,
        before: &Document,
        after: &Arc<Document>,
        page: NodeId,
        visible: Option<[f64; 4]>,
        start_at: (f64, f64),
        now: Instant,
        reduce: bool,
    ) {
        if self.speed == Speed::Off {
            return;
        }
        let shown = |r: &[f64; 4]| {
            visible.is_none_or(|v| r[0] < v[2] && v[0] < r[2] && r[1] < v[3] && v[1] < r[3])
        };
        let tasks: Vec<Task> = script::script(before, after, page)
            .into_iter()
            .filter(|t| shown(&t.rect))
            .collect();
        if reduce {
            // Reduced motion: nothing drags, types or travels; what was added fades in, at once.
            let fades = tasks
                .into_iter()
                .filter(|t| !matches!(t.kind, Kind::Delete | Kind::Update { .. }))
                .map(|t| Task::new(t.node, Kind::Fade, t.rect, None));
            self.fade(look, page, after, fades.collect(), now);
            return;
        }
        self.schedule(look, page, after, tasks, start_at, now);
    }

    #[allow(clippy::too_many_arguments)]
    /// Plays a screen's changed elements (`tasks`, [`blueprint::tasks`]) for `look` over its
    /// frame on `page`, in the document `doc`.
    pub fn play_screen(
        &mut self,
        look: &Look,
        page: NodeId,
        doc: &Arc<Document>,
        tasks: Vec<Task>,
        start_at: (f64, f64),
        now: Instant,
        reduce: bool,
    ) {
        // A newer write of the screen replaces what an older one still covers, in every lane: its
        // covers go at once, never left over the live page (B17).
        let frames: Vec<NodeId> = tasks.iter().map(|t| t.node).collect();
        self.drop_covers(&frames);
        if reduce || self.speed == Speed::Off || tasks.is_empty() {
            return;
        }
        self.schedule(look, page, doc, tasks, start_at, now);
    }

    /// Reduced motion's play: every one of `tasks` fades in together, from `now`.
    fn fade(
        &mut self,
        look: &Look,
        page: NodeId,
        after: &Arc<Document>,
        tasks: Vec<Task>,
        now: Instant,
    ) {
        if tasks.is_empty() {
            return;
        }
        self.lanes.retain(|l| l.look.id != look.id);
        let timed = tasks
            .into_iter()
            .map(|task| {
                let end = now + task.time;
                Timed {
                    made: now,
                    from: (task.rect[0], task.rect[1]),
                    travel: now,
                    start: now,
                    end,
                    task,
                }
            })
            .collect();
        self.lanes.push(Lane {
            look: look.clone(),
            page,
            after: after.clone(),
            tasks: timed,
            level: 0,
        });
    }

    /// Puts `tasks` in `look`'s lane after what it still plays, which speeds up or snaps.
    fn schedule(
        &mut self,
        look: &Look,
        page: NodeId,
        after: &Arc<Document>,
        tasks: Vec<Task>,
        start_at: (f64, f64),
        now: Instant,
    ) {
        let factor = self.speed.factor();
        let ix = match self.lanes.iter().position(|l| l.look.id == look.id) {
            Some(ix) => ix,
            None => {
                self.lanes.push(Lane {
                    look: look.clone(),
                    page,
                    after: after.clone(),
                    tasks: Vec::new(),
                    level: 0,
                });
                self.lanes.len() - 1
            }
        };
        let lane = &mut self.lanes[ix];
        lane.tasks.retain(|t| t.end > now);
        if lane.tasks.is_empty() {
            lane.level = 0;
        } else {
            lane.level += 1;
            if lane.level >= SNAP_AFTER {
                let at = lane
                    .tasks
                    .first()
                    .map_or((0.0, 0.0), |t| (t.task.rect[0], t.task.rect[1]));
                self.snapped.push((super::sound::Sound::Snap, at));
            }
            catch_up(lane, now, factor);
        }
        lane.look = look.clone();
        lane.page = page;
        lane.after = after.clone();
        if tasks.is_empty() {
            return;
        }
        let mut at = lane.tasks.last().map_or(now, |t| t.end);
        let mut from = lane
            .tasks
            .last()
            .map_or(start_at, |t| (t.task.rect[2], t.task.rect[3]));
        let speed = factor * 2f64.powi(i32::from(lane.level.min(2)));
        for task in tasks {
            let mut timed = Timed {
                made: now,
                from,
                travel: at,
                start: at,
                end: at,
                task,
            };
            let travel = trip_time(from, timed.start_point()).div_f64(speed);
            timed.start = at + travel;
            timed.end = timed.start + timed.task.time.div_f64(speed);
            at = timed.end;
            from = (timed.task.rect[2], timed.task.rect[3]);
            lane.tasks.push(timed);
        }
    }

    /// Whether anything still plays at `now`.
    pub fn active(&self, now: Instant) -> bool {
        self.lanes
            .iter()
            .any(|l| l.tasks.iter().any(|t| t.end > now))
    }

    /// Ends every replay at once: undo, a load, the setting turned Off.
    pub fn cancel(&mut self) {
        self.lanes.clear();
        self.drawn = None;
    }

    /// Where lane `id`'s pointer is at `now` (page point) and which tool it shows, while it plays.
    pub fn pointer(&self, id: &SharedString, now: Instant) -> Option<((f64, f64), Tool)> {
        let lane = self.lanes.iter().find(|l| &l.look.id == id)?;
        let task = lane.tasks.iter().find(|t| t.end > now && t.travel <= now)?;
        if now < task.start {
            let total = task
                .start
                .saturating_duration_since(task.travel)
                .as_secs_f64();
            let t = now.saturating_duration_since(task.travel).as_secs_f64() / total.max(1e-6);
            let point = super::pointer::travel(task.from, task.start_point(), t, 1.0);
            return Some((point, Tool::Arrow));
        }
        let p = task.progress(now).unwrap_or(1.0);
        Some((
            marks::pointer_point(task, p),
            marks::tool_of(&task.task.kind),
        ))
    }

    /// The document to draw at `now` in place of `doc`: a copy in which the layers not placed yet
    /// are hidden and text being typed shows its typed part; `None` while nothing plays. A layer
    /// the document no longer has as its op left it, or one in `selected`, snaps whole.
    pub fn document(
        &mut self,
        doc: &Arc<Document>,
        selected: &[NodeId],
        now: Instant,
    ) -> Option<Arc<Document>> {
        for lane in &mut self.lanes {
            let after = lane.after.clone();
            lane.tasks.retain(|t| {
                let node = t.task.node;
                let same = match (after.node(node), doc.node(node)) {
                    (Some(a), Some(b)) => std::ptr::eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                t.end + linger(t) > now
                    && same
                    && !selected.contains(&node)
                    && !cover_expired(t, now)
            });
        }
        self.lanes.retain(|l| !l.tasks.is_empty());
        let mut stages: Vec<(NodeId, Stage)> = Vec::new();
        for lane in &self.lanes {
            for t in &lane.tasks {
                if let Some(stage) = stage(t, now) {
                    stages.push((t.task.node, stage));
                }
            }
        }
        if stages.is_empty() {
            self.drawn = None;
            return None;
        }
        if let Some((from, was, copy)) = &self.drawn
            && Arc::ptr_eq(from, doc)
            && *was == stages
        {
            return Some(copy.clone());
        }
        let mut copy = (**doc).clone();
        for (node, stage) in &stages {
            let Some(n) = copy.node_mut(*node) else {
                continue;
            };
            match stage {
                Stage::Hidden => n.visible = false,
                Stage::Typed(shown) => {
                    if let NodeKind::Text(text) = &mut n.kind {
                        let end = text.characters.chars().count();
                        if *shown < end {
                            text.restyle(*shown, end, |style| style.fills = Some(Vec::new()));
                        }
                    }
                }
            }
        }
        let copy = Arc::new(copy);
        self.drawn = Some((doc.clone(), stages, copy.clone()));
        Some(copy)
    }

    /// The document `doc` was drawn from: itself, or the one a replay's copy was made of.
    pub fn source_of(&self, doc: &Arc<Document>) -> Arc<Document> {
        match &self.drawn {
            Some((from, _, copy)) if Arc::ptr_eq(copy, doc) => from.clone(),
            _ => doc.clone(),
        }
    }
}

/// What the drawn copy shows of task `t` at `now`: hidden until its element is placed, its text
/// typed to the last whole word; `None` once it is whole (or for a task that shows its layer as
/// it plays: an image wiping in, a moved layer, a deletion).
fn stage(t: &Timed, now: Instant) -> Option<Stage> {
    if t.task.cover || matches!(t.task.kind, Kind::Fade) {
        return None;
    }
    let progress = t.progress(now);
    match &t.task.kind {
        Kind::Delete | Kind::Image => match progress {
            None if matches!(t.task.kind, Kind::Image) => Some(Stage::Hidden),
            _ => None,
        },
        Kind::Text { chars, .. } => match progress {
            None => Some(Stage::Hidden),
            Some(p) if p < 1.0 => Some(Stage::Typed(word_end(*chars, p))),
            Some(_) => None,
        },
        _ => match progress {
            Some(p) if p >= 1.0 => None,
            _ => Some(Stage::Hidden),
        },
    }
}

/// How many of `chars` characters show `p` into the typing, rounded down to a step of four, so
/// the copy is made again a few characters at a time, not every frame.
fn word_end(chars: usize, p: f64) -> usize {
    let typed = (chars as f64 * p) as usize;
    typed - typed % 4
}

/// A lane got a new op while playing: what is left plays at 2×, then 4×, from `now`; on the third
/// time it snaps.
fn catch_up(lane: &mut Lane, now: Instant, factor: f64) {
    if lane.level >= SNAP_AFTER {
        lane.tasks.clear();
        lane.level = 0;
        return;
    }
    let speed = factor * 2f64.powi(i32::from(lane.level));
    let mut at = now;
    for t in &mut lane.tasks {
        let travel = t
            .start
            .saturating_duration_since(t.travel.max(now))
            .div_f64(speed / factor);
        let play = t
            .end
            .saturating_duration_since(t.start.max(now))
            .div_f64(speed / factor);
        t.travel = at;
        t.start = at + travel;
        t.end = t.start + play;
        at = t.end;
    }
}

/// What each lane's script holds, for tests and `app_state`: lane id → (tasks left, sped up).
#[cfg(test)]
impl Replays {
    pub fn queues(&self, now: Instant) -> Vec<(SharedString, usize)> {
        self.lanes
            .iter()
            .map(|l| {
                (
                    l.look.id.clone(),
                    l.tasks.iter().filter(|t| t.end > now).count(),
                )
            })
            .collect()
    }
}
