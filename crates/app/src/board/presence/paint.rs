//! Painting the agents' marks over the canvas ([`Mark`]), back to front.

use gpui_kit::*;

use super::POINTER;
use super::labels::{PILL_HEIGHT, PILL_PAD, pill_rect};
use super::marks::{Mark, Painted};

/// The colour of failure: a failed work's frame, its "!" and its reason.
pub(super) const FAILED: u32 = 0xb42318;
/// A skeleton bar's colour, and its shimmer's.
const BAR: u32 = 0xe7e9ed;
const BAR_SHINE: u32 = 0xf4f5f7;

/// Paints `painted` over the canvas whose top-left is at `origin`, back to front: skeletons and
/// outlines, then placeholders, then pointers with their names, the most recent agent's last.
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
            Mark::Failed { rect } => window.paint_quad(quad(
                rect_bounds(rect).dilate(px(1.0)),
                px(3.0),
                transparent_black(),
                px(1.5),
                rgb(FAILED),
                BorderStyle::Dashed,
            )),
            Mark::Bar { rect, phase } => paint_bar(rect_bounds(rect), phase, window),
            Mark::Ghost {
                rect,
                color,
                phase,
                failed,
            } => paint_ghost(rect_bounds(rect), color, phase, failed, window, cx),
            Mark::Cursor { tip, color, alpha } => {
                let tip = at(tip);
                let points: Vec<Point<Pixels>> = POINTER
                    .iter()
                    .map(|(x, y)| point(tip.x + px(*x), tip.y + px(*y)))
                    .collect();
                // A 3 px stroke under the fill leaves a 1.5 px white edge around it.
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
            }
            Mark::Pill {
                tip, color, alpha, ..
            } => {
                let Some(line) = line else { continue };
                let pill = rect_bounds(pill_rect(tip, f32::from(line.width)));
                window.paint_quad(
                    fill(pill, color.opacity(alpha)).corner_radii(px(PILL_HEIGHT / 2.0)),
                );
                let text_at = point(pill.origin.x + px(PILL_PAD), pill.origin.y + px(1.0));
                let clip = ContentMask { bounds: pill };
                window.with_content_mask(Some(clip), |window| {
                    if let Err(err) =
                        line.paint(text_at, px(16.0), TextAlign::Left, None, window, cx)
                    {
                        log::error!("an agent's name couldn't be painted: {err}");
                    }
                });
            }
        }
    }
}

/// A skeleton bar: flat grey, with a lighter band sweeping across it at `phase` while its work
/// runs (none under reduced motion).
fn paint_bar(bounds: Bounds<Pixels>, phase: Option<f32>, window: &mut Window) {
    let radius = (bounds.size.height / 2.0).min(px(4.0));
    window.paint_quad(fill(bounds, rgb(BAR)).corner_radii(radius));
    if let Some(phase) = phase {
        // The band runs over a strip three bars wide, so that bars side by side shine in turn.
        let band = bounds.size.width * 0.45;
        let left = bounds.origin.x - band + (bounds.size.width + band * 2.0) * phase;
        let sweep = Bounds::new(point(left, bounds.origin.y), size(band, bounds.size.height));
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(sweep, rgb(BAR_SHINE)).corner_radii(radius));
        });
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
            rgb(FAILED),
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
