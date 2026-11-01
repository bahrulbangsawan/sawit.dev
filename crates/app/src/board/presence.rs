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
//! by comparing the document before and after the op ([`target`]). It glides there for [`GLIDE`],
//! eased out, and jumps under reduced motion.
//!
//! An op that takes noticeable time (`sawit_core::agents::slow_canvas_op`: a Mermaid layout, an
//! image or file brought in, a long `command.batch`) shows a placeholder at once where its result
//! will land ([`ghost_rect`]), and runs once that has been painted ([`PLACEHOLDER_LEAD`]). The
//! placeholder goes when the render thread hands over the frame with the result, or, if the op
//! failed, after a moment with an error mark.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::*;
use sawit_core::agents::{self as model, AgentIdentity, GONE_AFTER};
use serde::Deserialize as _;
use serde_json::Value;

use super::BoardView;
use crate::agents::Look;
use crate::workspace::Answer;

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

    /// What to paint over page `page` shown as `view` at `now`, from the canvas's top-left.
    pub fn marks(&self, view: &ViewState, page: NodeId, now: Instant, reduce: bool) -> Vec<Mark> {
        let screen = |[x0, y0, x1, y1]: [f64; 4]| {
            let (a, b) = view.page_to_screen(x0, y0);
            let (c, d) = view.page_to_screen(x1, y1);
            [a, b, c, d]
        };
        let mut marks = Vec::new();
        for agent in &self.agents {
            let since = now.saturating_duration_since(agent.touched_at);
            if agent.touched_page != Some(page) || since >= TOUCHED_TIME {
                continue;
            }
            // Whole for the first half, then fading out.
            let half = TOUCHED_TIME.as_secs_f32() / 2.0;
            let alpha = (1.0 - (since.as_secs_f32() - half).max(0.0) / half).clamp(0.0, 1.0);
            for rect in &agent.touched {
                marks.push(Mark::Outline {
                    rect: screen(*rect),
                    color: agent.look.color,
                    alpha,
                });
            }
        }
        for ghost in self.ghosts.iter().filter(|g| g.page == page) {
            let phase = (!reduce).then(|| {
                let into = now.saturating_duration_since(ghost.started).as_secs_f32();
                (into / SHIMMER.as_secs_f32()).fract()
            });
            marks.push(Mark::Ghost {
                rect: screen(ghost.rect),
                color: ghost.look.color,
                phase,
                failed: matches!(ghost.state, GhostState::Failed { .. }),
            });
        }
        for agent in &self.agents {
            let idle = now.saturating_duration_since(agent.seen);
            let (Some(alpha), Some(point)) = (model::opacity(idle), agent.point(now, reduce))
            else {
                continue;
            };
            if agent.at.is_none_or(|at| at.page != page) {
                continue;
            }
            let (x, y) = view.page_to_screen(point.0, point.1);
            marks.push(Mark::Cursor {
                tip: (x, y),
                color: agent.look.color,
                label: agent.look.label.clone(),
                doing: model::shows_action(idle).then(|| agent.doing.clone().into()),
                alpha,
            });
        }
        marks
    }

    /// The chips for the agents at work on page `page` whose pointer is off a canvas of `size`
    /// (px) shown as `view`: each kept just inside the edge nearest it, with the way to it.
    pub fn edges(
        &self,
        view: &ViewState,
        page: NodeId,
        size: (f64, f64),
        now: Instant,
    ) -> Vec<Edge> {
        let mut edges = Vec::new();
        for agent in &self.agents {
            let idle = now.saturating_duration_since(agent.seen);
            let (Some(at), Some(_)) = (agent.at, model::opacity(idle)) else {
                continue;
            };
            if at.page != page {
                continue;
            }
            let (x, y) = view.page_to_screen(at.point.0, at.point.1);
            if let Some(edge) = edge_chip((x, y), size) {
                edges.push(Edge {
                    look: agent.look.clone(),
                    at: edge.0,
                    toward: edge.1,
                    point: at.point,
                });
            }
        }
        edges
    }

    /// Where `look`'s agent's pointer is going, on which page.
    #[cfg(test)]
    pub fn spot(&self, id: &str) -> Option<Spot> {
        self.agents.iter().find(|a| a.look.id == id)?.at
    }
}

/// Which way an off-screen agent is from the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Toward {
    Left,
    Right,
    Up,
    Down,
}

/// An off-screen agent's chip at the canvas's edge.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Edge {
    pub look: Look,
    /// The chip's top-left from the canvas's top-left (px).
    pub at: (f64, f64),
    pub toward: Toward,
    /// Where the agent is on the page: the view goes there when the chip is clicked.
    pub point: (f64, f64),
}

