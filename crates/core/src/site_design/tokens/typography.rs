//! Type: the text roles from display to caption, the size scale around the body text, and the
//! font families.

use super::TypeToken;
use super::values::{num, split_top};
use crate::site_design::scan::{Scan, TextSample};

const BODY_TAGS: [&str; 5] = ["p", "li", "dd", "td", "span"];
const HEADINGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];

const GENERICS: [&str; 13] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];

pub(super) struct Type {
    pub(super) families: Vec<(String, String)>,
    pub(super) roles: Vec<(String, TypeToken)>,
    pub(super) scale: Vec<(String, f64)>,
    pub(super) smallest: f64,
    pub(super) body: Option<TypeToken>,
}

/// Size key for "distinct sizes": hundredths of a px.
pub(super) fn size_key(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

fn heaviest<'a>(it: impl Iterator<Item = &'a TextSample>) -> Option<&'a TextSample> {
    it.fold(None, |best: Option<&TextSample>, s| match best {
        Some(b) if b.chars >= s.chars => Some(b),
        _ => Some(s),
    })
}

/// Stack entries, unquoted, with whether each was quoted.
pub(super) fn stack(family: &str) -> Vec<(String, bool)> {
    split_top(family, |c| c == ',')
        .into_iter()
        .map(|e| {
            let quoted = e.starts_with('"') || e.starts_with('\'');
            (e.trim_matches(['"', '\'']).trim().to_string(), quoted)
        })
        .filter(|(e, _)| !e.is_empty())
        .collect()
}

fn last_generic(family: &str) -> Option<String> {
    stack(family)
        .into_iter()
        .rev()
        .find(|(e, quoted)| !quoted && GENERICS.contains(&e.to_ascii_lowercase().as_str()))
        .map(|(e, _)| e.to_ascii_lowercase())
}

fn type_token(s: &TextSample) -> TypeToken {
    let family = s
        .used_family
        .clone()
        .filter(|f| !f.trim().is_empty())
        .or_else(|| stack(&s.family).into_iter().next().map(|(e, _)| e))
        .unwrap_or_default();
    let ratio = |v: f64| (s.size > 0.0).then(|| v / s.size);
    TypeToken {
        family,
        size: s.size,
        weight: s.weight,
        line_height: s
            .line_height
            .filter(|v| *v > 0.0)
            .and_then(ratio)
            .map(|r| (r * 100.0).round() / 100.0),
        letter_spacing: s
            .letter_spacing
            .and_then(ratio)
            .map(|r| (r * 1000.0).round() / 1000.0)
            .filter(|r| *r != 0.0)
            .map(|r| format!("{}em", num(r, 3))),
    }
}

pub(super) fn derive_type(scan: &Scan) -> Type {
    let texts: Vec<&TextSample> = scan.texts.iter().filter(|s| s.size > 0.0).collect();
    let tagged = |tags: &[&str]| -> Option<&TextSample> {
        heaviest(
            texts
                .iter()
                .copied()
                .filter(|s| tags.contains(&s.tag.as_str())),
        )
    };
    let body_main = heaviest(
        texts
            .iter()
            .copied()
            .filter(|s| s.in_main && BODY_TAGS.contains(&s.tag.as_str())),
    );
    let body = body_main.or_else(|| tagged(&BODY_TAGS));

    let mut picks: Vec<(&str, &TextSample)> = Vec::new();
    for (role, tags) in [
        ("display", &["h1"][..]),
        ("headline-lg", &["h2"][..]),
        ("headline-md", &["h3"][..]),
        ("title", &["h4"][..]),
    ] {
        if let Some(s) = tagged(tags) {
            picks.push((role, s));
        }
    }
    if let Some(b) = body {
        let body_size = size_key(b.size);
        let plain = || {
            texts
                .iter()
                .copied()
                .filter(|s| !HEADINGS.contains(&s.tag.as_str()))
        };
        let above = plain()
            .map(|s| size_key(s.size))
            .filter(|k| *k > body_size)
            .min();
        let below = plain()
            .map(|s| size_key(s.size))
            .filter(|k| *k < body_size)
            .max();
        let at =
            |k: Option<i64>| k.and_then(|k| heaviest(plain().filter(|s| size_key(s.size) == k)));
        if let Some(s) = at(above) {
            picks.push(("body-lg", s));
        }
        picks.push(("body-md", b));
        if let Some(s) = at(below) {
            picks.push(("body-sm", s));
        }
    }
    for (role, tags) in [
        ("label", &["button", "label"][..]),
        ("caption", &["small", "figcaption"][..]),
    ] {
        if let Some(s) = tagged(tags) {
            picks.push((role, s));
        }
    }
    let roles: Vec<(String, TypeToken)> = picks
        .iter()
        .map(|(r, s)| (r.to_string(), type_token(s)))
        .collect();
    let body_token = body.map(type_token);

    let mut sizes: Vec<f64> = Vec::new();
    for s in &texts {
        if !sizes.iter().any(|v| size_key(*v) == size_key(s.size)) {
            sizes.push(s.size);
        }
    }
    sizes.sort_by(|a, b| a.total_cmp(b));
    let mut scale = Vec::new();
    if let Some(i) = body.and_then(|b| sizes.iter().position(|v| size_key(*v) == size_key(b.size)))
    {
        let below: Vec<(&str, f64)> = ["sm", "xs"]
            .into_iter()
            .zip(sizes[..i].iter().rev().copied())
            .collect();
        scale.extend(below.into_iter().rev().map(|(n, v)| (n.to_string(), v)));
        scale.push(("base".to_string(), sizes[i]));
        scale.extend(
            ["lg", "xl", "2xl", "3xl", "4xl"]
                .into_iter()
                .zip(sizes[i + 1..].iter().copied())
                .map(|(n, v)| (n.to_string(), v)),
        );
    }

    let mut families = Vec::new();
    if let (Some(b), Some(bt)) = (body, &body_token) {
        let role = match last_generic(&b.family).as_deref() {
            Some("serif" | "ui-serif") => "serif",
            Some("monospace" | "ui-monospace") => "mono",
            _ => "sans",
        };
        families.push((role.to_string(), b.family.clone()));
        if let Some(h1) = tagged(&["h1"]).filter(|h1| type_token(h1).family != bt.family) {
            families.push(("display".to_string(), h1.family.clone()));
        }
    }
    if !families.iter().any(|(r, _)| r == "mono") {
        let mut stacks: Vec<(&str, u64)> = Vec::new();
        for s in texts.iter().filter(|s| {
            s.family
                .trim()
                .trim_end_matches(['"', '\''])
                .to_ascii_lowercase()
                .ends_with("monospace")
        }) {
            match stacks.iter_mut().find(|(f, _)| *f == s.family) {
                Some((_, n)) => *n += u64::from(s.chars),
                None => stacks.push((&s.family, u64::from(s.chars))),
            }
        }
        let best = stacks.iter().fold(None::<(&str, u64)>, |b, x| match b {
            Some(b) if b.1 >= x.1 => Some(b),
            _ => Some(*x),
        });
        if let Some((f, _)) = best {
            families.push(("mono".to_string(), f.to_string()));
        }
    }

    Type {
        families,
        roles,
        scale,
        smallest: texts.iter().map(|s| s.size).reduce(f64::min).unwrap_or(0.0),
        body: body_token,
    }
}
