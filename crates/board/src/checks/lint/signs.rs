//! The signs of generic generated design the lint looks for: a purple-to-blue gradient, emoji as
//! icons.

/// Whether a gradient runs from purple to blue (either way): hues near 270° and near 220°.
pub(super) fn purple_to_blue(g: &figcraft_color::Gradient) -> bool {
    let hues: Vec<f64> = g.stops.iter().filter_map(|s| hue(&s.color)).collect();
    let purple = hues.iter().any(|h| (255.0..=300.0).contains(h));
    let blue = hues.iter().any(|h| (190.0..245.0).contains(h));
    purple && blue
}

/// The hue of `c` in degrees, or `None` for a grey.
fn hue(c: &figcraft_color::Color) -> Option<f64> {
    let (r, g, b) = (f64::from(c.r), f64::from(c.g), f64::from(c.b));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d < 0.08 {
        return None;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    Some(h * 60.0)
}

/// Whether `text` is one or two emoji and nothing else.
pub(super) fn is_emoji(text: &str) -> bool {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let pictures = chars
        .iter()
        .filter(|c| {
            let u = **c as u32;
            (0x1F300..=0x1FAFF).contains(&u) || (0x2600..=0x27BF).contains(&u)
        })
        .count();
    let joiners = chars
        .iter()
        .filter(|c| matches!(**c as u32, 0x200D | 0xFE0F | 0x1F3FB..=0x1F3FF))
        .count();
    (1..=2).contains(&pictures) && pictures + joiners == chars.len()
}
