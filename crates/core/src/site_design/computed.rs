//! Colours as WebKit computes them, read the way the page's design module read them before it
//! moved here: CSS colour text → sRGB `[r, g, b, a]` (0–255, alpha 0–1), OKLCH chroma, whether
//! a colour is transparent, and the opaque background behind an element.
//!
//! The page sends the colour text it reads (`getComputedStyle`); for a syntax [`read`] doesn't
//! know it adds the browser's own reading (a detached canvas's `fillStyle`), in
//! [`Fallbacks`]. Every number follows the JavaScript it replaces, edge cases included (a
//! `grad` hue reads as radians, `color(srgb 1 0)` keeps a NaN blue), so the design scan's
//! answers stay the same.

use std::collections::HashMap;
use std::f64::consts::PI;

/// sRGB `[r, g, b]` 0–255 and alpha 0–1, clamped; NaN where the colour text left a part out.
pub type Rgba = [f64; 4];

/// The browser's readings of colours [`read`] can't read: colour text → the canvas's reading
/// (`None`: the browser couldn't read it either).
pub type Fallbacks = HashMap<String, Option<String>>;

const XYZ_TO_SRGB: [[f64; 3]; 3] = [
    [3.2409699419045226, -1.537383177570094, -0.4986107602930034],
    [-0.9692436362808796, 1.8759675015077202, 0.04155505740717559],
    [
        0.05563007969699366,
        -0.20397695888897652,
        1.0569715142428786,
    ],
];
const D50_TO_D65: [[f64; 3]; 3] = [
    [
        0.9554734527042182,
        -0.023098536874261423,
        0.0632593086610217,
    ],
    [
        -0.028369706963208136,
        1.0099954580058226,
        0.021041398966943008,
    ],
    [
        0.012314001688319899,
        -0.020507696433477912,
        1.3303659366080753,
    ],
];
const P3_TO_XYZ: [[f64; 3]; 3] = [
    [0.4865709486482162, 0.26566769316909306, 0.1982172852343625],
    [0.2289745640697488, 0.6917385218365064, 0.079286914093745],
    [0.0, 0.04511338185890264, 1.043944368900976],
];

fn mat(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

/// JavaScript's `Math.sign` (NaN and ±0 kept).
fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        v
    }
}

