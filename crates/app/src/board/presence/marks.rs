//! What is painted over the canvas for the agents: the layers they just touched, outlined, their
//! slow ops' placeholders, and their pointers with a pill of their name and what they do.

use std::time::Instant;

use figcraft_doc::NodeId;
use figcraft_engine::ViewState;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_core::agents as model;

use super::{
    GhostState, PILL_AT, PILL_HEIGHT, PILL_PAD, PILL_TEXT, POINTER, Presence, SHIMMER, TOUCHED_TIME,
};
use crate::board::BoardView;

/// One thing painted for the agents, from the canvas's top-left (px).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::board) enum Mark {
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

impl Presence {
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
}

impl BoardView {
    /// The agents' marks to paint over page `page` shown as `view`, their pills shaped, when
    /// agents' cursors show; and the next frame asked for while something of them moves.
    pub(in crate::board) fn agents_paint(
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
}

/// [`Mark`]s ready to paint: their text shaped.
#[derive(Default)]
pub(in crate::board) struct Painted(Vec<(Mark, Option<Box<ShapedLine>>)>);

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
pub(in crate::board) fn paint(
    origin: Point<Pixels>,
    painted: Painted,
    window: &mut Window,
    cx: &mut App,
) {
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
