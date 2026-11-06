//! CSS colours as the checks read them: hex, `rgb()`, `hsl()` and the common keywords.

/// A colour with straight alpha, each channel 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub const WHITE: Rgba = Rgba::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Rgba = Rgba::rgb(0.0, 0.0, 0.0);
    pub const CLEAR: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Rgba { r, g, b, a: 1.0 }
    }

    /// This colour painted over `under`, which is opaque: the opaque colour that shows.
    pub fn over(self, under: Rgba) -> Rgba {
        let a = self.a.clamp(0.0, 1.0);
        let mix = |f: f64, b: f64| f * a + b * (1.0 - a);
        Rgba::rgb(
            mix(self.r, under.r),
            mix(self.g, under.g),
            mix(self.b, under.b),
        )
    }

    /// `#rrggbb`, with `aa` when it isn't opaque.
    pub fn hex(self) -> String {
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut out = format!(
            "#{:02x}{:02x}{:02x}",
            byte(self.r),
            byte(self.g),
            byte(self.b)
        );
        if self.a < 1.0 {
            out.push_str(&format!("{:02x}", byte(self.a)));
        }
        out
    }
}

/// The colour CSS value `value` names, if it is one the checks read: `#rgb`, `#rgba`, `#rrggbb`,
/// `#rrggbbaa`, `rgb()`/`rgba()`, `hsl()`/`hsla()`, `transparent` and the basic keywords.
pub fn parse_color(value: &str) -> Option<Rgba> {
    let v = value.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(args) = function(&v, &["rgb", "rgba"]) {
        let [r, g, b, a] = args4(&args)?;
        let channel = |s: &str| -> Option<f64> {
            match s.strip_suffix('%') {
                Some(p) => p.parse::<f64>().ok().map(|p| p / 100.0),
                None => s.parse::<f64>().ok().map(|v| v / 255.0),
            }
        };
        return Some(Rgba {
            r: channel(r)?.clamp(0.0, 1.0),
            g: channel(g)?.clamp(0.0, 1.0),
            b: channel(b)?.clamp(0.0, 1.0),
            a: alpha(a)?,
        });
    }
    if let Some(args) = function(&v, &["hsl", "hsla"]) {
        let [h, s, l, a] = args4(&args)?;
        let h = h.trim_end_matches("deg").parse::<f64>().ok()?;
        let s = s.trim_end_matches('%').parse::<f64>().ok()? / 100.0;
        let l = l.trim_end_matches('%').parse::<f64>().ok()? / 100.0;
        let (r, g, b) = hsl(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
        return Some(Rgba {
            r,
            g,
            b,
            a: alpha(a)?,
        });
    }
    let named = match v.as_str() {
        "transparent" => Rgba::CLEAR,
        "white" => Rgba::WHITE,
        "black" => Rgba::BLACK,
        "red" => Rgba::rgb(1.0, 0.0, 0.0),
        "green" => Rgba::rgb(0.0, 128.0 / 255.0, 0.0),
        "blue" => Rgba::rgb(0.0, 0.0, 1.0),
        "gray" | "grey" => Rgba::rgb(0.502, 0.502, 0.502),
        "silver" => Rgba::rgb(0.753, 0.753, 0.753),
        "lightgray" | "lightgrey" => Rgba::rgb(0.827, 0.827, 0.827),
        "darkgray" | "darkgrey" => Rgba::rgb(0.663, 0.663, 0.663),
        "whitesmoke" => Rgba::rgb(0.961, 0.961, 0.961),
        "yellow" => Rgba::rgb(1.0, 1.0, 0.0),
        "orange" => Rgba::rgb(1.0, 0.647, 0.0),
        "navy" => Rgba::rgb(0.0, 0.0, 0.502),
        _ => return None,
    };
    Some(named)
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let long: String = match hex.len() {
        3 | 4 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => hex.to_string(),
        _ => return None,
    };
    let byte = |i: usize| {
        u8::from_str_radix(&long[i..i + 2], 16)
            .ok()
            .map(|b| f64::from(b) / 255.0)
    };
    Some(Rgba {
        r: byte(0)?,
        g: byte(2)?,
        b: byte(4)?,
        a: if long.len() == 8 { byte(6)? } else { 1.0 },
    })
}

/// The arguments of `value` when it calls one of `names`: `rgb(1 2 3 / 50%)` → `1 2 3 / 50%`.
fn function(value: &str, names: &[&str]) -> Option<String> {
    let open = value.find('(')?;
    let name = value[..open].trim();
    if !names.contains(&name) || !value.ends_with(')') {
        return None;
    }
    Some(value[open + 1..value.len() - 1].to_string())
}

/// Three channels and an alpha (`1` when not given), from the comma or the space syntax.
fn args4(args: &str) -> Option<[&str; 4]> {
    let parts: Vec<&str> = args
        .split([',', ' ', '/'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    match parts[..] {
        [a, b, c] => Some([a, b, c, "1"]),
        [a, b, c, d] => Some([a, b, c, d]),
        _ => None,
    }
}

fn alpha(a: &str) -> Option<f64> {
    let v = match a.strip_suffix('%') {
        Some(p) => p.parse::<f64>().ok()? / 100.0,
        None => a.parse::<f64>().ok()?,
    };
    Some(v.clamp(0.0, 1.0))
}

fn hsl(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r + m, g + m, b + m)
}
