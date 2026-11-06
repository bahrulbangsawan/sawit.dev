//! What each element task paints while it plays (30d), in its agent's colour, as overlay marks:
//! a box dragged out corner to corner with its live `W × H` badge, a caret typing, the pen's
//! rubber band, a copy ⌥-dragged with its distance, a layer moved or resized, one taken off, an
//! image wiping in, a component's outline and badge, and an auto layout frame's spacing.

use std::time::Instant;

use gpui_kit::{Hsla, SharedString, rgb};

use super::script::Kind;
use super::{Lane, Timed};
use crate::board::presence::marks::Mark;
use crate::board::presence::pointer::Tool;

/// The colours of components, spacing marks, distances and placeholders.
const COMPONENT: u32 = 0x7c3aed;
const SPACING: u32 = 0xec4899;
const DISTANCE: u32 = 0xe5484d;
const PLACEHOLDER: u32 = 0xd8dbe0;

/// Eased in and out.
fn ease(p: f64) -> f64 {
    p * p * (3.0 - 2.0 * p)
}

fn lerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// The rect a box dragged out to `p` covers: from its top-left corner to the pointer.
fn dragged([x0, y0, x1, y1]: [f64; 4], p: f64) -> [f64; 4] {
    let e = ease(p);
    [x0, y0, x0 + (x1 - x0) * e, y0 + (y1 - y0) * e]
}

/// Where the pointer is while task `t` is `p` into its play (page point).
pub(in crate::board) fn pointer_point(t: &Timed, p: f64) -> (f64, f64) {
    let r = t.task.rect;
    match &t.task.kind {
        Kind::Text { .. } => (r[0] + (r[2] - r[0]) * p, r[1] + (r[3] - r[1]) / 2.0),
        Kind::Line => (
            r[0] + (r[2] - r[0]) * ease(p),
            r[1] + (r[3] - r[1]) * ease(p),
        ),
        Kind::Duplicate { from } | Kind::Update { from } => {
            let at = lerp(*from, r, ease(p));
            (at[0], at[1])
        }
        Kind::Delete => ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0),
        Kind::Image => (r[0], r[1]),
        _ => {
            let d = dragged(r, p);
            (d[2], d[3])
        }
    }
}

/// The pointer a task shows.
pub(in crate::board) fn tool_of(kind: &Kind) -> Tool {
    match kind {
        Kind::Text { .. } => Tool::IBeam,
        Kind::Line => Tool::Pen,
        Kind::Duplicate { .. } => Tool::Duplicate,
        Kind::Update { .. } => Tool::Hand,
        Kind::Delete | Kind::Image => Tool::Arrow,
        _ => Tool::Crosshair,
    }
}

/// The marks of `lane`'s tasks playing at `now`; `screen` turns a page rect into the canvas's px
/// and `scale` is the zoom, so badges and lines keep their screen size.
pub(in crate::board) fn lane_marks(
    lane: &Lane,
    now: Instant,
    screen: &dyn Fn([f64; 4]) -> [f64; 4],
    marks: &mut Vec<Mark>,
) {
    let color = lane.look.color;
    for t in &lane.tasks {
        let Some(p) = t.progress(now).filter(|p| *p < 1.0) else {
            continue;
        };
        task_marks(t, p, color, screen, marks);
    }
}