fn to_linear(v: f64) -> f64 {
    if v.abs() <= 0.04045 {
        v / 12.92
    } else {
        sign(v) * ((v.abs() + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gamma(v: f64) -> f64 {
    if v.abs() <= 0.0031308 {
        v * 12.92
    } else {
        sign(v) * (1.055 * v.abs().powf(1.0 / 2.4) - 0.055)
    }
}

fn oklab_to_linear(l: f64, a: f64, b: f64) -> [f64; 3] {
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powf(3.0);
    let m = (l - 0.1055613458 * a - 0.0638541728 * b).powf(3.0);
    let s = (l - 0.0894841775 * a - 1.291485548 * b).powf(3.0);
    [
        4.0767416621 * l_ - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l_ + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l_ - 0.7034186147 * m + 1.707614701 * s,
    ]
}

fn lab_to_linear(l: f64, a: f64, b: f64) -> [f64; 3] {
    let k = 24389.0 / 27.0;
    let e = 216.0 / 24389.0;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let x = if fx * fx * fx > e {
        fx * fx * fx
    } else {
        (116.0 * fx - 16.0) / k
    };
    let y = if l > k * e { fy * fy * fy } else { l / k };
    let z = if fz * fz * fz > e {
        fz * fz * fz
    } else {
        (116.0 * fz - 16.0) / k
    };
    mat(
        &XYZ_TO_SRGB,
        mat(&D50_TO_D65, [x * 0.96422, y, z * 0.82521]),
    )
}

/// JavaScript's `parseFloat`: the longest number at the start of `t` (after white space), or
/// NaN.
pub(crate) fn parse_float(t: &str) -> f64 {
    let s = t.trim_start();
    let bytes = s.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    if s[i..].starts_with("Infinity") {
        return if s.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let digits = |i: &mut usize| {
        let from = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - from
    };
    let mut seen = digits(&mut i);
    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        let frac = digits(&mut j);
        if seen + frac > 0 {
            i = j;
            seen += frac;
        }
    }
    if seen == 0 {
        return f64::NAN;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) > 0 {
            i = j;
        }
    }
    s[..i].parse().unwrap_or(f64::NAN)
}

/// One component: a number, a percentage of `full`, an angle in degrees, or `none` (0). Units
/// are tested in the order the page tested them, so `grad` (which ends in `rad`) reads as
/// radians.
fn part(t: Option<&str>, full: f64) -> f64 {
    let Some(t) = t.filter(|t| *t != "none") else {
        return 0.0;
    };
    let n = parse_float(t);
    if !n.is_finite() {
        return f64::NAN;
    }
    if t.ends_with('%') {
        n / 100.0 * full
    } else if t.ends_with("turn") {
        n * 360.0
    } else if t.ends_with("rad") {
        n * 180.0 / PI
    } else {
        n
    }
}

/// CSS colour text → sRGB, or `None`. Reads rgb()/rgba(), hex, oklch(), oklab(), lab(),
/// lch() and color(srgb | srgb-linear | display-p3); out-of-gamut values are clamped.
pub fn read(input: &str) -> Option<Rgba> {
    let c = input.trim().to_lowercase();
    if c.is_empty() {
        return None;
    }
    if c == "transparent" {
        return Some([0.0, 0.0, 0.0, 0.0]);
    }
    if let Some(h) = c.strip_prefix('#') {
        if !(3..=8).contains(&h.len()) || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let h: String = if h.len() == 3 || h.len() == 4 {
            h.chars().flat_map(|ch| [ch, ch]).collect()
        } else {
            h.to_string()
        };
        if h.len() != 6 && h.len() != 8 {
            return None;
        }
        let n = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_or(f64::NAN, f64::from);
        let alpha = if h.len() == 8 { n(6) / 255.0 } else { 1.0 };
        return Some([n(0), n(2), n(4), alpha]);
    }
    let (name, body) = function(&c)?;
    let mut halves = body.split('/');
    let main = halves.next().unwrap_or("");
    let alpha_text = halves.next();
    let args: Vec<&str> = main
        .trim()
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|a| !a.is_empty())
        .collect();
    let arg = |i: usize| args.get(i).copied();
    let mut alpha = alpha_text.map_or(1.0, |a| part(Some(a.trim()), 1.0));
    let mut linear: Option<Vec<f64>> = None;
    let rgb: Vec<f64>;
    match name {
        "rgb" | "rgba" => {
            if args.len() < 3 {
                return None;
            }
            if args.len() == 4 && alpha_text.is_none() {
                alpha = part(arg(3), 1.0);
            }
            rgb = vec![
                part(arg(0), 255.0),
                part(arg(1), 255.0),
                part(arg(2), 255.0),
            ];
        }
        "oklab" => {
            linear = Some(
                oklab_to_linear(part(arg(0), 1.0), part(arg(1), 0.4), part(arg(2), 0.4)).to_vec(),
            );
            rgb = Vec::new();
        }
        "oklch" => {
            let chroma = part(arg(1), 0.4);
            let h = part(arg(2), 1.0) * PI / 180.0;
            linear = Some(
                oklab_to_linear(part(arg(0), 1.0), chroma * h.cos(), chroma * h.sin()).to_vec(),
            );
            rgb = Vec::new();
        }
        "lab" => {
            linear = Some(
                lab_to_linear(
                    part(arg(0), 100.0),
                    part(arg(1), 125.0),
                    part(arg(2), 125.0),
                )
                .to_vec(),
            );
            rgb = Vec::new();
        }
        "lch" => {
            let chroma = part(arg(1), 150.0);
            let h = part(arg(2), 1.0) * PI / 180.0;
            linear = Some(
                lab_to_linear(part(arg(0), 100.0), chroma * h.cos(), chroma * h.sin()).to_vec(),
            );
            rgb = Vec::new();
        }
        "color" => {
            let v: Vec<f64> = args
                .iter()
                .skip(1)
                .take(3)
                .map(|t| part(Some(t), 1.0))
                .collect();
            let at = |i: usize| v.get(i).copied().unwrap_or(f64::NAN);
            match arg(0) {
                Some("srgb") => rgb = v.iter().map(|x| x * 255.0).collect(),
                Some("srgb-linear") => {
                    linear = Some(v.clone());
                    rgb = Vec::new();
                }
                Some("display-p3") => {
                    let lin = [to_linear(at(0)), to_linear(at(1)), to_linear(at(2))];
                    linear = Some(mat(&XYZ_TO_SRGB, mat(&P3_TO_XYZ, lin)).to_vec());
                    rgb = Vec::new();
                }
                _ => return None,
            }
        }
        _ => return None,
    }
    let rgb = match linear {
        Some(linear) => linear.iter().map(|x| to_gamma(*x) * 255.0).collect(),
        None => rgb,
    };
    if rgb.iter().any(|x| !x.is_finite()) || !alpha.is_finite() {
        return None;
    }
    // JavaScript's Math.min(Math.max(x, 0), hi): NaN (a part left out) stays NaN.
    let clamp = |x: Option<&f64>, hi: f64| match x {
        Some(x) if !x.is_nan() => x.clamp(0.0, hi),
        _ => f64::NAN,
    };
    Some([
        clamp(rgb.first(), 255.0),
        clamp(rgb.get(1), 255.0),
        clamp(rgb.get(2), 255.0),
        clamp(Some(&alpha), 1.0),
    ])
}

/// `name(body)` with no parentheses inside, as `^([a-z-]+)\(\s*([^()]*)\)$` reads it.
fn function(c: &str) -> Option<(&str, &str)> {
    let open = c.find('(')?;
    let name = &c[..open];
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        return None;
    }
    let body = c[open + 1..].strip_suffix(')')?;
    if body.contains(['(', ')']) {
        return None;
    }
    Some((name, body.trim_start()))
}

/// OKLCH chroma of sRGB `[r, g, b]` (0–255).
pub fn chroma_of(p: Rgba) -> f64 {
    let [r, g, b] = [
        to_linear(p[0] / 255.0),
        to_linear(p[1] / 255.0),
        to_linear(p[2] / 255.0),
    ];
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
    (a * a + b * b).sqrt()
}

/// Any colour the page computed → sRGB: [`read`], else [`read`] of the browser's reading.
pub fn rgba_of(c: &str, fallbacks: &Fallbacks) -> Option<Rgba> {
    read(c).or_else(|| fallbacks.get(c).and_then(|f| f.as_deref()).and_then(read))
}

/// No colour, `transparent`, or alpha 0.
pub fn transparent(c: Option<&str>, fallbacks: &Fallbacks) -> bool {
    match c {
        None | Some("") | Some("transparent") => true,
        Some(c) => rgba_of(c, fallbacks).is_some_and(|p| p[3] == 0.0),
    }
}

/// OKLCH chroma of any colour; "chromatic" is ≥ 0.03.
pub fn chroma(c: &str, fallbacks: &Fallbacks) -> f64 {
    match rgba_of(c, fallbacks) {
        Some(p) if p[3] != 0.0 => chroma_of(p),
        _ => 0.0,
    }
}

/// JavaScript's `Math.round`: halves go up.
pub(crate) fn js_round(x: f64) -> f64 {
    let f = x.floor();
    (if x - f >= 0.5 { f + 1.0 } else { f }) + 0.0
}

/// A rounded number as JavaScript writes it in a string.
fn js_int(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else {
        format!("{}", x as i64)
    }
}

/// The opaque colour behind an element, as `rgb()`: `layers` are its background colours and
/// its ancestors', from the element up. Transparent ones are passed over, translucent ones
/// composited over what's below, down to the first opaque one, else white.
pub fn effective_background(layers: &[String], fallbacks: &Fallbacks) -> String {
    let mut over = Vec::new();
    let mut base = None;
    for c in layers {
        if transparent(Some(c), fallbacks) {
            continue;
        }
        let Some(p) = rgba_of(c, fallbacks) else {
            continue;
        };
        if p[3] >= 1.0 {
            base = Some(p);
            break;
        }
        over.push(p);
    }
    let [mut r, mut g, mut b, _] = base.unwrap_or([255.0, 255.0, 255.0, 1.0]);
    for [lr, lg, lb, a] in over.into_iter().rev() {
        r = lr * a + r * (1.0 - a);
        g = lg * a + g * (1.0 - a);
        b = lb * a + b * (1.0 - a);
    }
    format!(
        "rgb({}, {}, {})",
        js_int(js_round(r)),
        js_int(js_round(g)),
        js_int(js_round(b))
    )
}

#[cfg(test)]
#[path = "tests/computed.rs"]
mod tests;
