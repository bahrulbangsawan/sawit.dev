//! The design checks' tests: a screen's HTML ([`html`]), an artboard's layers ([`layers`]), and
//! the contrast formula, which is `scripts/contrast.py`'s.

mod html;
mod layers;

use super::{Rgba, contrast, on_scale, parse_color};

/// The ratios `python3 -I scripts/contrast.py` prints for the same pairs.
#[test]
fn contrast_is_the_scripts() {
    let pair = |fg: &str, bg: &str| contrast(parse_color(fg).unwrap(), parse_color(bg).unwrap());
    // white on Tailwind blue-500: 3.68:1 (col.15).
    assert!((pair("#ffffff", "#3b82f6") - 3.678).abs() < 0.001);
    assert!((pair("#000", "#fff") - 21.0).abs() < 1e-9);
    assert!((pair("#777777", "#ffffff") - 4.478).abs() < 0.001);
    // A translucent foreground is composited over its background first.
    let half = Rgba {
        a: 0.5,
        ..Rgba::BLACK
    };
    assert!((contrast(half, Rgba::WHITE) - pair("#808080", "#fff")).abs() < 0.05);
    assert_eq!(parse_color("rgb(0 0 0 / 50%)").unwrap().a, 0.5);
    assert_eq!(parse_color("hsl(0, 0%, 100%)").unwrap(), Rgba::WHITE);
}

#[test]
fn the_spacing_scale_is_fours() {
    for px in [0.0, 1.0, 2.0, 4.0, 8.0, 12.0, 24.0, 128.0, -16.0] {
        assert!(on_scale(px), "{px}");
    }
    for px in [3.0, 7.0, 11.0, 13.0, 31.0, 6.0] {
        assert!(!on_scale(px), "{px}");
    }
}
