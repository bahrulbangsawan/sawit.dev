//! An element's styles as the screen checks need them: the declarations that win the cascade
//! for it, `var()` resolved, and what it inherits (colour, font, the background behind it).

use std::collections::HashMap;

use super::super::Rgba;
use super::super::color::parse_color;
use super::super::css::{El, Sheet, declarations};

/// What makes one declaration win over another: `!important`, specificity, then place.
type Priority = (bool, (u32, u32, u32), usize);

/// The declarations that win for one element, by property.
#[derive(Clone, Debug, Default)]
pub struct Cascaded(HashMap<String, (String, Priority)>);

impl Cascaded {
    /// The winners for `path`'s last element: the rules matching it, then its `style` attribute.
    pub fn of(sheet: &Sheet, path: &[El], style: Option<&str>) -> Self {
        let mut out = Cascaded::default();
        for rule in &sheet.rules {
            if let Some(spec) = rule.matches(path) {
                for (name, value, important) in &rule.decls {
                    out.put(name, value, (*important, spec, rule.order));
                }
            }
        }
        for (name, value, important) in style.map(declarations).unwrap_or_default() {
            out.put(&name, &value, (important, (1000, 0, 0), usize::MAX));
        }
        out
    }

    fn put(&mut self, name: &str, value: &str, priority: Priority) {
        let wins = self.0.get(name).is_none_or(|(_, held)| priority >= *held);
        if wins {
            self.0
                .insert(name.to_string(), (value.to_string(), priority));
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|(v, _)| v.as_str())
    }

    /// The value of whichever of `names` wins, with its name.
    fn best<'a>(&'a self, names: &[&'a str]) -> Option<(&'a str, &'a str)> {
        let mut best: Option<(&str, &str, Priority)> = None;
        for name in names {
            if let Some((value, priority)) = self.0.get(*name)
                && best.is_none_or(|(_, _, held)| *priority >= held)
            {
                best = Some((name, value, *priority));
            }
        }
        best.map(|(n, v, _)| (n, v))
    }

    /// The custom properties declared here.
    pub fn customs(&self) -> impl Iterator<Item = (&str, &str)> {
        let all = self.0.iter();
        all.filter(|(n, _)| n.starts_with("--"))
            .map(|(n, (v, _))| (n.as_str(), v.as_str()))
    }
}

/// What an element inherits, and the background painted behind it.
#[derive(Clone, Debug)]
pub struct Inherited {
    pub custom: HashMap<String, String>,
    pub color: Rgba,
    pub font_size: f64,
    pub weight: u32,
    /// Line height in px, `None` for `normal`.
    pub line_height: Option<f64>,
    /// The opaque colour behind, and whether an image or gradient is there instead.
    pub bg: Rgba,
    pub bg_image: bool,
    pub opacity: f64,
    pub nowrap: bool,
}

impl Default for Inherited {
    fn default() -> Self {
        Self {
            custom: HashMap::new(),
            color: Rgba::BLACK,
            font_size: 16.0,
            weight: 400,
            line_height: None,
            bg: Rgba::WHITE,
            bg_image: false,
            opacity: 1.0,
            nowrap: false,
        }
    }
}

/// An element's computed values, as the checks read them.
#[derive(Clone, Debug)]
pub struct Computed {
    pub cascaded: Cascaded,
    pub inherited: Inherited,
    pub hidden: bool,
}

