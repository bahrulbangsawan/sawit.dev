//! Block elements (U+2580–U+259F: `▀ ▄ █ ▌ ▐ ░ ▒ ▓ ▖ … ▟`) drawn as filled rectangles of the
//! cell, not as font glyphs, as most terminals do.
//!
//! A font's block glyphs only fill its own ascent and descent, so at the terminal's 1.3 line
//! height they leave a stripe between rows; and a font without them (Monaco has `█` but not
//! `▀ ▄ ▌ ▐`) shows a missing-glyph box. Either breaks what is drawn with them: the QR codes
//! Expo, `qrcode-terminal` and many CLIs print, progress bars, charts in TUIs.

use gpui_kit::*;

/// One filled part of a cell, in fractions of its width and height (left, top, right, bottom),
/// and how opaque (the shades `░ ▒ ▓` are the full cell at a quarter, half, three quarters).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Part {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub alpha: f32,
}

const fn part(x0: f32, y0: f32, x1: f32, y1: f32) -> Part {
    Part {
        x0,
        y0,
        x1,
        y1,
        alpha: 1.,
    }
}

const UPPER_LEFT: Part = part(0., 0., 0.5, 0.5);
const UPPER_RIGHT: Part = part(0.5, 0., 1., 0.5);
const LOWER_LEFT: Part = part(0., 0.5, 0.5, 1.);
const LOWER_RIGHT: Part = part(0.5, 0.5, 1., 1.);

/// The parts `ch` fills, when it is a block element.
pub(super) fn parts(ch: char) -> Option<Vec<Part>> {
    let c = ch as u32;
    if !(0x2580..=0x259F).contains(&c) {
        return None;
    }
    let eighth = |n: u32| n as f32 / 8.;
    Some(match c {
        // ▀ upper half.
        0x2580 => vec![part(0., 0., 1., 0.5)],
        // ▁ … ▇ lower one to seven eighths, █ full.
        0x2581..=0x2588 => vec![part(0., 1. - eighth(c - 0x2580), 1., 1.)],
        // ▉ … ▏ left seven eighths to one eighth.
        0x2589..=0x258F => vec![part(0., 0., eighth(8 - (c - 0x2588)), 1.)],
        // ▐ right half.
        0x2590 => vec![part(0.5, 0., 1., 1.)],
        // ░ ▒ ▓ shades.
        0x2591..=0x2593 => vec![Part {
            alpha: (c - 0x2590) as f32 / 4.,
            ..part(0., 0., 1., 1.)
        }],
        // ▔ upper eighth, ▕ right eighth.
        0x2594 => vec![part(0., 0., 1., eighth(1))],
        0x2595 => vec![part(eighth(7), 0., 1., 1.)],
        // Quadrants.
        0x2596 => vec![LOWER_LEFT],
        0x2597 => vec![LOWER_RIGHT],
        0x2598 => vec![UPPER_LEFT],
        0x2599 => vec![UPPER_LEFT, LOWER_LEFT, LOWER_RIGHT],
        0x259A => vec![UPPER_LEFT, LOWER_RIGHT],
        0x259B => vec![UPPER_LEFT, UPPER_RIGHT, LOWER_LEFT],
        0x259C => vec![UPPER_LEFT, UPPER_RIGHT, LOWER_RIGHT],
        0x259D => vec![UPPER_RIGHT],
        0x259E => vec![UPPER_RIGHT, LOWER_LEFT],
        _ => vec![UPPER_RIGHT, LOWER_LEFT, LOWER_RIGHT],
    })
}

/// A block cell of a row, ready to paint: its column, what it fills, in what colour.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Block {
    pub col: usize,
    pub parts: Vec<Part>,
    pub color: Hsla,
}

/// Paints `blocks` of the row whose top left is `at`, each edge on a whole device pixel so
/// neighbouring cells meet without a seam or an overlap.
pub(super) fn paint(blocks: &[Block], at: Point<Pixels>, cell: Size<Pixels>, window: &mut Window) {
    let scale = window.scale_factor();
    let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
    for block in blocks {
        let left = at.x + cell.width * block.col as f32;
        for p in &block.parts {
            let x0 = snap(left + cell.width * p.x0);
            let x1 = snap(left + cell.width * p.x1);
            let y0 = snap(at.y + cell.height * p.y0);
            let y1 = snap(at.y + cell.height * p.y1);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let bounds = Bounds::new(point(x0, y0), size(x1 - x0, y1 - y0));
            window.paint_quad(fill(bounds, block.color.opacity(p.alpha)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Part, parts};

    fn one(ch: char) -> Part {
        let p = parts(ch).unwrap();
        assert_eq!(p.len(), 1, "{ch}");
        p[0]
    }

    #[test]
    fn halves_eighths_and_shades() {
        let p = one('▀');
        assert_eq!((p.y0, p.y1, p.x0, p.x1), (0., 0.5, 0., 1.));
        let p = one('▄');
        assert_eq!((p.y0, p.y1), (0.5, 1.));
        let p = one('█');
        assert_eq!((p.x0, p.y0, p.x1, p.y1), (0., 0., 1., 1.));
        assert_eq!(one('▁').y0, 7. / 8.);
        assert_eq!(one('▌').x1, 0.5);
        assert_eq!(one('▏').x1, 1. / 8.);
        assert_eq!(one('▉').x1, 7. / 8.);
        assert_eq!(one('▐').x0, 0.5);
        assert_eq!(one('░').alpha, 0.25);
        assert_eq!(one('▓').alpha, 0.75);
        assert_eq!(parts('▟').unwrap().len(), 3);
        assert_eq!(parts('▚').unwrap().len(), 2);
        assert!(parts('a').is_none());
        assert!(parts('─').is_none());
    }

    #[test]
    fn every_block_element_fills_something() {
        for c in 0x2580..=0x259F_u32 {
            let ch = char::from_u32(c).unwrap();
            let ps = parts(ch).unwrap();
            assert!(!ps.is_empty(), "{ch}");
            for p in ps {
                assert!(p.x0 < p.x1 && p.y0 < p.y1 && p.alpha > 0., "{ch}");
            }
        }
    }
}