/// Where the chip of an agent whose pointer is at `point` (px from the canvas's top-left) goes on
/// a canvas of `size`, and which way it points; `None` while the pointer is on the canvas.
fn edge_chip(point: (f64, f64), size: (f64, f64)) -> Option<((f64, f64), Toward)> {
    let (x, y) = point;
    let (w, h) = size;
    if !(x.is_finite() && y.is_finite()) || (0.0..=w).contains(&x) && (0.0..=h).contains(&y) {
        return None;
    }
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (dx, dy) = (x - cx, y - cy);
    // The edge it leaves by: the side the pointer is furthest past, for the canvas's shape.
    let toward = if dx.abs() * h >= dy.abs() * w {
        if dx < 0.0 {
            Toward::Left
        } else {
            Toward::Right
        }
    } else if dy < 0.0 {
        Toward::Up
    } else {
        Toward::Down
    };
    let clamp = |v: f64, lo: f64, hi: f64| v.max(lo).min(hi.max(lo));
    let left = clamp(
        x - EDGE_CHIP.0 / 2.0,
        EDGE_INSET,
        w - EDGE_INSET - EDGE_CHIP.0,
    );
    let top = clamp(
        y - EDGE_CHIP.1 / 2.0,
        EDGE_INSET,
        h - EDGE_INSET - EDGE_CHIP.1,
    );
    Some(((left, top), toward))
}

/// One thing painted for the agents, from the canvas's top-left (px).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Mark {
    /// A layer an agent just touched, outlined in its colour.
    Outline {
        rect: [f64; 4],
        color: Hsla,
        alpha: f32,
    },
    /// A slow op's placeholder, its shimmer at `phase` (none under reduced motion), or its error
    /// mark once the op `failed`.
    Ghost {
        rect: [f64; 4],
        color: Hsla,
        phase: Option<f32>,
        failed: bool,
    },
    /// An agent's pointer, with its pill.
    Cursor {
        tip: (f64, f64),
        color: Hsla,
        label: SharedString,
        doing: Option<SharedString>,
        alpha: f32,
    },
}

/// Where an agent's op landed on page `page`: what it added, changed or took off there, from the
/// document `before` and `after` it, each layer once: a sticky drawn with its text is the sticky.
/// Pages, and layers whose only change is what they hold (a frame given a new child), are left
/// out. `None` when nothing on the page changed.
pub(super) fn target(before: &Document, after: &Document, page: NodeId) -> Option<Target> {
    let on_page = |doc: &Document, id: NodeId| {
        doc.node(id).is_some_and(|n| !n.is_page()) && doc.page_of(id) == Some(page)
    };
    let mut added = Vec::new();
    let mut changed = Vec::new();
    for node in after.nodes() {
        if !on_page(after, node.id) {
            continue;
        }
        match before.node(node.id) {
            None => added.push(node.id),
            Some(old) if !std::ptr::eq(old, node) && !only_children_differ(old, node) => {
                changed.push(node.id);
            }
            Some(_) => {}
        }
    }
    let removed: Vec<NodeId> = before
        .nodes()
        .filter(|n| on_page(before, n.id) && after.node(n.id).is_none())
        .map(|n| n.id)
        .collect();
    // The outermost of what changed: a layer inside one added or changed with it is part of it.
    let all: Vec<NodeId> = added.iter().chain(&changed).copied().collect();
    let outermost = |id: &NodeId| {
        !all.iter()
            .any(|other| other != id && after.is_ancestor(*other, *id))
    };
    added.retain(outermost);
    changed.retain(outermost);
    let bounds = |doc: &Document, ids: &[NodeId]| -> Vec<[f64; 4]> {
        ids.iter()
            .filter_map(|id| doc.world_bounds(*id))
            .filter(|r| {
                r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
            })
            .map(|r| [r.x0, r.y0, r.x1, r.y1])
            .collect()
    };
    let new = bounds(after, &added);
    let mut touched: Vec<[f64; 4]> = new.iter().copied().chain(bounds(after, &changed)).collect();
    if touched.len() > MAX_OUTLINES {
        touched = union(&touched).into_iter().collect();
    }
    if let Some([x0, y0, _, _]) = union(&new) {
        return Some(Target {
            point: (x0, y0),
            rects: touched,
        });
    }
    let gone = bounds(before, &removed);
    let [x0, y0, x1, y1] = union(&touched).or_else(|| union(&gone))?;
    Some(Target {
        point: ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
        rects: touched,
    })
}

/// Whether `old` and `new` are one layer whose only change is the layers it holds.
fn only_children_differ(old: &figcraft_doc::Node, new: &figcraft_doc::Node) -> bool {
    if old.children == new.children {
        return false;
    }
    let (mut old, mut new) = (old.clone(), new.clone());
    old.children.clear();
    new.children.clear();
    old == new
}

/// Where an op landed: the point its pointer goes to, and the layers to outline.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    pub point: (f64, f64),
    pub rects: Vec<[f64; 4]>,
}

fn union(rects: &[[f64; 4]]) -> Option<[f64; 4]> {
    rects
        .iter()
        .copied()
        .reduce(|[a, b, c, d], [e, f, g, h]| [a.min(e), b.min(f), c.max(g), d.max(h)])
}