impl Computed {
    /// The styles of element `tag` with `cascaded`, under `parent`'s.
    pub fn new(tag: &str, cascaded: Cascaded, parent: &Inherited) -> Self {
        let mut own = parent.clone();
        for (name, value) in cascaded.customs() {
            own.custom.insert(name.to_string(), value.to_string());
        }
        let get = |name: &str| cascaded.get(name).map(|v| resolve(v, &own.custom, 0));
        let ua_size = ua_font_size(tag);
        let size = get("font-size")
            .and_then(|v| font_size(&v, parent.font_size))
            .or_else(|| ua_size.map(|f| f(parent.font_size)));
        own.font_size = size.unwrap_or(parent.font_size);
        own.weight = get("font-weight")
            .and_then(|v| weight(&v, parent.weight))
            .unwrap_or(match tag {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "b" | "strong" | "th" => 700,
                _ => parent.weight,
            });
        if let Some(lh) = get("line-height") {
            own.line_height = line_height(&lh, own.font_size);
        } else if size.is_some() {
            // An inherited number scales with the new size; px stay; normal is recomputed.
            own.line_height = parent
                .line_height
                .map(|lh| lh / parent.font_size * own.font_size);
        }
        if let Some(c) = get("color").and_then(|v| color_of(&v, parent.color)) {
            own.color = c;
        }
        if let Some(o) = get("opacity").and_then(|v| v.trim().parse::<f64>().ok()) {
            own.opacity *= o.clamp(0.0, 1.0);
        }
        let bg = get("background-color")
            .or_else(|| get("background"))
            .map(|v| background(&v, own.color));
        match bg {
            Some(Background::Image) => own.bg_image = true,
            Some(Background::Color(c)) if c.a > 0.0 => {
                own.bg = c.over(parent.bg);
                own.bg_image = parent.bg_image && c.a < 1.0;
            }
            _ => {}
        }
        if let Some(ws) = get("white-space") {
            own.nowrap = matches!(ws.trim(), "nowrap" | "pre");
        }
        let hidden = get("display").is_some_and(|d| d.trim() == "none")
            || get("visibility").is_some_and(|v| v.trim() == "hidden");
        Computed {
            cascaded,
            inherited: own,
            hidden,
        }
    }

    /// Declared value `name`, `var()` resolved.
    pub fn get(&self, name: &str) -> Option<String> {
        let v = self.cascaded.get(name)?;
        Some(resolve(v, &self.inherited.custom, 0))
    }

    /// A length property in px (`None` for `auto`, a percentage, `calc()`...).
    pub fn px(&self, name: &str) -> Option<f64> {
        length(&self.get(name)?, self.inherited.font_size)
    }

    /// Padding (`padding`) or border width (`border`) on each side: top, right, bottom, left.
    pub fn sides(&self, what: &str) -> [f64; 4] {
        let names = ["top", "right", "bottom", "left"];
        let mut out = [0.0; 4];
        for (i, side) in names.iter().enumerate() {
            let axis = if i % 2 == 0 { "block" } else { "inline" };
            let candidates = match what {
                "border" => [
                    format!("border-{side}-width"),
                    format!("border-{side}"),
                    format!("border-{axis}-width"),
                    "border-width".to_string(),
                    "border".to_string(),
                ],
                _ => [
                    format!("{what}-{side}"),
                    format!("{what}-{axis}"),
                    what.to_string(),
                    String::new(),
                    String::new(),
                ],
            };
            let names: Vec<&str> = candidates
                .iter()
                .map(String::as_str)
                .filter(|n| !n.is_empty())
                .collect();
            let Some((name, value)) = self.cascaded.best(&names) else {
                continue;
            };
            let value = resolve(value, &self.inherited.custom, 0);
            let em = self.inherited.font_size;
            let parts: Vec<f64> = value
                .split_whitespace()
                .filter_map(|p| length(p, em))
                .collect();
            let pick = if name.contains(side) || name == "border" {
                parts.first()
            } else if name.ends_with(axis) || name.ends_with(&format!("{axis}-width")) {
                // block: top then bottom; inline: left then right.
                let second = matches!(i, 1 | 2);
                parts.get(usize::from(second)).or(parts.first())
            } else {
                match parts.len() {
                    1 => parts.first(),
                    2 => parts.get(i % 2),
                    3 => parts.get(if i == 3 { 1 } else { i }),
                    _ => parts.get(i),
                }
            };
            out[i] = pick.copied().unwrap_or(0.0);
        }
        out
    }
}

/// The default size of a heading or `small` from its parent's.
fn ua_font_size(tag: &str) -> Option<fn(f64) -> f64> {
    Some(match tag {
        "h1" => |p| p * 2.0,
        "h2" => |p| p * 1.5,
        "h3" => |p| p * 1.17,
        "h5" | "small" => |p| p * 0.83,
        "h6" => |p| p * 0.67,
        // WebKit's controls: 13.33px unless the page sets them.
        "button" | "input" | "select" | "textarea" => |_| 13.333,
        _ => return None,
    })
}