fn task_marks(
    t: &Timed,
    p: f64,
    color: Hsla,
    screen: &dyn Fn([f64; 4]) -> [f64; 4],
    marks: &mut Vec<Mark>,
) {
    let r = t.task.rect;
    let size = |rect: [f64; 4]| -> SharedString {
        format!(
            "{} × {}",
            (rect[2] - rect[0]).abs().round(),
            (rect[3] - rect[1]).abs().round()
        )
        .into()
    };
    match &t.task.kind {
        Kind::Frame | Kind::Shape | Kind::Sticky | Kind::Component { .. } => {
            let d = dragged(r, p);
            let edge: Hsla = match t.task.kind {
                Kind::Component { .. } => rgb(COMPONENT).into(),
                _ => color,
            };
            marks.push(Mark::Box {
                rect: screen(d),
                color: edge,
                alpha: 1.0,
                dashed: false,
                fill: 0.06,
            });
            let s = screen(d);
            marks.push(Mark::Badge {
                at: (s[2] + 6.0, s[3] + 6.0),
                text: size(d),
                color: edge,
            });
            if let Kind::Component { instance: true } = t.task.kind {
                marks.push(Mark::Badge {
                    at: (s[0], s[1] - 20.0),
                    text: "◇".into(),
                    color: rgb(COMPONENT).into(),
                });
            }
            if let Some((pad, gap)) = t.task.spacing.filter(|_| p > 0.6) {
                let [x0, y0, x1, y1] = r;
                let pink: Hsla = rgb(SPACING).into();
                for band in [
                    [x0, y0, x1, y0 + pad[0]],
                    [x1 - pad[1], y0, x1, y1],
                    [x0, y1 - pad[2], x1, y1],
                    [x0, y0, x0 + pad[3], y1],
                ] {
                    marks.push(Mark::Box {
                        rect: screen(band),
                        color: pink,
                        alpha: 0.0,
                        dashed: false,
                        fill: 0.22,
                    });
                }
                if gap > 0.0 {
                    marks.push(Mark::Badge {
                        at: (s[0] + 4.0, s[1] + 4.0),
                        text: format!("gap {gap}").into(),
                        color: pink,
                    });
                }
            }
        }
        Kind::Text { retype, .. } => {
            if *retype && p < 0.2 {
                marks.push(Mark::Box {
                    rect: screen(r),
                    color,
                    alpha: 0.0,
                    dashed: false,
                    fill: 0.25,
                });
            }
            let s = screen(r);
            let x = s[0] + (s[2] - s[0]) * p;
            let line = (s[3] - s[1]).clamp(10.0, 28.0);
            marks.push(Mark::Caret {
                at: (x, s[1]),
                height: line,
                color,
            });
        }
        Kind::Image => {
            let wiped = r[1] + (r[3] - r[1]) * ease(p);
            marks.push(Mark::Box {
                rect: screen([r[0], wiped, r[2], r[3]]),
                color: rgb(PLACEHOLDER).into(),
                alpha: 0.0,
                dashed: false,
                fill: 1.0,
            });
        }
        Kind::Line => {
            let s = screen(r);
            let e = ease(p);
            let to = (s[0] + (s[2] - s[0]) * e, s[1] + (s[3] - s[1]) * e);
            marks.push(Mark::Line {
                from: (s[0], s[1]),
                to,
                color,
                width: 2.0,
            });
            for anchor in [(s[0], s[1]), to] {
                marks.push(Mark::Box {
                    rect: [
                        anchor.0 - 3.0,
                        anchor.1 - 3.0,
                        anchor.0 + 3.0,
                        anchor.1 + 3.0,
                    ],
                    color,
                    alpha: 1.0,
                    dashed: false,
                    fill: 1.0,
                });
            }
        }
        Kind::Duplicate { from } => {
            let at = lerp(*from, r, ease(p));
            marks.push(Mark::Box {
                rect: screen(*from),
                color,
                alpha: 0.8,
                dashed: false,
                fill: 0.0,
            });
            marks.push(Mark::Box {
                rect: screen(at),
                color,
                alpha: 1.0,
                dashed: true,
                fill: 0.08,
            });
            let (a, b) = (screen(*from), screen(at));
            let red: Hsla = rgb(DISTANCE).into();
            let (from_x, to_x, y) = (a[2], b[0], (b[1] + b[3]) / 2.0);
            if to_x > from_x {
                marks.push(Mark::Line {
                    from: (from_x, y),
                    to: (to_x, y),
                    color: red,
                    width: 1.0,
                });
                let gap = (at[0] - from[2]).round();
                marks.push(Mark::Badge {
                    at: ((from_x + to_x) / 2.0, y + 6.0),
                    text: format!("{gap}").into(),
                    color: red,
                });
            }
        }
        Kind::Update { from } => {
            let at = lerp(*from, r, ease(p));
            marks.push(Mark::Box {
                rect: screen(at),
                color,
                alpha: 1.0,
                dashed: false,
                fill: 0.0,
            });
        }
        Kind::Delete => {
            let shrink = 0.15 * p;
            let (w, h) = (r[2] - r[0], r[3] - r[1]);
            let at = [
                r[0] + w * shrink,
                r[1] + h * shrink,
                r[2] - w * shrink,
                r[3] - h * shrink,
            ];
            marks.push(Mark::Box {
                rect: screen(at),
                color,
                alpha: (1.0 - p) as f32,
                dashed: false,
                fill: 0.15 * (1.0 - p) as f32,
            });
        }
    }
}
