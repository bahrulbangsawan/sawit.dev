//! CSS color parsing, OKLCH/OKLab, WCAG contrast and 50-950 ramps.

use serde::{Serialize, Serializer};

pub const RAMP_L: [f64; 11] = [
    0.97, 0.93, 0.88, 0.80, 0.71, 0.62, 0.54, 0.46, 0.38, 0.30, 0.22,
];
pub const RAMP_C: [f64; 11] = [
    0.10, 0.22, 0.40, 0.62, 0.85, 1.00, 0.95, 0.85, 0.72, 0.58, 0.45,
];
pub const STEPS: [u16; 11] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn lin_to_oklab(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    (
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    )
}

fn oklab_to_lin(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    (
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s,
    )
}

fn lin_to_rgba(r: f64, g: f64, b: f64, a: f32) -> Rgba {
    let ch = |c: f64| (from_linear(c.clamp(0.0, 1.0)) * 255.0).round() as u8;
    Rgba {
        r: ch(r),
        g: ch(g),
        b: ch(b),
        a: a.clamp(0.0, 1.0),
    }
}

fn lch_to_lab(l: f64, c: f64, h: f64) -> (f64, f64, f64) {
    let hr = h.to_radians();
    (l, c * hr.cos(), c * hr.sin())
}

impl Rgba {
    pub fn to_hex(&self) -> String {
        if self.a < 1.0 {
            let a = (self.a.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, a)
        } else {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        }
    }

    fn oklab(&self) -> (f64, f64, f64) {
        lin_to_oklab(
            to_linear(self.r as f64 / 255.0),
            to_linear(self.g as f64 / 255.0),
            to_linear(self.b as f64 / 255.0),
        )
    }

    /// (L, C, H in degrees)
    pub fn to_oklch(&self) -> (f64, f64, f64) {
        let (l, a, b) = self.oklab();
        let c = a.hypot(b);
        let mut h = b.atan2(a).to_degrees();
        if h < 0.0 {
            h += 360.0;
        }
        (l, c, h)
    }

    /// `None` when out of the sRGB gamut.
    pub fn from_oklch(l: f64, c: f64, h: f64) -> Option<Rgba> {
        let (la, a, b) = lch_to_lab(l, c, h);
        let (r, g, bl) = oklab_to_lin(la, a, b);
        const EPS: f64 = 0.0005;
        if [r, g, bl].iter().any(|v| *v < -EPS || *v > 1.0 + EPS) {
            return None;
        }
        Some(lin_to_rgba(r, g, bl, 1.0))
    }

    pub fn oklch_css(&self) -> String {
        let (l, c, h) = self.to_oklch();
        let h = if c < 0.0005 { 0.0 } else { h };
        format!("oklch({} {} {})", trim(l, 4), trim(c, 4), trim(h, 2))
    }
}