/// `value` with its `var(--x, fallback)`s replaced by the custom properties in `custom`.
pub fn resolve(value: &str, custom: &HashMap<String, String>, depth: u32) -> String {
    let Some(at) = value.find("var(") else {
        return value.to_string();
    };
    if depth > 8 {
        return value.to_string();
    }
    let open = at + 4;
    let mut level = 1;
    let mut close = value.len();
    for (i, c) in value[open..].char_indices() {
        match c {
            '(' => level += 1,
            ')' => {
                level -= 1;
                if level == 0 {
                    close = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let inner = &value[open..close.min(value.len())];
    let (name, fallback) = match inner.split_once(',') {
        Some((n, f)) => (n.trim(), Some(f.trim())),
        None => (inner.trim(), None),
    };
    let got = custom
        .get(name)
        .map(String::as_str)
        .or(fallback)
        .unwrap_or("");
    let got = resolve(got, custom, depth + 1);
    let rest = value.get(close + 1..).unwrap_or("");
    resolve(&format!("{}{got}{rest}", &value[..at]), custom, depth + 1)
}

/// A length in px: `12px`, `0.75rem`, `1.5em` (of `em` px), `0`.
pub fn length(value: &str, em: f64) -> Option<f64> {
    let v = value.trim().to_ascii_lowercase();
    if v == "0" {
        return Some(0.0);
    }
    let (number, scale) = if let Some(n) = v.strip_suffix("px") {
        (n, 1.0)
    } else if let Some(n) = v.strip_suffix("rem") {
        (n, 16.0)
    } else {
        (v.strip_suffix("em")?, em)
    };
    number.trim().parse::<f64>().ok().map(|n| n * scale)
}

fn font_size(value: &str, parent: f64) -> Option<f64> {
    let v = value.trim().to_ascii_lowercase();
    if let Some(p) = v.strip_suffix('%') {
        return p.parse::<f64>().ok().map(|p| p / 100.0 * parent);
    }
    let keyword = match v.as_str() {
        "xx-small" => 9.0,
        "x-small" => 10.0,
        "small" => 13.0,
        "medium" => 16.0,
        "large" => 18.0,
        "x-large" => 24.0,
        "xx-large" => 32.0,
        "smaller" => parent / 1.2,
        "larger" => parent * 1.2,
        _ => return length(&v, parent),
    };
    Some(keyword)
}

fn weight(value: &str, parent: u32) -> Option<u32> {
    match value.trim() {
        "normal" => Some(400),
        "bold" => Some(700),
        "bolder" => Some((parent + 300).min(900)),
        "lighter" => Some(parent.saturating_sub(300).max(100)),
        v => v.parse().ok(),
    }
}

fn line_height(value: &str, size: f64) -> Option<f64> {
    let v = value.trim();
    if v == "normal" {
        return None;
    }
    if let Some(p) = v.strip_suffix('%') {
        return p.parse::<f64>().ok().map(|p| p / 100.0 * size);
    }
    match v.parse::<f64>() {
        Ok(n) => Some(n * size),
        Err(_) => length(v, size),
    }
}

/// A colour value, `currentColor` being `current`.
pub fn color_of(value: &str, current: Rgba) -> Option<Rgba> {
    match value.trim().eq_ignore_ascii_case("currentcolor") {
        true => Some(current),
        false => parse_color(value),
    }
}

/// What a `background` paints.
enum Background {
    Color(Rgba),
    Image,
    None,
}

fn background(value: &str, current: Rgba) -> Background {
    let lower = value.to_ascii_lowercase();
    if lower.contains("gradient(") || lower.contains("url(") || lower.contains("image-set(") {
        return Background::Image;
    }
    if let Some(c) = color_of(value, current) {
        return Background::Color(c);
    }
    // A shorthand: the colour is one of its parts, a function's spaces kept.
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, c) in value.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ' ' if depth == 0 => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    let found = parts.iter().find_map(|p| color_of(p, current));
    found.map_or(Background::None, Background::Color)
}
