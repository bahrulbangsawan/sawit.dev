//! Flattening: the palette baked into Pele's SVG, every `var()` and `color-mix()` resolved.

use std::collections::HashMap;

use super::Palette;

// --- Flattening -----------------------------------------------------------

/// Bakes `palette` into Pele's SVG: resolves every `var()` (custom properties
/// are declared in `style` attributes, the theme's `--pele-*` win), evaluates
/// `color-mix()`, drops the declarations, and paints the background so the
/// file reads the same on any canvas.
pub fn flatten(svg: &str, palette: &Palette) -> String {
    let mut vars: HashMap<String, String> = palette
        .vars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    rewrite_attrs(svg, |name, value| {
        if name == "style" {
            for (k, v) in declarations(value).filter_map(custom_property) {
                vars.entry(k.to_string()).or_insert_with(|| v.to_string());
            }
        }
        None
    });
    let flat = rewrite_attrs(svg, |name, value| {
        let kept;
        let value = if name == "style" {
            kept = declarations(value)
                .filter(|d| custom_property(d).is_none())
                .collect::<Vec<_>>()
                .join(";");
            kept.as_str()
        } else if value.contains("var(") || value.contains("color-mix(") {
            value
        } else {
            return None;
        };
        Some(mix_colors(&resolve(value, &vars, 0)))
    });
    with_background(&flat, &palette.background)
}

/// Calls `f(attribute, value)` for every quoted attribute value in the
/// markup (not text content); a `Some` replaces the value.
fn rewrite_attrs(svg: &str, mut f: impl FnMut(&str, &str) -> Option<String>) -> String {
    let bytes = svg.as_bytes();
    let mut out = String::with_capacity(svg.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let mut name = "";
        i += 1;
        while i < bytes.len() && bytes[i] != b'>' {
            match bytes[i] {
                b'=' => {
                    let before = svg[..i].trim_end();
                    let start = before
                        .rfind(|c: char| c.is_whitespace() || c == '<')
                        .map_or(0, |p| p + 1);
                    name = &before[start..];
                    i += 1;
                }
                quote @ (b'"' | b'\'') => {
                    let start = i + 1;
                    let end = svg[start..]
                        .find(quote as char)
                        .map_or(svg.len(), |n| start + n);
                    if let Some(new) = f(name, &svg[start..end]) {
                        out.push_str(&svg[copied..start]);
                        out.push_str(&new);
                        copied = end;
                    }
                    i = end + 1;
                }
                _ => i += 1,
            }
        }
    }
    out.push_str(&svg[copied.min(svg.len())..]);
    out
}

/// The `;`-separated declarations of a `style` value (not inside parens).
fn declarations(style: &str) -> impl Iterator<Item = &str> {
    split_top(style, ';')
        .into_iter()
        .map(str::trim)
        .filter(|d| !d.is_empty())
}

fn custom_property(declaration: &str) -> Option<(&str, &str)> {
    let (k, v) = declaration.split_once(':')?;
    let k = k.trim();
    k.starts_with("--").then(|| (k, v.trim()))
}

/// Splits on `sep` outside parentheses.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Length of `s` up to the `)` closing an already-open paren.
fn closing_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Replaces each `var(--name[, fallback])` with its value.
fn resolve(value: &str, vars: &HashMap<String, String>, depth: u8) -> String {
    if depth > 8 || !value.contains("var(") {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find("var(") {
        out.push_str(&rest[..at]);
        let inner_start = at + "var(".len();
        let Some(len) = closing_paren(&rest[inner_start..]) else {
            out.push_str(&rest[at..]);
            return out;
        };
        let inner = &rest[inner_start..inner_start + len];
        let (name, fallback) = match inner.split_once(',') {
            Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
            None => (inner.trim(), None),
        };
        let replacement = vars.get(name).map(String::as_str).or(fallback);
        out.push_str(&resolve(replacement.unwrap_or_default(), vars, depth + 1));
        rest = &rest[inner_start + len + 1..];
    }
    out.push_str(rest);
    out
}

/// Evaluates `color-mix(in srgb, A p%, B [q%])` into `#rrggbb`.
pub(super) fn mix_colors(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find("color-mix(") {
        out.push_str(&rest[..at]);
        let inner_start = at + "color-mix(".len();
        let Some(len) = closing_paren(&rest[inner_start..]) else {
            out.push_str(&rest[at..]);
            return out;
        };
        let inner = mix_colors(&rest[inner_start..inner_start + len]);
        out.push_str(&mix(&inner).unwrap_or_else(|| format!("color-mix({inner})")));
        rest = &rest[inner_start + len + 1..];
    }
    out.push_str(rest);
    out
}

fn mix(args: &str) -> Option<String> {
    let parts = split_top(args, ',');
    let [space, a, b] = parts.as_slice() else {
        return None;
    };
    if space.trim() != "in srgb" {
        return None;
    }
    let (a, pa) = color_stop(a)?;
    let (b, pb) = color_stop(b)?;
    let wa = match (pa, pb) {
        (Some(pa), _) => pa,
        (None, Some(pb)) => 1.0 - pb,
        (None, None) => 0.5,
    };
    let channel = |i: usize| (a[i] * wa + b[i] * (1.0 - wa)).round().clamp(0.0, 255.0) as u8;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channel(0),
        channel(1),
        channel(2)
    ))
}

/// `#rrggbb 40%` → color and weight.
fn color_stop(s: &str) -> Option<([f64; 3], Option<f64>)> {
    let s = s.trim();
    let (color, weight) = match s.rsplit_once(char::is_whitespace) {
        Some((c, w)) if w.ends_with('%') => (
            c.trim(),
            Some(w.trim_end_matches('%').parse::<f64>().ok()? / 100.0),
        ),
        _ => (s, None),
    };
    Some((parse_hex(color)?, weight))
}

fn parse_hex(color: &str) -> Option<[f64; 3]> {
    let hex = color.strip_prefix('#')?;
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    let rgb = match digits.len() {
        3 | 4 => [digits[0] * 17, digits[1] * 17, digits[2] * 17],
        6 | 8 => [
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
        ],
        _ => return None,
    };
    Some(rgb.map(f64::from))
}

/// Inserts a full-size background rect as the root's first child.
fn with_background(svg: &str, color: &str) -> String {
    let Some(open) = svg.find("<svg") else {
        return svg.to_string();
    };
    let mut quote = None;
    for (i, c) in svg[open..].char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('>', None) => {
                let at = open + i + 1;
                return format!(
                    "{}<rect width=\"100%\" height=\"100%\" fill=\"{color}\"/>{}",
                    &svg[..at],
                    &svg[at..]
                );
            }
            _ => {}
        }
    }
    svg.to_string()
}
