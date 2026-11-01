//! The export's CSS: a Tailwind v4 `@theme` block, shadcn/ui variables, and the site's own
//! custom properties.

use std::fmt::Write as _;

use super::{px, semantic};
use crate::site_design::color::STEPS;
use crate::site_design::tokens::Tokens;

const NOT_SEEN: [&str; 7] = [
    "destructive",
    "chart-1",
    "chart-2",
    "chart-3",
    "chart-4",
    "chart-5",
    "sidebar",
];
const BREAKPOINTS: [&str; 5] = ["sm", "md", "lg", "xl", "2xl"];

pub fn tailwind_theme(t: &Tokens) -> String {
    let mut out = String::from("@theme {\n");
    for r in &t.palette {
        for (step, c) in STEPS.iter().zip(r.steps.iter()) {
            let _ = writeln!(out, "  --color-{}-{step}: {};", r.role, c.to_hex());
        }
    }
    for (name, c) in &t.semantic {
        let _ = writeln!(out, "  --color-{name}: {};", c.to_hex());
    }
    for (name, stack) in &t.families {
        let _ = writeln!(out, "  --font-{name}: {stack};");
    }
    for (name, v) in &t.type_scale {
        let _ = writeln!(out, "  --text-{name}: {};", px(*v));
    }
    if let Some(base) = t.spacing_base {
        let _ = writeln!(out, "  --spacing: {};", px(base));
    }
    for (name, v) in &t.radius {
        let _ = writeln!(out, "  --radius-{name}: {v};");
    }
    for (name, v) in &t.shadows {
        let _ = writeln!(out, "  --shadow-{name}: {v};");
    }
    for (name, b) in BREAKPOINTS.iter().zip(t.breakpoints.iter()) {
        let _ = writeln!(out, "  --breakpoint-{name}: {b}px;");
    }
    out.push_str("}\n");
    out
}

pub fn shadcn_css(t: &Tokens) -> String {
    let mut lines = String::new();
    let mut skipped: Vec<&str> = Vec::new();
    let mut emit = |name: &'static str, value: Option<String>| match value {
        Some(v) => {
            let _ = writeln!(lines, "  --{name}: {v};");
        }
        None => skipped.push(name),
    };
    let css = |n: &str| semantic(t, n).map(|c| c.oklch_css());
    emit(
        "radius",
        t.radius
            .iter()
            .find(|(k, _)| k == "md")
            .map(|(_, v)| v.clone()),
    );
    emit("background", css("background"));
    emit("foreground", css("foreground"));
    emit("card", css("background"));
    emit("card-foreground", css("foreground"));
    emit("popover", css("background"));
    emit("popover-foreground", css("foreground"));
    emit("primary", css("primary"));
    emit("primary-foreground", css("primary-foreground"));
    emit("secondary", css("muted"));
    emit("secondary-foreground", css("foreground"));
    emit("muted", css("muted"));
    emit("muted-foreground", css("muted-foreground"));
    emit("accent", css("muted"));
    emit("accent-foreground", css("foreground"));
    emit("border", css("border"));
    emit("input", css("border"));
    emit("ring", css("ring"));
    let mut unseen: Vec<&str> = NOT_SEEN.to_vec();
    unseen.extend(skipped);
    format!(
        ":root {{\n{lines}}}\n\n/* Not seen on the page: {} — keep your theme's values. */\n",
        unseen.join(", ")
    )
}

pub fn variables_css(vars: &[(String, String)]) -> String {
    let mut out = String::from(":root {\n");
    for (k, v) in vars {
        let _ = writeln!(out, "  {k}: {v};");
    }
    out.push_str("}\n");
    out
}
