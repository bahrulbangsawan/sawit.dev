//! A free pointer's name: a pill in the agent's colour beside the pointer's tip, the name only, in
//! 11 px semibold white, at most [`PILL_MAX`] wide with an ellipsis. A pill that would cover the
//! pill of an agent at work more recently is left out, and its pointer shows alone.

use gpui_kit::*;

use super::marks::{Mark, Painted};

/// Where the pill goes from the pointer's tip (px), and its height, padding and text size.
pub(super) const PILL_AT: (f32, f32) = (11.0, 5.0);
pub(super) const PILL_HEIGHT: f32 = 18.0;
pub(super) const PILL_PAD: f32 = 7.0;
const PILL_TEXT: f32 = 11.0;
/// The widest a pill may be (px).
pub(super) const PILL_MAX: f32 = 120.0;

/// A pill's name, shaped to fit.
pub(in crate::board) type Shaped = Box<ShapedLine>;

/// The pill of the pointer whose tip is at `tip`, for a name `width` wide: `[x0, y0, x1, y1]`.
pub(super) fn pill_rect(tip: (f64, f64), width: f32) -> [f64; 4] {
    let x0 = tip.0 + f64::from(PILL_AT.0);
    let y0 = tip.1 + f64::from(PILL_AT.1);
    let w = (width + PILL_PAD * 2.0).min(PILL_MAX);
    [x0, y0, x0 + f64::from(w), y0 + f64::from(PILL_HEIGHT)]
}

/// Which of the pills `rects`, in painting order (the most recent agent's last), show: from the
/// most recent back, each that covers none of those kept.
pub(super) fn kept(rects: &[[f64; 4]]) -> Vec<bool> {
    let meets =
        |a: &[f64; 4], b: &[f64; 4]| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
    let mut keep = vec![false; rects.len()];
    let mut placed: Vec<[f64; 4]> = Vec::new();
    for (ix, rect) in rects.iter().enumerate().rev() {
        if !placed.iter().any(|p| meets(p, rect)) {
            placed.push(*rect);
            keep[ix] = true;
        }
    }
    keep
}

/// Shapes the pills' names of `marks` in the font `family`, and leaves out the pills that would
/// cover a more recent agent's.
pub(super) fn shape(marks: Vec<Mark>, family: &SharedString, window: &Window) -> Painted {
    let shaped: Vec<(Mark, Option<Shaped>)> = marks
        .into_iter()
        .map(|mark| {
            let line = match &mark {
                Mark::Pill { label, alpha, .. } => {
                    Some(Box::new(name(label, *alpha, family, window)))
                }
                _ => None,
            };
            (mark, line)
        })
        .collect();
    let rects: Vec<(usize, [f64; 4])> = (shaped.iter().enumerate())
        .filter_map(|(ix, (mark, line))| match (mark, line) {
            (Mark::Pill { tip, .. }, Some(line)) => Some((ix, pill_rect(*tip, line.width.into()))),
            _ => None,
        })
        .collect();
    let keep = kept(&rects.iter().map(|(_, r)| *r).collect::<Vec<_>>());
    let dropped: Vec<usize> = (rects.iter().zip(keep))
        .filter(|(_, keep)| !keep)
        .map(|((ix, _), _)| *ix)
        .collect();
    let painted = (shaped.into_iter().enumerate())
        .filter(|(ix, _)| !dropped.contains(ix))
        .map(|(_, mark)| mark)
        .collect();
    Painted(painted)
}

/// An agent's name in semibold white, cut with an ellipsis to fit a pill of [`PILL_MAX`].
fn name(label: &SharedString, alpha: f32, family: &SharedString, window: &Window) -> ShapedLine {
    let mut bold = font(family.clone());
    bold.weight = FontWeight::SEMIBOLD;
    let shape = |text: SharedString| {
        let runs = [TextRun {
            len: text.len(),
            font: bold.clone(),
            color: hsla(0.0, 0.0, 1.0, alpha),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        window
            .text_system()
            .shape_line(text, px(PILL_TEXT), &runs, None)
    };
    let room = px(PILL_MAX - PILL_PAD * 2.0);
    let whole = shape(label.clone());
    if whole.width <= room {
        return whole;
    }
    let chars: Vec<char> = label.chars().collect();
    for keep in (1..chars.len()).rev() {
        let cut: String = chars[..keep]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string()
            + "…";
        let line = shape(cut.into());
        if line.width <= room {
            return line;
        }
    }
    whole
}
