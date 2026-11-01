//! Component tokens: the page's button, input, card and nav link samples, written with
//! references to the tokens they match.

use super::layout::{Radius, parse_radius};
use super::typography::{size_key, stack};
use super::values::{num, px, split_top};
use super::{ComponentToken, Tokens, TypeToken};
use crate::site_design::color::{self, Rgba, STEPS};
use crate::site_design::scan::{ComponentSample, Scan};

fn color_ref(t: &Tokens, c: Rgba, text_on: Option<&str>) -> String {
    let hex = c.to_hex();
    let is_fg = |n: &str| n.ends_with("foreground");
    let mut names: Vec<&str> = Vec::new();
    match text_on {
        Some(bg) => {
            let paired = format!("{bg}-foreground");
            names.extend(
                t.semantic
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| *n == paired),
            );
            names.extend(
                t.semantic
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| is_fg(n)),
            );
            names.extend(t.semantic.iter().map(|(n, _)| n.as_str()));
        }
        None => {
            names.extend(
                t.semantic
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| !is_fg(n)),
            );
            names.extend(t.semantic.iter().map(|(n, _)| n.as_str()));
        }
    }
    for n in names {
        if t.semantic.iter().any(|(m, v)| m == n && v.to_hex() == hex) {
            return format!("{{colors.{n}}}");
        }
    }
    for r in &t.palette {
        for (step, v) in STEPS.iter().zip(r.steps.iter()) {
            if v.to_hex() == hex {
                return format!("{{colors.{}-{step}}}", r.role);
            }
        }
    }
    hex
}

fn radius_ref(t: &Tokens, s: &str) -> Option<String> {
    let token = |v: &str| {
        t.radius
            .iter()
            .find(|(_, x)| x == v)
            .map(|(n, _)| format!("{{rounded.{n}}}"))
    };
    match parse_radius(s)? {
        Radius::Full => Some(token("9999px").unwrap_or_else(|| "9999px".to_string())),
        Radius::Px(v) => {
            let v = format!("{}px", num(v, 2));
            Some(token(&v).unwrap_or(v))
        }
    }
}

fn typography_ref(t: &Tokens, c: &ComponentSample) -> Option<String> {
    let entries: Vec<String> = stack(&c.style.font_family)
        .into_iter()
        .map(|(e, _)| e.to_ascii_lowercase())
        .collect();
    let fits = |tok: &TypeToken| {
        entries.contains(&tok.family.to_ascii_lowercase())
            && size_key(tok.size) == size_key(c.style.font_size)
            && tok.weight == c.style.font_weight
    };
    let label = t.type_roles.iter().filter(|(n, _)| n == "label");
    label
        .chain(t.type_roles.iter())
        .find(|(_, tok)| fits(tok))
        .map(|(n, _)| format!("{{typography.{n}}}"))
}

pub(super) fn derive_components(scan: &Scan, t: &Tokens) -> Vec<ComponentToken> {
    let mut out = Vec::new();
    for (kind, name) in [
        ("button", "button-primary"),
        ("button-secondary", "button-secondary"),
        ("input", "input"),
        ("card", "card"),
        ("link", "nav-link"),
    ] {
        let Some(c) = scan.components.iter().find(|c| c.kind == kind) else {
            continue;
        };
        let st = &c.style;
        let mut props: Vec<(String, String)> = Vec::new();
        let mut bg_name = None;
        if let Some(bg) = color::parse(&st.background).filter(|c| c.a > 0.0) {
            let r = color_ref(t, bg, None);
            bg_name = r
                .strip_prefix("{colors.")
                .and_then(|r| r.strip_suffix('}'))
                .map(str::to_string);
            props.push(("backgroundColor".into(), r));
        }
        if let Some(fg) = color::parse(&st.color).filter(|c| c.a > 0.0) {
            let on = bg_name.as_deref().unwrap_or("");
            props.push(("textColor".into(), color_ref(t, fg, Some(on))));
        }
        if let Some(r) = typography_ref(t, c) {
            props.push(("typography".into(), r));
        }
        if let Some(r) = radius_ref(t, &st.radius) {
            props.push(("rounded".into(), r));
        }
        let padding = st.padding.trim();
        if !padding.is_empty()
            && split_top(padding, char::is_whitespace)
                .iter()
                .any(|p| px(p) != Some(0.0))
        {
            props.push(("padding".into(), padding.to_string()));
        }
        if st.height > 0.0 {
            props.push(("height".into(), format!("{}px", num(st.height, 2))));
        }
        if !props.is_empty() {
            out.push(ComponentToken {
                name: name.to_string(),
                props,
            });
        }
    }
    out
}
