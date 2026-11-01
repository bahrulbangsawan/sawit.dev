//! CSS values: lists split outside parentheses, px lengths and box-shadows read, and numbers
//! written.

use super::Shadow;
use crate::site_design::color;

/// The first shadow of a `box-shadow` list.
pub fn parse_box_shadow(s: &str) -> Option<Shadow> {
    let first = split_top(s, |c| c == ',').into_iter().next()?;
    let mut lens = Vec::new();
    let mut color = None;
    let mut inset = false;
    for tok in split_top(first, char::is_whitespace) {
        if tok.eq_ignore_ascii_case("inset") {
            inset = true;
        } else if let Some(v) = px(tok) {
            lens.push(v);
        } else if color.is_none() {
            color = Some(color::parse(tok)?);
        } else {
            return None;
        }
    }
    if !(2..=4).contains(&lens.len()) {
        return None;
    }
    Some(Shadow {
        color: color?,
        x: lens[0],
        y: lens[1],
        blur: lens.get(2).copied().unwrap_or(0.0),
        spread: lens.get(3).copied().unwrap_or(0.0),
        inset,
    })
}

/// Splits at `sep` outside parentheses, trimming and dropping empty pieces.
pub(super) fn split_top(s: &str, sep: impl Fn(char) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if depth == 0 && sep(c) => {
                out.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// `12px`, `0` → px.
pub(super) fn px(tok: &str) -> Option<f64> {
    let v = if tok == "0" {
        0.0
    } else {
        tok.strip_suffix("px")?.parse::<f64>().ok()?
    };
    v.is_finite().then_some(v)
}

/// A number with at most `dp` decimals and no trailing zeros.
pub(super) fn num(v: f64, dp: usize) -> String {
    let s = format!("{v:.dp$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    if s == "-0" { "0".into() } else { s.to_string() }
}
