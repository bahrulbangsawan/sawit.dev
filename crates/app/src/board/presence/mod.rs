//! Agents on the canvas, as a multiplayer canvas shows its other people, and the components they
//! build there.
//!
//! - **The pointer**: 10 × 14 px in the agent's colour with a 1.5 px white edge, the same size at
//!   every zoom ([`POINTER`]). It rests just outside the top-right corner of what the agent works
//!   on: its component's frame, or for an op outside a component, what the op added, changed or
//!   took off ([`target`](target::target)). It glides there for [`GLIDE`], eased out, and jumps
//!   under reduced motion. The most recent agent draws on top.
//! - **The name**: the agent's name only. On a component it is on the component's row; a free
//!   pointer's is a pill beside it ([`labels`]), while the agent is at work and for
//!   [`NAME_AFTER`] after, and while the user's pointer rests on it. A pill that would cover
//!   another is left out.
//! - **A component** an agent builds ([`work`]): a skeleton in its frame until content lands, a
//!   row above it with its name and step, the agent's name and a badge ([`rows`]), "Stalled" after
//!   a minute without word, "!" and the reason when it failed ([`Work`]).
//! - **The roster** in the top bar ([`roster`]) and a chip per edge for agents at work off screen
//!   ([`edges`]); both list the agents, and the user's click takes the view there.
//! - **Text** an agent writes reveals at reading speed ([`reveal`]).
//! - **Screen readers** hear one polite sentence per change, queued per agent ([`announce`]).
//!
//! None of it is the user's: an agent's op leaves the user's selection, keyboard, view and page as
//! they are (`sawit_board::ops`). What the canvas paints for it is decorative to assistive
//! technology; the roster and the chips are buttons, and the status line speaks.
//!
//! An op that takes noticeable time (`sawit_core::agents::slow_canvas_op`) shows a placeholder at
//! once where its result will land ([`ghost_rect`](target::ghost_rect)), and runs once that has
//! been painted ([`PLACEHOLDER_LEAD`]).

mod announce;
mod edges;
mod labels;
mod marks;
mod ops;
mod paint;
mod reveal;
mod roster;
mod rows;
mod target;
mod work;
mod work_ops;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId};
use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::agents::work::NAME_AFTER;
use sawit_core::agents::{self as model, GONE_AFTER};
use serde_json::Value;

pub(super) use marks::Painted;
pub(super) use paint::paint;
pub(super) use roster::render_roster;
use target::Target;
pub(super) use work::Work;

use crate::agents::Look;

/// How long a pointer glides to its new place.
pub(super) const GLIDE: Duration = Duration::from_millis(180);
/// How long the layers a free op touched stay outlined, fading out over the last half.
const TOUCHED_TIME: Duration = Duration::from_millis(1200);
/// How long a slow op's placeholder shows before the op runs.
pub(super) const PLACEHOLDER_LEAD: Duration = Duration::from_millis(50);
/// How long a placeholder whose op is done waits at most for the frame with its result.
const LANDING_TIME: Duration = Duration::from_millis(1500);
/// How long a failed op's placeholder stays, with its error mark.
const FAILED_TIME: Duration = Duration::from_millis(1600);
/// How long one sweep of a placeholder's or a skeleton's shimmer takes.
const SHIMMER: Duration = Duration::from_millis(1400);
/// A placeholder's size when the op doesn't say how big its result is (page units).
const GHOST_SIZE: (f64, f64) = (320.0, 200.0);
/// How far right of what a page holds a diagram goes when its op names no place (page units).
const DIAGRAM_GAP: f64 = 100.0;
/// How many layers a free op may touch and have each outlined; more are outlined as one box.
const MAX_OUTLINES: usize = 8;
/// How far an off-screen chip stays inside the canvas's edge (px).
const EDGE_INSET: f64 = 12.0;
/// How long after announcing what an agent did outside a component the canvas waits before
/// announcing that agent's next op.
const ANNOUNCE_GAP: Duration = Duration::from_secs(2);
/// The pointer's outline, from its tip (px): 7 × 11, which its 1.5 px white edge makes 10 × 14.
const POINTER: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.0, 9.6),
    (2.4, 7.2),
    (4.1, 11.0),
    (5.5, 10.3),
    (3.9, 6.6),
    (7.0, 6.5),
];
/// Where the pointer's tip goes from the corner it rests at (px): just outside it.
const TIP_FROM_CORNER: (f64, f64) = (3.0, 1.0);

/// Where on the canvas an agent works: a point on a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Spot {
    pub page: NodeId,
    pub point: (f64, f64),
}

/// One agent on this canvas.
pub(super) struct Agent {
    pub look: Look,
    /// Where its pointer goes: the top-right corner of what it works on.
    pub at: Option<Spot>,
    /// Where its pointer glides from, and when it set off.
    from: Option<(f64, f64)>,
    moved: Instant,
    /// What it does, in words ("adding 3 stickies"), and when it last did anything here.
    pub doing: String,
    pub seen: Instant,
    /// The work its pointer rests on, when its last op was one of a work's: its name is on that
    /// work's row, not on a pill.
    pub work: Option<String>,
    /// The layers a free op of it touched last, as page rectangles on `touched_page`, outlined
    /// until [`TOUCHED_TIME`] after `touched_at`.
    touched: Vec<[f64; 4]>,
    touched_page: Option<NodeId>,
    touched_at: Instant,
}