/// The page rectangle where slow op `op`'s result will land, with the view's centre at
/// `centre`: what an `add` will draw (`sawit_board::skeleton::bounds`), else a diagram's or
/// picture's corner (`x`, `y`) when the op names one, else `corner`, where the op puts it when it
/// names none (a diagram's), else the view's centre; as big as it will be when that is known,
/// else [`GHOST_SIZE`].
pub(super) fn ghost_rect(op: &Value, centre: (f64, f64), corner: Option<(f64, f64)>) -> [f64; 4] {
    let (w, h) = GHOST_SIZE;
    if op.get("op").and_then(Value::as_str) == Some("add") {
        let items: Vec<sawit_board::skeleton::Skeleton> = op
            .get("elements")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| sawit_board::skeleton::Skeleton::deserialize(item).ok())
                    .collect()
            })
            .unwrap_or_default();
        if let Some((x0, y0, x1, y1)) = sawit_board::skeleton::bounds(&items) {
            return [x0, y0, x1.max(x0 + 24.0), y1.max(y0 + 24.0)];
        }
    }
    let params = op.get("params").unwrap_or(op);
    let number = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
    };
    match number("x").zip(number("y")).or(corner) {
        Some((x, y)) => {
            let w = number("width").filter(|w| *w > 0.0).unwrap_or(w);
            let h = number("height").filter(|h| *h > 0.0).unwrap_or(h);
            [x, y, x + w, y + h]
        }
        None => {
            let (cx, cy) = centre;
            [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
        }
    }
}

