//! What is painted over the canvas for the agents, worked out from where they are and what they
//! build: the layers a free op touched, outlined for a moment; each running work's skeleton and a
//! failed one's dashed frame; slow ops' placeholders; and the pointers, with the free pointers'
//! names ([`labels`](super::labels)), the most recent agent's on top.

use std::collections::HashSet;
use std::time::Instant;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_core::agents as model;
use sawit_core::agents::work::RowLook;

use super::labels::{self, Shaped};
use super::{GhostState, Presence, SHIMMER, TIP_FROM_CORNER, TOUCHED_TIME, Work};
use crate::board::BoardView;

/// One thing painted for the agents, from the canvas's top-left (px).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) enum Mark {
    /// A layer a free op just touched, outlined in its agent's colour.
    Outline {
        rect: [f64; 4],
        color: Hsla,
        alpha: f32,
    },
    /// A failed work's frame, dashed in the colour of failure.
    Failed { rect: [f64; 4] },
    /// A bar of a running work's skeleton, its shimmer at `phase` (none under reduced motion).
    Bar { rect: [f64; 4], phase: Option<f32> },
    /// A slow op's placeholder, its shimmer at `phase`, or its error mark once the op `failed`.
    Ghost {
        rect: [f64; 4],
        color: Hsla,
        phase: Option<f32>,
        failed: bool,
    },
    /// An agent's pointer.
    Cursor {
        tip: (f64, f64),
        color: Hsla,
        alpha: f32,
    },
    /// A free pointer's name, beside its tip.
    Pill {
        tip: (f64, f64),
        color: Hsla,
        label: SharedString,
        alpha: f32,
    },
}

/// The skeleton's padding inside its frame, and its bars' heights and gap (page units).
const SKELETON_PAD: f64 = 14.0;
const TITLE_BAR: f64 = 16.0;
const LINE_BAR: f64 = 10.0;
const BAR_GAP: f64 = 10.0;
const BUTTON_BAR: f64 = 26.0;

impl Presence {
    /// What to paint over page `page` of `doc` shown as `view` at `now`, from the canvas's
    /// top-left, back to front.
    pub fn marks(
        &self,
        doc: &Document,
        view: &ViewState,
        page: NodeId,
        pictured: &HashSet<NodeId>,
        now: Instant,
        reduce: bool,
    ) -> Vec<Mark> {
        let screen = |[x0, y0, x1, y1]: [f64; 4]| {
            let (a, b) = view.page_to_screen(x0, y0);
            let (c, d) = view.page_to_screen(x1, y1);
            [a, b, c, d]
        };
        let mut marks = Vec::new();
        for work in self.works.iter().filter(|w| w.page == page) {
            let Some(b) = doc.world_bounds(work.frame) else {
                continue;
            };
            let frame = [b.x0, b.y0, b.x1, b.y1];
            match work.row(now) {
                // A screen's skeleton gives way to its first picture.
                RowLook::Working { .. } if pictured.contains(&work.frame) => {}
                RowLook::Working { stalled } => {
                    let phase = work.shimmer(now, reduce || stalled);
                    // A routes work's section: a skeleton in each of its frames until it is
                    // pictured, once it has frames.
                    let mut parts = route_frames(doc, work.frame);
                    if parts.is_empty() {
                        parts.push((work.frame, frame));
                    }
                    for (part, rect) in parts.into_iter().filter(|(p, _)| !pictured.contains(p)) {
                        for bar in skeleton(doc, part, rect) {
                            marks.push(Mark::Bar {
                                rect: screen(bar),
                                phase,
                            });
                        }
                    }
                }
                RowLook::Failed => marks.push(Mark::Failed {
                    rect: screen(frame),
                }),
                RowLook::Done { .. } | RowLook::Gone => {}
            }
        }
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
        // The most recent agent last, on top.
        let mut agents: Vec<_> = self.agents.iter().collect();
        agents.sort_by_key(|a| a.seen);
        for agent in agents {
            let idle = now.saturating_duration_since(agent.seen);
            let (Some(alpha), Some(point)) = (model::opacity(idle), agent.point(now, reduce))
            else {
                continue;
            };
            if agent.at.is_none_or(|at| at.page != page) {
                continue;
            }
            let (x, y) = view.page_to_screen(point.0, point.1);
            let tip = (x + TIP_FROM_CORNER.0, y + TIP_FROM_CORNER.1);
            let color = agent.look.color;
            marks.push(Mark::Cursor { tip, color, alpha });
            let on_row = (agent.work.as_deref())
                .and_then(|id| self.work(id))
                .is_some_and(|work| work.page == page && names_agent(work, now));
            let hovered = self.hovered.as_ref() == Some(&agent.look.id);
            if !on_row && (agent.named(now) || hovered) {
                let label = agent.look.label.clone();
                marks.push(Mark::Pill {
                    tip,
                    color,
                    label,
                    alpha,
                });
            }
        }
        marks
    }