impl Agent {
    /// Where its pointer is at `now`: on its way from where it was, eased out, or there.
    fn point(&self, now: Instant, reduce: bool) -> Option<(f64, f64)> {
        let to = self.at?.point;
        let Some(from) = self.from.filter(|_| !reduce) else {
            return Some(to);
        };
        let t = model::glide(now.saturating_duration_since(self.moved), GLIDE);
        Some((from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t))
    }

    /// Whether it did something in the last [`NAME_AFTER`]: its name shows.
    pub fn named(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.seen) < NAME_AFTER
    }
}

/// What becomes of a placeholder.
pub(super) enum GhostState {
    /// Its op hasn't run yet.
    Waiting,
    /// Its op ran and changed the document to `doc`: it goes with the frame that shows it.
    Landed { doc: Arc<Document>, at: Instant },
    /// Its op failed: it shows an error mark for a moment.
    Failed { at: Instant },
}

/// A placeholder where a slow op's result will land.
pub(super) struct Ghost {
    pub id: u64,
    pub look: Look,
    pub page: NodeId,
    /// The page rectangle `[x0, y0, x1, y1]` it covers.
    pub rect: [f64; 4],
    started: Instant,
    pub state: GhostState,
}

/// An agent's op waiting for its placeholder to be painted, or for the op before it.
pub(super) struct Waiting {
    pub op: Value,
    pub look: Look,
    pub ghost: Option<u64>,
    pub reply: oneshot::Sender<Result<Value, String>>,
}

/// The agents on one canvas, and the works they build there.
#[derive(Default)]
pub(super) struct Presence {
    pub agents: Vec<Agent>,
    pub ghosts: Vec<Ghost>,
    pub works: Vec<Work>,
    /// Agents' ops in the order they came, while one of them waits for its placeholder to show.
    pub queue: VecDeque<Waiting>,
    /// The queue is to be run, once [`PLACEHOLDER_LEAD`] is over.
    pub draining: bool,
    /// The polite status line's sentences, one at a time.
    pub announcer: announce::Announcer,
    /// The text agents' ops wrote, revealed at reading speed.
    pub reveals: reveal::Reveals,
    /// The agent whose pointer the user's pointer rests on: its name shows.
    pub hovered: Option<SharedString>,
    next_ghost: u64,
    /// The canvas redraws every [`TICK`] while an agent shows, so they fade and go on time.
    pub ticking: bool,
    /// Whether agents were at work when the canvas last told its tab ([`AgentActivity`]).
    pub told_at_work: bool,
}

/// How often the canvas redraws while an agent shows and nothing moves.
const TICK: Duration = Duration::from_millis(250);
/// How often it redraws while only works show: their rows change by the second at most.
const QUIET_TICK: Duration = Duration::from_secs(1);

impl Presence {
    /// `look`'s agent did something here at `now`: `doing`, in words.
    pub fn saw(&mut self, look: &Look, doing: &str, now: Instant) -> &mut Agent {
        let ix = match self.agents.iter().position(|a| a.look.id == look.id) {
            Some(ix) => ix,
            None => {
                self.agents.push(Agent {
                    look: look.clone(),
                    at: None,
                    from: None,
                    moved: now,
                    doing: String::new(),
                    seen: now,
                    work: None,
                    touched: Vec::new(),
                    touched_page: None,
                    touched_at: now,
                });
                self.agents.len() - 1
            }
        };
        let agent = &mut self.agents[ix];
        agent.look = look.clone();
        agent.doing = doing.to_string();
        agent.seen = now;
        agent
    }

    /// `look`'s agent's pointer goes to `target` on page `page`, gliding from where it was
    /// (unless motion is `reduce`d), and the layers it touched are outlined; `work` is the work
    /// it is at, if any.
    pub fn point_at(
        &mut self,
        look: &Look,
        page: NodeId,
        target: Target,
        work: Option<&str>,
        now: Instant,
    ) {
        let agent = self.saw_quietly(look, now);
        agent.from = match agent.at {
            Some(at) if at.page == page => Some(agent.point(now, false).unwrap_or(at.point)),
            _ => None,
        };
        agent.at = Some(Spot {
            page,
            point: target.point,
        });
        agent.moved = now;
        agent.work = work.map(str::to_string);
        agent.touched = target.rects;
        agent.touched_page = Some(page);
        agent.touched_at = now;
    }

    fn saw_quietly(&mut self, look: &Look, now: Instant) -> &mut Agent {
        let doing = (self.agents.iter())
            .find(|a| a.look.id == look.id)
            .map(|a| a.doing.clone())
            .unwrap_or_default();
        self.saw(look, &doing, now)
    }

