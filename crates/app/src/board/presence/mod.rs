//! Agents on the canvas, as a multiplayer canvas shows its other people: each agent's pointer in
//! its colour with a pill of its name and what it does, the layers it just touched outlined in
//! its colour for a moment, a placeholder where a slow op's result is about to land, and a chip
//! at the canvas's edge for an agent at work off screen, which takes the view there when clicked.
//!
//! None of it is the user's: an agent's op leaves the user's selection and keyboard as they are
//! (`sawit_board::ops`), and its cursor never moves the view. What the canvas paints for it is
//! decorative to assistive technology; the canvas announces what agents did in a polite status
//! line instead ([`Presence::announcement`]).
//!
//! The pointer goes to the op's target: the top-left corner of what it added, else the centre of
//! what it changed (one layer's bounds, or the bounds of several, as a selection's centre), found
//! by comparing the document before and after the op ([`target`](target::target)). It glides
//! there for [`GLIDE`], eased out, and jumps under reduced motion.
//!
//! An op that takes noticeable time (`sawit_core::agents::slow_canvas_op`: a Mermaid layout, an
//! image or file brought in, a long `command.batch`) shows a placeholder at once where its result
//! will land ([`ghost_rect`](target::ghost_rect)), and runs once that has been painted
//! ([`PLACEHOLDER_LEAD`]). The placeholder goes when the render thread hands over the frame with
//! the result, or, if the op failed, after a moment with an error mark.

mod edges;
mod marks;
mod ops;
mod target;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId};
use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::agents::{self as model, GONE_AFTER};
use serde_json::Value;

pub(super) use marks::{Painted, paint};
use target::Target;

use crate::agents::Look;

/// How long a pointer glides to its new place.
pub(super) const GLIDE: Duration = Duration::from_millis(180);
/// How long the layers an agent touched stay outlined, fading out over the last half.
const TOUCHED_TIME: Duration = Duration::from_millis(1200);
/// How long a slow op's placeholder shows before the op runs: a few frames, so it is on screen
/// before the op holds the UI thread.
pub(super) const PLACEHOLDER_LEAD: Duration = Duration::from_millis(50);
/// How long a placeholder whose op is done waits at most for the frame with its result.
const LANDING_TIME: Duration = Duration::from_millis(1500);
/// How long a failed op's placeholder stays, with its error mark.
const FAILED_TIME: Duration = Duration::from_millis(1600);
/// How long one sweep of a placeholder's shimmer takes.
const SHIMMER: Duration = Duration::from_millis(1100);
/// A placeholder's size when the op doesn't say how big its result is (page units).
const GHOST_SIZE: (f64, f64) = (320.0, 200.0);
/// How far right of what a page holds a diagram goes when its op names no place, as
/// `sawit_board::ops` puts it there (page units).
const DIAGRAM_GAP: f64 = 100.0;
/// How many layers an op may touch and have each outlined; more (a diagram's) are outlined as
/// one box around them all, as a selection of many is.
const MAX_OUTLINES: usize = 8;
/// How far an off-screen agent's chip stays inside the canvas's edge (px).
const EDGE_INSET: f64 = 12.0;
/// The chip's size (px), for placing it.
pub(super) const EDGE_CHIP: (f64, f64) = (120.0, 24.0);
/// How long after announcing what an agent did the canvas waits before announcing that agent
/// again: a screen reader would otherwise read every op of a busy agent.
const ANNOUNCE_GAP: Duration = Duration::from_secs(2);
/// The pointer's outline, from its tip (px), as the page's overlay draws it too.
const POINTER: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.0, 16.0),
    (4.4, 12.3),
    (7.3, 18.6),
    (9.9, 17.4),
    (7.1, 11.2),
    (12.6, 11.2),
];
/// Where the pill goes from the pointer's tip (px), its height and its padding.
const PILL_AT: (f32, f32) = (12.0, 17.0);
const PILL_HEIGHT: f32 = 20.0;
const PILL_PAD: f32 = 8.0;
const PILL_TEXT: f32 = 11.0;

/// Where on the canvas an agent works: a point on a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Spot {
    pub page: NodeId,
    pub point: (f64, f64),
}

/// One agent on this canvas.
pub(super) struct Agent {
    pub look: Look,
    /// Where its pointer goes, once an op of it had a target.
    pub at: Option<Spot>,
    /// Where its pointer glides from, and when it set off.
    from: Option<(f64, f64)>,
    moved: Instant,
    /// What it does, in words ("adding 3 stickies"), and when it last did anything here.
    pub doing: String,
    pub seen: Instant,
    /// The layers it touched last, as page rectangles on `touched_page`, outlined until
    /// [`TOUCHED_TIME`] after `touched_at`.
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

/// The agents on one canvas.
#[derive(Default)]
pub(super) struct Presence {
    pub agents: Vec<Agent>,
    pub ghosts: Vec<Ghost>,
    /// Agents' ops in the order they came, while one of them waits for its placeholder to show.
    pub queue: VecDeque<Waiting>,
    /// The queue is to be run, once [`PLACEHOLDER_LEAD`] is over.
    pub draining: bool,
    /// What the canvas last announced, read by a polite status line: "Claude Code added 3
    /// stickies".
    pub announcement: SharedString,
    announced: Vec<(SharedString, Instant)>,
    next_ghost: u64,
    /// The canvas redraws every [`TICK`] while an agent shows, so they fade and go on time.
    pub ticking: bool,
}

/// How often the canvas redraws while an agent shows and nothing moves.
const TICK: Duration = Duration::from_millis(250);

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
    /// (unless motion is `reduce`d), and the layers it touched are outlined.
    pub fn point_at(&mut self, look: &Look, page: NodeId, target: Target, now: Instant) {
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
            point: (rect[0], rect[1]),
            rects: Vec::new(),
        };
        self.point_at(look, page, corner, now);
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
        self.ghosts.retain(|g| match &g.state {
            GhostState::Landed { doc: landed, .. } => !Arc::ptr_eq(landed, doc),
            _ => true,
        });
    }

    /// Says what `look`'s agent did, `done`, in the status line, unless that agent was announced
    /// less than [`ANNOUNCE_GAP`] ago.
    pub fn announce(&mut self, look: &Look, done: &str, now: Instant) {
        self.announced
            .retain(|(_, at)| now.saturating_duration_since(*at) < ANNOUNCE_GAP);
        if self.announced.iter().any(|(id, _)| *id == look.id) {
            return;
        }
        self.announced.push((look.id.clone(), now));
        self.announcement = crate::agents::sentence(look, done);
    }

    /// Lets go of what has had its time at `now`: agents quiet for 30 s, outlines that faded,
    /// placeholders whose result came (or never will) and error marks. Whether anything shows.
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
        !self.agents.is_empty() || !self.ghosts.is_empty()
    }

    /// Whether something moves at `now`, so the canvas draws the next frame too: a pointer on
    /// its way, an outline fading, a placeholder's shimmer.
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
    }

    /// Where `look`'s agent's pointer is going, on which page.
    #[cfg(test)]
    pub fn spot(&self, id: &str) -> Option<Spot> {
        self.agents.iter().find(|a| a.look.id == id)?.at
    }
}

#[cfg(test)]
mod tests;