    /// The agent whose pointer, painted over page `page` shown as `view`, is under `point` (px
    /// from the canvas's top-left) at `now`: the most recent one, as it is on top.
    pub fn pointer_at(
        &self,
        view: &ViewState,
        page: NodeId,
        point: (f64, f64),
        now: Instant,
    ) -> Option<SharedString> {
        let mut agents: Vec<_> = self.agents.iter().collect();
        agents.sort_by_key(|a| std::cmp::Reverse(a.seen));
        agents.into_iter().find_map(|agent| {
            let at = agent.at.filter(|at| at.page == page)?;
            model::opacity(now.saturating_duration_since(agent.seen))?;
            let (x, y) = view.page_to_screen(at.point.0, at.point.1);
            let (dx, dy) = (
                point.0 - x - TIP_FROM_CORNER.0,
                point.1 - y - TIP_FROM_CORNER.1,
            );
            ((-3.0..=12.0).contains(&dx) && (-3.0..=16.0).contains(&dy))
                .then(|| agent.look.id.clone())
        })
    }
}

/// Whether the row of `work` names its agent at `now`: while it runs or failed, and for a moment
/// once it is done.
pub(super) fn names_agent(work: &Work, now: Instant) -> bool {
    matches!(
        work.row(now),
        RowLook::Working { .. } | RowLook::Failed | RowLook::Done { name: true, .. }
    )
}

/// The route frames in `section`, with their places on the page.
fn route_frames(doc: &Document, section: NodeId) -> Vec<(NodeId, [f64; 4])> {
    let children = doc
        .node(section)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    (children.into_iter())
        .filter(|id| {
            doc.node(*id)
                .and_then(sawit_board::routes::web_of)
                .is_some()
        })
        .filter_map(|id| {
            let b = doc.world_bounds(id)?;
            Some((id, [b.x0, b.y0, b.x1, b.y1]))
        })
        .collect()
}

/// The bars of the skeleton in `frame` (page `[x0, y0, x1, y1]`), a frame of `doc`: a title, three
/// lines and a button where they fit, each left out where what the agent drew already is.
pub(super) fn skeleton(doc: &Document, frame: NodeId, [x0, y0, x1, y1]: [f64; 4]) -> Vec<[f64; 4]> {
    let inner = (x1 - x0) - 2.0 * SKELETON_PAD;
    if inner <= 8.0 || (y1 - y0) <= 2.0 * SKELETON_PAD + TITLE_BAR {
        return Vec::new();
    }
    let (left, bottom) = (x0 + SKELETON_PAD, y1 - SKELETON_PAD);
    let mut bars = vec![[
        left,
        y0 + SKELETON_PAD,
        left + inner * 0.7,
        y0 + SKELETON_PAD + TITLE_BAR,
    ]];
    let mut top = bars[0][3] + BAR_GAP + 2.0;
    let button = if y1 - y0 > 140.0 {
        BUTTON_BAR + BAR_GAP
    } else {
        0.0
    };
    for width in [0.88, 0.74, 0.6] {
        if top + LINE_BAR > bottom - button {
            break;
        }
        bars.push([left, top, left + inner * width, top + LINE_BAR]);
        top += LINE_BAR + BAR_GAP;
    }
    if button > 0.0 {
        bars.push([
            left,
            bottom - BUTTON_BAR,
            left + (inner * 0.4).min(110.0),
            bottom,
        ]);
    }
    let drawn: Vec<[f64; 4]> = (doc.node(frame).map(|n| n.children.clone()))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|child| doc.world_bounds(child))
        .map(|b| [b.x0, b.y0, b.x1, b.y1])
        .collect();
    let meets =
        |a: &[f64; 4], b: &[f64; 4]| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
    bars.retain(|bar| !drawn.iter().any(|d| meets(bar, d)));
    bars
}

/// [`Mark`]s ready to paint: their names shaped, and the names that would cover another left out.
#[derive(Default)]
pub(in crate::board) struct Painted(pub Vec<(Mark, Option<Shaped>)>);

impl BoardView {
    /// The agents' marks to paint over page `page` of `doc` shown as `view`, when agents' cursors
    /// show; and the next frame asked for while something of them moves.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::board) fn agents_paint(
        &self,
        doc: &Document,
        view: &ViewState,
        page: NodeId,
        pictured: &HashSet<NodeId>,
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
        let marks = self.agents.marks(doc, view, page, pictured, now, reduce);
        labels::shape(marks, &cx.theme().font_family, window)
    }
}