impl BoardView {
    /// Runs agent `identity`'s op on the canvas, as [`Self::run_op`] runs it, and shows the agent
    /// there: its pointer at the op's target with what it does, and the layers it touched
    /// outlined. A slow op, on a canvas on screen with agents' cursors shown, shows a placeholder
    /// where its result will land at once, and runs once that has been painted: its answer comes
    /// later. So does the answer of an op that comes while one waits, which runs after it.
    pub fn agent_op(
        &mut self,
        op: Value,
        identity: Option<&AgentIdentity>,
        cx: &mut Context<Self>,
    ) -> Answer {
        let look = crate::agents::meet(identity, cx);
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
        self.keep_agents_ticking(cx);
        cx.notify();
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

    /// Runs `look`'s op `op`, and moves its pointer to where it landed, outlines what it touched
    /// and announces what it did; the placeholder `ghost`, if it has one, waits for the frame
    /// with the result, or shows its error mark.
    fn run_agent_op(
        &mut self,
        op: &Value,
        look: &Look,
        ghost: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Value {
        let action = model::canvas_action(op);
        let before = self.document();
        let reply = self.run_op(op, cx);
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
        if let (Some((before, after)), Some(page)) = (&changed, page) {
            if let Some(target) = target(before, after, page) {
                self.agents.point_at(look, page, target, now);
            }
            self.agents.announce(look, &action.done, now);
        }
        if let Some(ghost) = ghost {
            match changed.filter(|_| ok) {
                Some((_, after)) => self.agents.land(ghost, after, now),
                None => self.agents.fail(ghost, !ok, now),
            }
        }
        self.keep_agents_ticking(cx);
        cx.notify();
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

    /// Redraws the canvas every [`TICK`] while an agent shows on it, so its pointer fades and
    /// goes on time with nothing else drawing the canvas.
    fn keep_agents_ticking(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.agents.ticking, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let going = this.update(cx, |this, cx| {
                    let now = cx.background_executor().now();
                    let any = this.agents.prune(now);
                    this.agents.ticking = any;
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

    /// The agents' marks to paint over page `page` shown as `view`, their pills shaped, when
    /// agents' cursors show; and the next frame asked for while something of them moves.
    pub(super) fn agents_paint(
        &self,
        view: &ViewState,
        page: NodeId,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Painted {
        if !crate::agents::shown(cx) {
            return Painted::default();
        }
        let reduce = crate::agents::reduce_motion(cx);
        if self.agents.animating(now, reduce) {
            let view = cx.entity_id();
            window.on_next_frame(move |_, cx| cx.notify(view));
        }
        let marks = self.agents.marks(view, page, now, reduce);
        shape(marks, &cx.theme().font_family, window)
    }

    /// The render thread handed over the frame of request `key`: placeholders whose result it
    /// shows go.
    pub(super) fn agents_rendered(&mut self, key: u64) {
        if let Some((asked, doc)) = &self.requested
            && *asked == key
        {
            self.agents.rendered(doc);
        }
    }

    /// The agents at work on this canvas, as `app_state.agents` lists them.
    pub fn describe_agents(&self, cx: &App) -> Vec<Value> {
        let now = cx.background_executor().now();
        (self.agents.agents.iter())
            .filter(|a| now.saturating_duration_since(a.seen) < GONE_AFTER)
            .map(|a| {
                serde_json::json!({
                    "id": a.look.id,
                    "name": a.look.label,
                    "color": a.look.hex,
                    "surface": "canvas",
                    "file": self.path(),
                    "action": a.doing,
                    "seconds_ago": now.saturating_duration_since(a.seen).as_secs(),
                })
            })
            .collect()
    }

    /// Takes the view to `point` on the page, where an off-screen agent works: a click on its
    /// chip. The zoom stays as it is.
    fn go_to_agent(&mut self, point: (f64, f64), cx: &mut Context<Self>) {
        if let Some(view) = self.view_mut() {
            view.cx = point.0;
            view.cy = point.1;
            cx.notify();
        }
    }

    /// Over the canvas: a chip at its edge for each agent at work off screen, which takes the
    /// view there, and the polite status line that says what agents did.
    pub(super) fn render_agents(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let status = div()
            .id("board-agent-activity")
            .role(Role::Status)
            .aria_label(self.agents.announcement.clone())
            .absolute()
            .top_0()
            .left_0()
            .size(px(1.0))
            .overflow_hidden()
            .into_any_element();
        let mut elements = vec![status];
        if !crate::agents::shown(cx) {
            return elements;
        }
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return elements;
        };
        let size = (
            f64::from(self.bounds.size.width),
            f64::from(self.bounds.size.height),
        );
        let now = cx.background_executor().now();
        let this = cx.entity().downgrade();
        let edges = self.agents.edges(&ds.view, ds.page, size, now);
        for (ix, edge) in edges.into_iter().enumerate() {
            let (icon, way) = match edge.toward {
                Toward::Left => (Lucide::ArrowLeft, "to the left"),
                Toward::Right => (Lucide::ArrowRight, "to the right"),
                Toward::Up => (Lucide::ArrowUp, "above"),
                Toward::Down => (Lucide::ArrowDown, "below"),
            };
            let name: SharedString =
                format!("Go to {}, at work off screen {way}", edge.look.label).into();
            let this = this.clone();
            let point = edge.point;
            elements.push(
                div()
                    .id(("board-agent-edge", ix))
                    .role(Role::Button)
                    .aria_label(name.clone())
                    .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
                    .absolute()
                    .left(px(edge.at.0 as f32))
                    .top(px(edge.at.1 as f32))
                    .h(px(EDGE_CHIP.1 as f32))
                    .max_w(px(EDGE_CHIP.0 as f32))
                    .px_2()
                    .gap_1()
                    .flex()
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(white())
                    .bg(edge.look.color)
                    .text_color(white())
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .shadow_md()
                    .cursor_pointer()
                    .occlude()
                    .child(Icon::new(icon).size_3())
                    .child(div().min_w_0().truncate().child(edge.look.label.clone()))
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |view, cx| view.go_to_agent(point, cx));
                    })
                    .into_any_element(),
            );
        }
        elements
    }
}

/// [`Mark`]s ready to paint: their text shaped.
#[derive(Default)]
pub(super) struct Painted(Vec<(Mark, Option<Box<ShapedLine>>)>);

/// Shapes the pills' text of `marks` in the font `family`.
pub(super) fn shape(marks: Vec<Mark>, family: &SharedString, window: &Window) -> Painted {
    let painted = marks
        .into_iter()
        .map(|mark| {
            let line = match &mark {
                Mark::Cursor {
                    label,
                    doing,
                    alpha,
                    ..
                } => Some(Box::new(pill_text(
                    label,
                    doing.as_ref(),
                    *alpha,
                    family,
                    window,
                ))),
                _ => None,
            };
            (mark, line)
        })
        .collect();
    Painted(painted)
}

/// The pill's line: the agent's name in semibold, then what it does in regular, both white.
fn pill_text(
    label: &SharedString,
    doing: Option<&SharedString>,
    alpha: f32,
    family: &SharedString,
    window: &Window,
) -> ShapedLine {
    let name = label.to_string();
    let text: SharedString = match doing.filter(|d| !d.is_empty()) {
        Some(doing) => format!("{name}  {doing}").into(),
        None => name.clone().into(),
    };
    let mut bold = font(family.clone());
    bold.weight = FontWeight::SEMIBOLD;
    let regular = font(family.clone());
    let white = |a: f32| hsla(0.0, 0.0, 1.0, a * alpha);
    let run = |len: usize, font: Font, color: Hsla| TextRun {
        len,
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = vec![run(name.len(), bold, white(1.0))];
    if text.len() > name.len() {
        runs.push(run(text.len() - name.len(), regular, white(0.9)));
    }
    window
        .text_system()
        .shape_line(text, px(PILL_TEXT), &runs, None)
}

/// Paints `painted` over the canvas whose top-left is at `origin`, back to front: outlines, then
/// placeholders, then pointers.
pub(super) fn paint(origin: Point<Pixels>, painted: Painted, window: &mut Window, cx: &mut App) {
    let at = |(x, y): (f64, f64)| point(origin.x + px(x as f32), origin.y + px(y as f32));
    let rect_bounds = |[x0, y0, x1, y1]: [f64; 4]| {
        Bounds::from_corners(at((x0.min(x1), y0.min(y1))), at((x0.max(x1), y0.max(y1))))
    };
    let white = |a: f32| hsla(0.0, 0.0, 1.0, a);
    for (mark, line) in painted.0 {
        match mark {
            Mark::Outline { rect, color, alpha } => {
                let bounds = rect_bounds(rect);
                window.paint_quad(quad(
                    bounds.dilate(px(3.0)),
                    px(4.0),
                    transparent_black(),
                    px(1.0),
                    white(0.7 * alpha),
                    BorderStyle::Solid,
                ));
                window.paint_quad(quad(
                    bounds.dilate(px(2.0)),
                    px(3.0),
                    transparent_black(),
                    px(2.0),
                    color.opacity(alpha),
                    BorderStyle::Solid,
                ));
            }
            Mark::Ghost {
                rect,
                color,
                phase,
                failed,
            } => paint_ghost(rect_bounds(rect), color, phase, failed, window, cx),
            Mark::Cursor {
                tip, color, alpha, ..
            } => {
                let tip = at(tip);
                let points: Vec<Point<Pixels>> = POINTER
                    .iter()
                    .map(|(x, y)| point(tip.x + px(*x), tip.y + px(*y)))
                    .collect();
                let mut edge = PathBuilder::stroke(px(3.0));
                edge.add_polygon(&points, true);
                if let Ok(path) = edge.build() {
                    window.paint_path(path, white(alpha));
                }
                let mut body = PathBuilder::fill();
                body.add_polygon(&points, true);
                if let Ok(path) = body.build() {
                    window.paint_path(path, color.opacity(alpha));
                }
                let Some(line) = line else { continue };
                let size = size(line.width + px(PILL_PAD * 2.0), px(PILL_HEIGHT));
                let pill = Bounds::new(point(tip.x + px(PILL_AT.0), tip.y + px(PILL_AT.1)), size);
                window.paint_quad(quad(
                    pill,
                    px(PILL_HEIGHT / 2.0),
                    color.opacity(alpha),
                    px(1.5),
                    white(alpha),
                    BorderStyle::Solid,
                ));
                let text_at = point(pill.origin.x + px(PILL_PAD), pill.origin.y + px(2.0));
                if let Err(err) = line.paint(text_at, px(16.0), TextAlign::Left, None, window, cx) {
                    log::error!("an agent's name couldn't be painted: {err}");
                }
            }
        }
    }
}

/// A placeholder: a dashed frame in the agent's colour over a faint fill, with a lighter band
/// sweeping across it (`phase`) while the op works; a failed one keeps its frame solid in the
/// agent's colour with a "!" badge on its corner.
fn paint_ghost(
    bounds: Bounds<Pixels>,
    color: Hsla,
    phase: Option<f32>,
    failed: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let style = if failed {
        BorderStyle::Solid
    } else {
        BorderStyle::Dashed
    };
    window.paint_quad(quad(
        bounds,
        px(6.0),
        color.opacity(0.08),
        px(1.5),
        color.opacity(0.9),
        style,
    ));
    if let Some(phase) = phase.filter(|_| !failed) {
        let band = bounds.size.width * 0.35;
        let left = bounds.origin.x - band + (bounds.size.width + band) * phase;
        let sweep = Bounds::new(point(left, bounds.origin.y), size(band, bounds.size.height));
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(sweep, color.opacity(0.12)).corner_radii(px(6.0)));
        });
    }
    if failed {
        let badge = Bounds::centered_at(
            point(bounds.origin.x + bounds.size.width, bounds.origin.y),
            size(px(18.0), px(18.0)),
        );
        window.paint_quad(quad(
            badge,
            px(9.0),
            color,
            px(1.5),
            hsla(0.0, 0.0, 1.0, 1.0),
            BorderStyle::Solid,
        ));
        let runs = [TextRun {
            len: 1,
            font: Font {
                weight: FontWeight::BOLD,
                ..font(window.text_style().font_family.clone())
            },
            color: hsla(0.0, 0.0, 1.0, 1.0),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let mark = window
            .text_system()
            .shape_line("!".into(), px(12.0), &runs, None);
        let at = point(
            badge.origin.x + (badge.size.width - mark.width) / 2.0,
            badge.origin.y + px(1.0),
        );
        let _ = mark.paint(at, px(16.0), TextAlign::Left, None, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use figcraft_doc::{Document, NodeId};
    use figcraft_engine::ViewState;
    use futures::channel::oneshot;
    use gpui_kit::TestAppContext;
    use sawit_core::agents::{AgentIdentity, GONE_AFTER};
    use serde_json::{Value, json};

    use super::{
        ANNOUNCE_GAP, EDGE_CHIP, EDGE_INSET, FAILED_TIME, GLIDE, GhostState, Mark,
        PLACEHOLDER_LEAD, Presence, TICK, TOUCHED_TIME, Target, Toward, edge_chip, ghost_rect,
        target, union,
    };
    use crate::agents::Look;
    use crate::board::tests::{Scratch, next_frame, open_in_window};
    use crate::workspace::Answer;

    fn look(id: &str) -> Look {
        crate::agents::Look::for_tests(id)
    }

    fn codex() -> AgentIdentity {
        AgentIdentity {
            id: "mcp-1-2".into(),
            name: "Codex".into(),
            label: Some("shop".into()),
            version: None,
        }
    }

    /// The op's answer, once it has come.
    fn answered(answer: &mut oneshot::Receiver<Result<Value, String>>) -> Option<Value> {
        answer.try_recv().ok().flatten().and_then(Result::ok)
    }

    /// Runs the clock on in steps of the canvas's tick, as the app's time goes on.
    fn wait(cx: &mut TestAppContext, time: Duration) {
        let steps = (time.as_millis() / TICK.as_millis()).max(1) + 1;
        for _ in 0..steps {
            cx.executor().advance_clock(TICK);
            cx.run_until_parked();
        }
    }

    /// An agent's quick op answers at once, as it always has, and its pointer goes to what it
    /// drew, in its colour, with what it did said to screen readers and listed in app_state.
    #[gpui_kit::test]
    fn a_quick_op_answers_at_once_and_shows_the_agent_there(cx: &mut TestAppContext) {
        let tmp = Scratch::new("agent-quick-op");
        cx.update(gpui_kit::init);
        let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
        next_frame(cx, &view, None);
        let op = json!({"op": "add", "elements": [
            {"type": "sticky", "x": 40, "y": 60, "width": 100, "height": 100}]});
        let answer = view.update(cx, |view, cx| view.agent_op(op, Some(&codex()), cx));
        let Answer::Now(reply) = answer else {
            panic!("a quick op answers at once");
        };
        assert_eq!(reply["ok"], true, "{reply}");
        view.read_with(cx, |view, cx| {
            let agent = &view.agents.agents[0];
            assert_eq!(agent.look.label.as_ref(), "Codex");
            assert_eq!(agent.doing, "adding a sticky");
            assert_eq!(agent.at.map(|at| at.point), Some((40.0, 60.0)));
            assert_eq!(view.agents.announcement.as_ref(), "Codex added a sticky");
            let listed = view.describe_agents(cx);
            assert_eq!(listed[0]["surface"], "canvas");
            assert_eq!(listed[0]["action"], "adding a sticky");
        });
    }

    /// A slow op shows its placeholder at once, runs once that has been painted, and the
    /// placeholder goes with the frame of its result.
    #[gpui_kit::test]
    fn a_slow_ops_placeholder_shows_until_its_result_lands(cx: &mut TestAppContext) {
        let tmp = Scratch::new("agent-slow-op");
        cx.update(gpui_kit::init);
        let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
        let first = next_frame(cx, &view, None);
        let op = json!({"op": "mermaid", "source": "graph TD\nA-->B", "x": 0, "y": 0});
        let answer = view.update(cx, |view, cx| view.agent_op(op, Some(&codex()), cx));
        let Answer::Later(mut answer) = answer else {
            panic!("a slow op answers once it has run");
        };
        view.read_with(cx, |view, _| {
            assert_eq!(view.agents.ghosts.len(), 1, "the placeholder shows at once");
            assert!(matches!(view.agents.ghosts[0].state, GhostState::Waiting));
            assert_eq!(
                view.agents.queue.len(),
                1,
                "the op waits for it to be painted"
            );
        });
        cx.executor().advance_clock(PLACEHOLDER_LEAD);
        cx.run_until_parked();
        let reply = answered(&mut answer).expect("the op ran");
        assert_eq!(reply["ok"], true, "{reply}");
        // The frame of the diagram takes the placeholder away.
        let mut last = first;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while view.read_with(cx, |view, _| !view.agents.ghosts.is_empty()) {
            assert!(
                std::time::Instant::now() < deadline,
                "the placeholder stayed"
            );
            last = next_frame(cx, &view, Some(&last));
        }
    }

    /// A slow op that fails leaves an error mark where its placeholder was, for a moment, and
    /// says it couldn't finish.
    #[gpui_kit::test]
    fn a_failed_slow_ops_placeholder_shows_an_error_mark_then_goes(cx: &mut TestAppContext) {
        let tmp = Scratch::new("agent-failed-op");
        cx.update(gpui_kit::init);
        let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
        next_frame(cx, &view, None);
        let op = json!({"op": "mermaid", "source": "this is not a diagram ((("});
        let answer = view.update(cx, |view, cx| view.agent_op(op, Some(&codex()), cx));
        let Answer::Later(mut answer) = answer else {
            panic!("a slow op answers once it has run");
        };
        cx.executor().advance_clock(PLACEHOLDER_LEAD);
        cx.run_until_parked();
        let reply = answered(&mut answer).expect("the op ran");
        assert_eq!(reply["ok"], false, "{reply}");
        view.read_with(cx, |view, _| {
            assert!(matches!(
                view.agents.ghosts[0].state,
                GhostState::Failed { .. }
            ));
            assert_eq!(
                view.agents.agents[0].doing,
                "couldn't finish drawing a diagram"
            );
        });
        wait(cx, FAILED_TIME);
        view.read_with(cx, |view, _| assert!(view.agents.ghosts.is_empty()));
    }

    /// With agents' cursors turned off, a slow op runs at once, with no placeholder.
    #[gpui_kit::test]
    fn with_cursors_off_a_slow_op_runs_at_once(cx: &mut TestAppContext) {
        let tmp = Scratch::new("agent-cursors-off");
        cx.update(gpui_kit::init);
        let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
        next_frame(cx, &view, None);
        cx.update(crate::agents::toggle);
        let op = json!({"op": "mermaid", "source": "graph TD\nA-->B"});
        let answer = view.update(cx, |view, cx| view.agent_op(op, None, cx));
        assert!(matches!(answer, Answer::Now(_)));
        view.read_with(cx, |view, _| assert!(view.agents.ghosts.is_empty()));
    }

    /// A board with two stickies, and the same board after `op`, run as an agent's op runs.
    fn before_after(op: Value) -> (Arc<Document>, Arc<Document>, NodeId) {
        let mut board = sawit_board::BoardDoc::new_board("Board").unwrap();
        let add = json!({"op": "add", "elements": [
            {"type": "sticky", "id": "a", "x": 0, "y": 0, "width": 100, "height": 100},
            {"type": "sticky", "id": "b", "x": 400, "y": 0, "width": 100, "height": 100},
        ]});
        assert_eq!(sawit_board::ops::run(&mut board, &add)["ok"], true);
        let before = board.document_itself().unwrap();
        let reply = sawit_board::ops::run(&mut board, &op);
        assert_eq!(reply["ok"], true, "{reply}");
        let after = board.document_itself().unwrap();
        let page = board.session.ds().unwrap().page;
        (before, after, page)
    }

    /// An agent's pointer goes to the top-left corner of what it adds, and the new layers are
    /// outlined.
    #[test]
    fn the_pointer_goes_to_what_an_add_drew() {
        let (before, after, page) = before_after(json!({"op": "add", "elements": [
            {"type": "sticky", "x": 200, "y": 300, "width": 100, "height": 100},
            {"type": "sticky", "x": 320, "y": 300, "width": 100, "height": 100},
        ]}));
        let target = target(&before, &after, page).expect("a target");
        assert_eq!(target.point, (200.0, 300.0));
        assert_eq!(target.rects.len(), 2, "{:?}", target.rects);
    }

    /// Many layers at once (a diagram's) are outlined as one box around them all.
    #[test]
    fn many_layers_are_outlined_as_one_box() {
        let stickies: Vec<Value> = (0..12)
            .map(|i| json!({"type": "sticky", "x": 1000 + i * 120, "y": 0, "width": 100, "height": 100}))
            .collect();
        let (before, after, page) = before_after(json!({"op": "add", "elements": stickies}));
        let target = target(&before, &after, page).expect("a target");
        assert_eq!(target.point, (1000.0, 0.0));
        let [[x0, y0, x1, _]] = target.rects[..] else {
            panic!("one box: {:?}", target.rects);
        };
        assert_eq!((x0, y0), (1000.0, 0.0));
        assert!(x1 >= 1000.0 + 11.0 * 120.0 + 100.0 - 1.0, "{x1}");
    }

    /// For an update it goes to the centre of what changed: one layer's bounds, or, for several,
    /// the centre of them all, as a selection's.
    #[test]
    fn the_pointer_goes_to_the_centre_of_what_an_update_changed() {
        let (before, after, page) =
            before_after(json!({"op": "update", "elements": [{"id": "b", "x": 600, "y": 200}]}));
        let one = target(&before, &after, page).expect("a target");
        assert_eq!(one.rects.len(), 1);
        let [x0, y0, x1, y1] = one.rects[0];
        assert_eq!(one.point, ((x0 + x1) / 2.0, (y0 + y1) / 2.0));
        assert!(
            x0 >= 599.0 && y0 >= 199.0,
            "where it moved to: {:?}",
            one.rects[0]
        );

        let (before, after, page) = before_after(json!({"op": "update", "elements": [
            {"id": "a", "x": 0, "y": 100}, {"id": "b", "x": 400, "y": 100}]}));
        let both = target(&before, &after, page).expect("a target");
        assert_eq!(both.rects.len(), 2);
        let [x0, _, x1, _] = union(&both.rects).unwrap();
        assert_eq!(both.point.0, (x0 + x1) / 2.0);
    }

    /// A deletion leaves no layer to outline: the pointer goes where the layers were.
    #[test]
    fn the_pointer_goes_where_a_deletion_took_layers_off() {
        let (before, after, page) = before_after(json!({"op": "delete", "ids": ["b"]}));
        let target = target(&before, &after, page).expect("a target");
        assert!(target.rects.is_empty());
        assert!(target.point.0 > 400.0, "{:?}", target.point);
        assert_eq!(
            super::target(&before, &before, page),
            None,
            "nothing changed"
        );
    }

    /// The pointer glides from where it was to its new place, eased out, and jumps there under
    /// reduced motion; the first time, it shows where it is.
    #[test]
    fn pointers_glide_between_targets() {
        let mut presence = Presence::default();
        let page = NodeId(1);
        let t0 = Instant::now();
        let a = look("a");
        let at = |point| Target {
            point,
            rects: vec![],
        };
        presence.saw(&a, "adding a sticky", t0);
        presence.point_at(&a, page, at((0.0, 0.0)), t0);
        assert_eq!(presence.agents[0].point(t0, false), Some((0.0, 0.0)));
        let t1 = t0 + Duration::from_secs(1);
        presence.point_at(&a, page, at((100.0, 0.0)), t1);
        let half = presence.agents[0].point(t1 + GLIDE / 2, false).unwrap();
        assert!(half.0 > 50.0 && half.0 < 100.0, "eased out: {half:?}");
        assert_eq!(
            presence.agents[0].point(t1 + GLIDE, false),
            Some((100.0, 0.0))
        );
        assert_eq!(presence.agents[0].point(t1, true), Some((100.0, 0.0)));
        assert!(presence.animating(t1, false));
        assert!(!presence.animating(t1 + TOUCHED_TIME, false));
    }

    /// An agent quiet for 30 s goes, and so does its pointer.
    #[test]
    fn presence_expires() {
        let mut presence = Presence::default();
        let t0 = Instant::now();
        presence.saw(&look("a"), "adding a sticky", t0);
        assert!(presence.prune(t0 + Duration::from_secs(29)));
        assert!(!presence.prune(t0 + GONE_AFTER));
        assert!(presence.agents.is_empty());
    }

    /// A placeholder shows from when its op comes; it goes with the frame of the document its op
    /// made, or after a moment if that never comes, and a failed op's shows its error mark for a
    /// moment.
    #[test]
    fn placeholders_land_or_fail() {
        let mut presence = Presence::default();
        let page = NodeId(1);
        let t0 = Instant::now();
        let a = look("a");
        let ghost = presence.add_ghost(&a, page, [10.0, 20.0, 330.0, 220.0], t0);
        assert_eq!(presence.spot("a").map(|s| s.point), Some((10.0, 20.0)));
        let view = ViewState {
            vw: 800.0,
            vh: 600.0,
            ..ViewState::default()
        };
        let ghosts = |p: &Presence| {
            p.marks(&view, page, t0, false)
                .into_iter()
                .filter(|m| matches!(m, Mark::Ghost { .. }))
                .count()
        };
        assert_eq!(ghosts(&presence), 1);
        assert!(presence.animating(t0, false), "it shimmers");
        assert!(
            !presence.animating(t0 + GLIDE, true),
            "not under reduced motion"
        );
        let doc = Arc::new(Document::default());
        presence.land(ghost, doc.clone(), t0);
        assert!(presence.prune(t0 + Duration::from_millis(100)));
        assert_eq!(ghosts(&presence), 1, "until its frame comes");
        presence.rendered(&Arc::new(Document::default()));
        assert_eq!(ghosts(&presence), 1, "another document's frame");
        presence.rendered(&doc);
        assert_eq!(ghosts(&presence), 0);

        let failed = presence.add_ghost(&a, page, [0.0, 0.0, 10.0, 10.0], t0);
        presence.fail(failed, true, t0);
        assert!(
            presence
                .marks(&view, page, t0, false)
                .iter()
                .any(|m| matches!(m, Mark::Ghost { failed: true, .. }))
        );
        presence.prune(t0 + FAILED_TIME);
        assert_eq!(ghosts(&presence), 0, "the error mark goes after a moment");
    }

    /// An agent at work off screen gets a chip just inside the edge nearest it, pointing its way.
    #[test]
    fn off_screen_agents_get_a_chip_at_the_edge() {
        let size = (800.0, 600.0);
        assert_eq!(edge_chip((400.0, 300.0), size), None, "on screen");
        let (at, toward) = edge_chip((2000.0, 300.0), size).unwrap();
        assert_eq!(toward, Toward::Right);
        assert_eq!(
            at,
            (800.0 - EDGE_INSET - EDGE_CHIP.0, 300.0 - EDGE_CHIP.1 / 2.0)
        );
        let (at, toward) = edge_chip((400.0, -900.0), size).unwrap();
        assert_eq!((toward, at.1), (Toward::Up, EDGE_INSET));
        assert_eq!(edge_chip((-50.0, 10.0), size).unwrap().1, Toward::Left);
    }

    /// The same agent is announced once in a while, two agents each in turn.
    #[test]
    fn announcements_are_polite() {
        let mut presence = Presence::default();
        let t0 = Instant::now();
        presence.announce(&look("a"), "added a sticky", t0);
        assert_eq!(presence.announcement.as_ref(), "Agent a added a sticky");
        presence.announce(&look("a"), "moved a layer", t0 + Duration::from_millis(500));
        assert_eq!(presence.announcement.as_ref(), "Agent a added a sticky");
        presence.announce(
            &look("b"),
            "drew a diagram",
            t0 + Duration::from_millis(600),
        );
        assert_eq!(presence.announcement.as_ref(), "Agent b drew a diagram");
        presence.announce(&look("a"), "moved a layer", t0 + ANNOUNCE_GAP);
        assert_eq!(presence.announcement.as_ref(), "Agent a moved a layer");
    }

    /// A slow op's placeholder covers what it will draw, else the corner it names, else the
    /// middle of the view.
    #[test]
    fn placeholders_go_where_the_result_will() {
        let add = json!({"op": "add", "elements": [
            {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50}]});
        assert_eq!(
            ghost_rect(&add, (0.0, 0.0), None),
            [10.0, 20.0, 110.0, 70.0]
        );
        let mermaid = json!({"op": "mermaid", "source": "graph TD", "x": 500, "y": 40});
        let beside = Some((900.0, 0.0));
        assert_eq!(
            ghost_rect(&mermaid, (0.0, 0.0), beside),
            [500.0, 40.0, 820.0, 240.0]
        );
        let anywhere = json!({"op": "mermaid", "source": "graph TD"});
        assert_eq!(
            ghost_rect(&anywhere, (0.0, 0.0), beside),
            [900.0, 0.0, 1220.0, 200.0]
        );
        let image = json!({"op": "command", "command": "image.place",
            "params": {"path": "/a.png", "x": 1, "y": 2, "width": 30, "height": 40}});
        assert_eq!(ghost_rect(&image, (0.0, 0.0), None), [1.0, 2.0, 31.0, 42.0]);
        let batch = json!({"op": "command", "command": "command.batch", "params": {"steps": []}});
        assert_eq!(
            ghost_rect(&batch, (100.0, 100.0), None),
            [-60.0, 0.0, 260.0, 200.0]
        );
    }
}