    /// A placeholder for `look`'s slow op over `rect` on `page`, with the pointer at its corner.
    pub fn add_ghost(&mut self, look: &Look, page: NodeId, rect: [f64; 4], now: Instant) -> u64 {
        self.next_ghost += 1;
        let id = self.next_ghost;
        self.ghosts.push(Ghost {
            id,
            look: look.clone(),
            page,
            rect,
            started: now,
            state: GhostState::Waiting,
        });
        let corner = Target {
            point: (rect[2], rect[1]),
            rects: Vec::new(),
        };
        self.point_at(look, page, corner, None, now);
        id
    }

    /// The op of placeholder `id` changed the document to `doc`.
    pub fn land(&mut self, id: u64, doc: Arc<Document>, now: Instant) {
        if let Some(ghost) = self.ghosts.iter_mut().find(|g| g.id == id) {
            ghost.state = GhostState::Landed { doc, at: now };
        }
    }

    /// The op of placeholder `id` failed, or changed nothing: an error mark, or nothing.
    pub fn fail(&mut self, id: u64, failed: bool, now: Instant) {
        match failed {
            true => {
                if let Some(ghost) = self.ghosts.iter_mut().find(|g| g.id == id) {
                    ghost.state = GhostState::Failed { at: now };
                }
            }
            false => self.ghosts.retain(|g| g.id != id),
        }
    }

    /// The render thread handed over a frame of `doc`: the placeholders whose result it shows go.
    pub fn rendered(&mut self, doc: &Arc<Document>) {
        let doc = self.reveals.source_of(doc);
        self.ghosts.retain(|g| match &g.state {
            GhostState::Landed { doc: landed, .. } => !Arc::ptr_eq(landed, &doc),
            _ => true,
        });
    }

    /// Lets go of what has had its time at `now`: agents quiet for 30 s, outlines that faded,
    /// placeholders whose result came (or never will) and error marks, works that are over, and
    /// moves the announcements on. Whether anything still shows.
    pub fn prune(&mut self, now: Instant) -> bool {
        self.agents
            .retain(|a| now.saturating_duration_since(a.seen) < GONE_AFTER);
        for agent in &mut self.agents {
            if now.saturating_duration_since(agent.touched_at) >= TOUCHED_TIME {
                agent.touched.clear();
            }
        }
        self.ghosts.retain(|g| match &g.state {
            GhostState::Waiting => true,
            GhostState::Landed { at, .. } => now.saturating_duration_since(*at) < LANDING_TIME,
            GhostState::Failed { at } => now.saturating_duration_since(*at) < FAILED_TIME,
        });
        self.prune_works(now);
        self.announcer.tick(now);
        !self.agents.is_empty()
            || !self.ghosts.is_empty()
            || !self.works.is_empty()
            || self.announcer.pending()
    }

    /// How often the canvas redraws for its agents: every [`TICK`] while an agent shows, a
    /// placeholder waits or a sentence is to be said; every [`QUIET_TICK`] while only works show,
    /// which change no faster (a failed one may stay for hours).
    pub fn tick_every(&self) -> Duration {
        match self.agents.is_empty() && self.ghosts.is_empty() && !self.announcer.pending() {
            true => QUIET_TICK,
            false => TICK,
        }
    }

    /// Whether something moves at `now`, so the canvas draws the next frame too: a pointer on its
    /// way, an outline or a check fading, a shimmer, text being revealed.
    pub fn animating(&self, now: Instant, reduce: bool) -> bool {
        let gliding = |a: &Agent| !reduce && now.saturating_duration_since(a.moved) < GLIDE;
        let touched = |a: &Agent| {
            !a.touched.is_empty() && now.saturating_duration_since(a.touched_at) < TOUCHED_TIME
        };
        let shimmer = |g: &Ghost| !reduce && matches!(g.state, GhostState::Waiting);
        let fading = |a: &Agent| {
            let idle = now.saturating_duration_since(a.seen);
            idle > model::FADE_AFTER && idle < model::FADE_AFTER + model::FADE_TIME
        };
        (self.agents.iter()).any(|a| gliding(a) || touched(a) || fading(a))
            || self.ghosts.iter().any(shimmer)
            || self.works.iter().any(|w| w.animating(now, reduce))
            || (!reduce && self.reveals.active(now))
    }

    /// The agent at work here most recently, while one is: its op in the last
    /// [`model::FADE_AFTER`], or a work of its still running. Its colour is the canvas tab's dot.
    pub fn at_work(&self, now: Instant) -> Option<&Look> {
        let busy = |a: &&Agent| {
            now.saturating_duration_since(a.seen) < model::FADE_AFTER
                || self
                    .works
                    .iter()
                    .any(|w| w.look.id == a.look.id && w.building(now))
        };
        self.agents
            .iter()
            .filter(busy)
            .max_by_key(|a| a.seen)
            .map(|a| &a.look)
    }

    /// Where `look`'s agent's pointer is going, on which page.
    #[cfg(test)]
    pub fn spot(&self, id: &str) -> Option<Spot> {
        self.agents.iter().find(|a| a.look.id == id)?.at
    }
}

/// The canvas's agents started or stopped working: its tab shows a dot while they work.
pub struct AgentActivity;

#[cfg(test)]
mod tests;
