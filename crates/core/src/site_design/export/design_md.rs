//! DESIGN.md: the tokens as YAML front matter, then a section of prose for each thing the page
//! showed.

use std::fmt::Write as _;

use super::{n, plural, px, semantic};
use crate::site_design::color::STEPS;
use crate::site_design::tokens::Tokens;

/// A YAML scalar: plain when it is obviously safe, otherwise double-quoted.
fn yq(v: &str) -> String {
    let plain = !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '%' | '-'))
        && !v.starts_with('-');
    if plain || v.parse::<f64>().is_ok() {
        v.to_string()
    } else {
        quoted(v)
    }
}

fn quoted(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

fn title(name: &str) -> String {
    let s = name.replace('-', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

pub fn design_md(t: &Tokens) -> String {
    let mut out = String::from("---\nversion: alpha\n");
    let _ = writeln!(out, "name: {} design system", t.site);
    let _ = writeln!(
        out,
        "description: Extracted from {} on {} by Sawit.",
        t.url, t.date
    );
    if !t.semantic.is_empty() || !t.palette.is_empty() {
        out.push_str("colors:\n");
        for (name, c) in &t.semantic {
            let _ = writeln!(out, "  {name}: {}", quoted(&c.to_hex()));
        }
        for r in &t.palette {
            for (step, c) in STEPS.iter().zip(r.steps.iter()) {
                let _ = writeln!(out, "  {}-{step}: {}", r.role, quoted(&c.to_hex()));
            }
        }
    }
    if !t.type_roles.is_empty() {
        out.push_str("typography:\n");
        for (name, tok) in &t.type_roles {
            let _ = writeln!(out, "  {name}:");
            let _ = writeln!(out, "    fontFamily: {}", quoted(&tok.family));
            let _ = writeln!(out, "    fontSize: {}", px(tok.size));
            let _ = writeln!(out, "    fontWeight: {}", tok.weight);
            if let Some(lh) = tok.line_height {
                let _ = writeln!(out, "    lineHeight: {}", n(lh));
            }
            if let Some(ls) = &tok.letter_spacing {
                let _ = writeln!(out, "    letterSpacing: {}", yq(ls));
            }
        }
    }
    if !t.radius.is_empty() {
        out.push_str("rounded:\n");
        for (name, v) in &t.radius {
            let _ = writeln!(out, "  {name}: {}", yq(v));
        }
    }
    if !t.spacing.is_empty() {
        out.push_str("spacing:\n");
        for (name, v) in &t.spacing {
            let _ = writeln!(out, "  {name}: {}", px(*v));
        }
    }
    if !t.components.is_empty() {
        out.push_str("components:\n");
        for c in &t.components {
            let _ = writeln!(out, "  {}:", c.name);
            for (k, v) in &c.props {
                let _ = writeln!(out, "    {k}: {}", yq(v));
            }
        }
    }
    out.push_str("---\n");

    for (heading, body) in sections(t) {
        let _ = write!(out, "\n## {heading}\n\n{body}");
    }
    out
}

fn sections(t: &Tokens) -> Vec<(&'static str, String)> {
    let mut v = Vec::new();
    let f = &t.facts;
    let body_family = f.body.as_ref().map(|b| b.family.as_str());

    // Overview
    if body_family.is_some() || f.colors_seen > 0 {
        let mood = if f.dark { "dark" } else { "light" };
        let mut s = format!("A {mood} interface");
        if let Some(fam) = body_family {
            let _ = write!(s, " built on {fam}");
        }
        if f.colors_seen > 0 {
            let _ = write!(
                s,
                ", with {} in use",
                plural(f.colors_seen as u32, "color", "colors")
            );
        }
        s.push_str(".\n");
        v.push(("Overview", s));
    }

    // Colors
    let mut lines = Vec::new();
    for r in &t.palette {
        let hex = r.base.to_hex();
        let name = title(&r.role);
        if r.role == "primary" {
            let mut parts = Vec::new();
            if f.primary_buttons > 0 {
                parts.push(plural(f.primary_buttons, "button", "buttons"));
            }
            if f.primary_links > 0 {
                parts.push(plural(f.primary_links, "link", "links"));
            }
            if parts.is_empty() {
                parts.push(plural(r.uses, "element", "elements"));
            }
            lines.push(format!(
                "- **{name}** `{hex}`: used on {}; reserve it for actions.",
                parts.join(" and ")
            ));
        } else {
            lines.push(format!(
                "- **{name}** `{hex}`: seen on {}.",
                plural(r.uses, "element", "elements")
            ));
        }
    }
    for (name, c) in &t.semantic {
        if name != "primary" {
            lines.push(format!("- **{}** `{}`.", title(name), c.to_hex()));
        }
    }
    if !lines.is_empty() {
        v.push(("Colors", lines.join("\n") + "\n"));
    }

    // Typography
    let mut s = String::new();
    if let Some(b) = &f.body {
        let lh = b
            .line_height
            .map(|l| format!("/{}", n(l)))
            .unwrap_or_default();
        let _ = write!(s, "Body text is {}{lh} {}", px(b.size), b.family);
        if f.smallest_text > 0.0 {
            let _ = write!(
                s,
                "; nothing on the page is set below {}",
                px(f.smallest_text)
            );
        }
        s.push_str(".\n");
    } else if f.smallest_text > 0.0 {
        let _ = writeln!(
            s,
            "Nothing on the page is set below {}.",
            px(f.smallest_text)
        );
    }
    if !t.type_roles.is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        for (name, tok) in &t.type_roles {
            let lh = tok
                .line_height
                .map(|l| format!("/{}", n(l)))
                .unwrap_or_default();
            let _ = writeln!(
                s,
                "- **{}**: {}{lh}, weight {}, {}",
                title(name),
                px(tok.size),
                tok.weight,
                tok.family
            );
        }
    }
    if !s.is_empty() {
        v.push(("Typography", s));
    }

    // Layout
    let mut s = String::new();
    if let Some(base) = t.spacing_base {
        let _ = writeln!(s, "Spacing sits on a {} grid.", px(base));
    }
    if !t.spacing.is_empty() {
        let list: Vec<String> = t
            .spacing
            .iter()
            .map(|(k, x)| format!("{k} {}", px(*x)))
            .collect();
        let _ = writeln!(s, "\nSpacing scale: {}.", list.join(", "));
    }
    if !t.breakpoints.is_empty() {
        let list: Vec<String> = t.breakpoints.iter().map(|b| format!("{b}px")).collect();
        let _ = writeln!(s, "\nBreakpoints: {}.", list.join(", "));
    }
    let s = s.trim_start().to_string();
    if !s.is_empty() {
        v.push(("Layout", s));
    }

    // Elevation
    if !t.shadows.is_empty() {
        let s: String = t
            .shadows
            .iter()
            .map(|(k, x)| format!("- **{}**: `{x}`\n", title(k)))
            .collect();
        v.push(("Elevation & Depth", s));
    }

    // Shapes
    if !t.radius.is_empty() {
        let s: String = t
            .radius
            .iter()
            .map(|(k, x)| format!("- **{}**: {x}\n", title(k)))
            .collect();
        v.push(("Shapes", s));
    }

    // Components
    if !t.components.is_empty() {
        let s: String = t
            .components
            .iter()
            .map(|c| {
                let props: Vec<String> =
                    c.props.iter().map(|(k, x)| format!("{k} `{x}`")).collect();
                format!("- **{}**: {}.\n", c.name, props.join(", "))
            })
            .collect();
        v.push(("Components", s));
    }

    // Do's and Don'ts
    let mut s = String::new();
    if semantic(t, "primary").is_some() {
        s.push_str("- Do use `{colors.primary}` only for primary actions.\n");
    }
    if f.smallest_text > 0.0 {
        let _ = writeln!(s, "- Don't set text below {}.", px(f.smallest_text));
    }
    if let Some(base) = t.spacing_base {
        let _ = writeln!(s, "- Do keep spacing on the {} grid.", px(base));
    }
    if !s.is_empty() {
        v.push(("Do's and Don'ts", s));
    }
    v
}