fn trim(v: f64, dp: usize) -> String {
    let s = format!("{:.*}", dp, v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

// ---------- parsing ----------

pub fn parse(css: &str) -> Option<Rgba> {
    let s = css.trim().to_ascii_lowercase();
    if s == "transparent" {
        return Some(Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 0.0,
        });
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    let open = s.find('(')?;
    let inner = s[open + 1..].strip_suffix(')')?;
    let name = s[..open].trim();
    let (parts, alpha) = split_args(inner)?;
    let alpha = match alpha {
        Some(a) => parse_alpha(&a)?,
        None => 1.0,
    };
    match name {
        "rgb" | "rgba" => {
            if parts.len() != 3 {
                return None;
            }
            let mut c = [0u8; 3];
            for (i, p) in parts.iter().enumerate() {
                let v = if let Some(pc) = p.strip_suffix('%') {
                    pc.trim().parse::<f64>().ok()? * 2.55
                } else {
                    p.parse::<f64>().ok()?
                };
                c[i] = v.round().clamp(0.0, 255.0) as u8;
            }
            Some(Rgba {
                r: c[0],
                g: c[1],
                b: c[2],
                a: alpha,
            })
        }
        "hsl" | "hsla" => {
            if parts.len() != 3 {
                return None;
            }
            let h = parse_hue(&parts[0])?;
            let sat = pct(&parts[1])?.clamp(0.0, 1.0);
            let lig = pct(&parts[2])?.clamp(0.0, 1.0);
            let k = |n: f64| (n + h / 30.0) % 12.0;
            let a = sat * lig.min(1.0 - lig);
            let f = |n: f64| lig - a * (k(n) - 3.0).min(9.0 - k(n)).clamp(-1.0, 1.0);
            let q = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
            Some(Rgba {
                r: q(f(0.0)),
                g: q(f(8.0)),
                b: q(f(4.0)),
                a: alpha,
            })
        }
        "oklch" | "oklab" => {
            if parts.len() != 3 {
                return None;
            }
            let l = num_or_pct(&parts[0], 1.0)?;
            let (la, a, b) = if name == "oklch" {
                let c = num_or_pct(&parts[1], 0.4)?;
                let h = parse_hue(&parts[2])?;
                lch_to_lab(l, c, h)
            } else {
                (l, num_or_pct(&parts[1], 0.4)?, num_or_pct(&parts[2], 0.4)?)
            };
            let (r, g, bl) = oklab_to_lin(la, a, b);
            Some(lin_to_rgba(r, g, bl, alpha))
        }
        "color" => {
            if parts.len() != 4 {
                return None;
            }
            let space = parts[0].as_str();
            let v: Vec<f64> = parts[1..]
                .iter()
                .map(|p| num_or_pct(p, 1.0))
                .collect::<Option<_>>()?;
            let (r, g, b) = (to_linear(v[0]), to_linear(v[1]), to_linear(v[2]));
            match space {
                "srgb" => Some(lin_to_rgba(r, g, b, alpha)),
                "display-p3" => Some(lin_to_rgba(
                    1.224_940_2 * r - 0.224_940_4 * g,
                    -0.042_056_9 * r + 1.042_057_1 * g,
                    -0.019_637_6 * r - 0.078_636_1 * g + 1.098_273_5 * b,
                    alpha,
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

fn parse_hex(h: &str) -> Option<Rgba> {
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    let (r, g, b, a) = match h.len() {
        3 => (d(0)?, d(1)?, d(2)?, 255),
        4 => (d(0)?, d(1)?, d(2)?, d(3)?),
        6 => (p(0)?, p(2)?, p(4)?, 255),
        8 => (p(0)?, p(2)?, p(4)?, p(6)?),
        _ => return None,
    };
    Some(Rgba {
        r,
        g,
        b,
        a: a as f32 / 255.0,
    })
}

/// Splits "a, b, c / d", "a b c / d" or "a, b, c, d" into (components, alpha).
fn split_args(inner: &str) -> Option<(Vec<String>, Option<String>)> {
    let (main, mut alpha) = match inner.split_once('/') {
        Some((m, a)) => (m, Some(a.trim().to_string())),
        None => (inner, None),
    };
    let mut parts: Vec<String> = main
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect();
    let is_color_fn = matches!(
        parts.first().map(String::as_str),
        Some("srgb" | "display-p3")
    );
    if alpha.is_none() && !is_color_fn && parts.len() == 4 {
        alpha = parts.pop();
    }
    Some((parts, alpha))
}

fn parse_alpha(s: &str) -> Option<f32> {
    let v = if let Some(p) = s.strip_suffix('%') {
        p.trim().parse::<f32>().ok()? / 100.0
    } else if s == "none" {
        0.0
    } else {
        s.parse::<f32>().ok()?
    };
    Some(v.clamp(0.0, 1.0))
}

fn pct(s: &str) -> Option<f64> {
    let p = s.strip_suffix('%')?;
    Some(p.trim().parse::<f64>().ok()? / 100.0)
}

fn num_or_pct(s: &str, full: f64) -> Option<f64> {
    if s == "none" {
        return Some(0.0);
    }
    match s.strip_suffix('%') {
        Some(p) => Some(p.trim().parse::<f64>().ok()? / 100.0 * full),
        None => s.parse::<f64>().ok(),
    }
}

fn parse_hue(s: &str) -> Option<f64> {
    if s == "none" {
        return Some(0.0);
    }
    let h = if let Some(v) = s.strip_suffix("deg") {
        v.parse::<f64>().ok()?
    } else if let Some(v) = s.strip_suffix("grad") {
        v.parse::<f64>().ok()? * 0.9
    } else if let Some(v) = s.strip_suffix("rad") {
        v.parse::<f64>().ok()?.to_degrees()
    } else if let Some(v) = s.strip_suffix("turn") {
        v.parse::<f64>().ok()? * 360.0
    } else {
        s.parse::<f64>().ok()?
    };
    Some(h.rem_euclid(360.0))
}

// ---------- metrics ----------

pub fn delta_e(a: Rgba, b: Rgba) -> f64 {
    let (l1, a1, b1) = a.oklab();
    let (l2, a2, b2) = b.oklab();
    ((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt()
}

fn luminance(r: u8, g: u8, b: u8) -> f64 {
    0.2126 * to_linear(r as f64 / 255.0)
        + 0.7152 * to_linear(g as f64 / 255.0)
        + 0.0722 * to_linear(b as f64 / 255.0)
}

pub fn contrast_ratio(fg: Rgba, bg: Rgba) -> f64 {
    let fg = if fg.a < 1.0 {
        let a = fg.a as f64;
        let mix = |f: u8, b: u8| (f as f64 * a + b as f64 * (1.0 - a)).round() as u8;
        Rgba {
            r: mix(fg.r, bg.r),
            g: mix(fg.g, bg.g),
            b: mix(fg.b, bg.b),
            a: 1.0,
        }
    } else {
        fg
    };
    let (l1, l2) = (luminance(fg.r, fg.g, fg.b), luminance(bg.r, bg.g, bg.b));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn ramp(base: Rgba) -> [Rgba; 11] {
    let (bl, bc, bh) = base.to_oklch();
    let nearest = (0..11)
        .min_by(|&i, &j| {
            (RAMP_L[i] - bl)
                .abs()
                .partial_cmp(&(RAMP_L[j] - bl).abs())
                .unwrap()
        })
        .unwrap();
    let mut out = [base; 11];
    for i in 0..11 {
        if i == nearest {
            out[i] = Rgba { a: 1.0, ..base };
            continue;
        }
        let mut c = bc * RAMP_C[i];
        out[i] = loop {
            if let Some(x) = Rgba::from_oklch(RAMP_L[i], c, bh) {
                break x;
            }
            if c <= 0.0 {
                break base;
            }
            c = (c - 0.005).max(0.0);
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Rgba {
        parse(s).unwrap()
    }

    fn near(a: Rgba, b: &str) {
        let b = hex(b);
        for (x, y) in [(a.r, b.r), (a.g, b.g), (a.b, b.b)] {
            assert!(
                (x as i32 - y as i32).abs() <= 1,
                "{} vs {}",
                a.to_hex(),
                b.to_hex()
            );
        }
    }

    #[test]
    fn parses_formats() {
        for s in ["#0071e3", "rgb(0, 113, 227)", "rgb(0 113 227)"] {
            assert_eq!(hex(s).to_hex(), "#0071e3", "{s}");
        }
        assert_eq!(hex("rgb(0 113 227 / 50%)").a, 0.5);
        assert_eq!(hex("rgba(0,0,0,0)").a, 0.0);
        assert_eq!(hex("hsl(210 100% 50%)").to_hex(), "#0080ff");
        near(hex("oklch(0.628 0.2577 29.23)"), "#ff0000");
        near(hex("color(srgb 1 0 0)"), "#ff0000");
        near(hex("color(display-p3 1 0 0)"), "#ff0000");
        assert_eq!(hex("#abc").to_hex(), "#aabbcc");
        assert!((hex("#11223380").a - 0.502).abs() < 0.001);
        assert_eq!(hex("transparent").a, 0.0);
        assert!(parse("bogus").is_none());
    }

    #[test]
    fn oklch_values() {
        let (l, c, h) = hex("#ff0000").to_oklch();
        assert!(
            (l - 0.628).abs() < 0.001 && (c - 0.2577).abs() < 0.001 && (h - 29.23).abs() < 0.05
        );
        let (l, c, h) = hex("#0071e3").to_oklch();
        assert!(
            (l - 0.5629).abs() < 0.001 && (c - 0.1933).abs() < 0.001 && (h - 256.16).abs() < 0.05
        );
        assert_eq!(hex("#ffffff").oklch_css(), "oklch(1 0 0)");
    }

    #[test]
    fn contrast() {
        let w = hex("#ffffff");
        assert!((contrast_ratio(hex("#000000"), w) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(hex("#777777"), w) - 4.48).abs() < 0.01);
        assert!((contrast_ratio(hex("#0071e3"), w) - 4.70).abs() < 0.01);
        assert!(delta_e(w, hex("#fefefe")) < 0.02);
    }

    #[test]
    fn ramps() {
        assert_eq!(ramp(hex("#0071e3"))[6].to_hex(), "#0071e3");
        assert_eq!(ramp(hex("#0f7fff"))[5].to_hex(), "#0f7fff");
        let r = ramp(hex("#0071e3"));
        for w in r.windows(2) {
            assert!(w[0].to_oklch().0 > w[1].to_oklch().0);
        }
        for x in r {
            assert_eq!(parse(&x.to_hex()).unwrap(), x);
        }
    }
}
