//! The files of a design export. Everything written traces to something the page showed: a
//! section, token or shadcn variable with no source is left out.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::color::STEPS;
use super::scan::{AssetOutcome, Scan};
use super::tokens::{Tokens, parse_box_shadow};

const LICENSE: &str = "Assets and fonts belong to their owners; check licenses before shipping.";
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

/// `17` → `17`, `1.470` → `1.47`.
fn n(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn px(v: f64) -> String {
    format!("{}px", n(v))
}

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

fn plural(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn semantic<'a>(t: &'a Tokens, name: &str) -> Option<&'a super::color::Rgba> {
    t.semantic.iter().find(|(s, _)| s == name).map(|(_, c)| c)
}

// ---------- DESIGN.md ----------

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

// ---------- DTCG ----------

fn token(ty: &str, value: Value) -> Value {
    json!({ "$type": ty, "$value": value })
}

fn put(root: &mut Map<String, Value>, group: &str, key: &str, v: Value) {
    let g = root
        .entry(group.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(m) = g {
        m.insert(key.to_string(), v);
    }
}

pub fn dtcg(t: &Tokens) -> Value {
    let mut root = Map::new();
    let mut color = Map::new();
    for r in &t.palette {
        let mut steps = Map::new();
        for (step, c) in STEPS.iter().zip(r.steps.iter()) {
            steps.insert(step.to_string(), token("color", json!(c.to_hex())));
        }
        color.insert(r.role.clone(), Value::Object(steps));
    }
    for (name, c) in &t.semantic {
        let tok = token("color", json!(c.to_hex()));
        match color.get_mut(name) {
            // A ramp of the same name is a group; DTCG keeps the group's own token in `$root`.
            Some(Value::Object(group)) => {
                group.insert("$root".into(), tok);
            }
            _ => {
                color.insert(name.clone(), tok);
            }
        }
    }
    if !color.is_empty() {
        root.insert("color".into(), Value::Object(color));
    }
    if !t.families.is_empty() || !t.type_scale.is_empty() {
        let mut font = Map::new();
        let family: Map<String, Value> = t
            .families
            .iter()
            .map(|(k, v)| (k.clone(), token("fontFamily", json!(v))))
            .collect();
        if !family.is_empty() {
            font.insert("family".into(), Value::Object(family));
        }
        let size: Map<String, Value> = t
            .type_scale
            .iter()
            .map(|(k, v)| (k.clone(), token("dimension", json!(px(*v)))))
            .collect();
        if !size.is_empty() {
            font.insert("size".into(), Value::Object(size));
        }
        root.insert("font".into(), Value::Object(font));
    }
    for (k, v) in &t.spacing {
        put(&mut root, "spacing", k, token("dimension", json!(px(*v))));
    }
    for (k, v) in &t.radius {
        put(&mut root, "radius", k, token("dimension", json!(v)));
    }
    for (k, raw) in &t.shadows {
        let Some(s) = parse_box_shadow(raw) else {
            continue;
        };
        let value = json!({
            "color": s.color.to_hex(),
            "offsetX": px(s.x),
            "offsetY": px(s.y),
            "blur": px(s.blur),
            "spread": px(s.spread),
        });
        put(&mut root, "shadow", k, token("shadow", value));
    }
    Value::Object(root)
}

// ---------- CSS ----------

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

// ---------- README ----------

fn describe(file: &str) -> Option<&'static str> {
    Some(match file {
        "DESIGN.md" => "the design system as Google Labs DESIGN.md (YAML tokens plus prose)",
        "tokens.json" => "design tokens in the DTCG format",
        "theme.css" => "a Tailwind v4 @theme block",
        "shadcn.css" => "shadcn/ui variables in oklch(), only those the page showed",
        "variables.css" => "the site's own custom properties, as found",
        "screenshot.png" => "the page as shown at export",
        "assets.json" => "every asset found, with where it was used and any error",
        "README.md" => "this file",
        _ => return None,
    })
}

pub fn readme(t: &Tokens, files: &[String]) -> String {
    let mut out = format!(
        "# {} design export\n\nSource: {}\nDate: {}\nExtracted by Sawit.\n\n## Files\n\n",
        t.site, t.url, t.date
    );
    let mut assets = 0;
    for f in files {
        if let Some(d) = describe(f) {
            let _ = writeln!(out, "- `{f}`: {d}");
        } else if f.starts_with("components/") {
            let _ = writeln!(
                out,
                "- `{f}`: one detected sample, outerHTML with computed styles inlined"
            );
        } else if f.starts_with("assets/") {
            assets += 1;
        }
    }
    if assets > 0 {
        let _ = writeln!(
            out,
            "- `assets/`: {} saved from the page (images, svg, icons, fonts, video, lottie)",
            plural(assets, "file", "files")
        );
    }
    let _ = write!(out, "\n{LICENSE}\n");
    out
}

// ---------- folder ----------

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = format!("{prefix}{}", e.file_name().to_string_lossy());
        if e.file_type()?.is_dir() {
            walk(&e.path(), &format!("{name}/"), out)?;
        } else {
            out.push(name);
        }
    }
    Ok(())
}

fn safe_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Replaces the export in `dir`, keeping `assets/` (the app refills it before this call).
pub fn write_folder(
    dir: &Path,
    t: &Tokens,
    scan: &Scan,
    screenshot_png: Option<&[u8]>,
    assets: &[AssetOutcome],
) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_name() == "assets" {
            continue;
        }
        if e.file_type()?.is_dir() {
            std::fs::remove_dir_all(e.path())?;
        } else {
            std::fs::remove_file(e.path())?;
        }
    }
    let write = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes);
    write("DESIGN.md", design_md(t).as_bytes())?;
    let tokens = serde_json::to_string_pretty(&dtcg(t)).map_err(std::io::Error::other)?;
    write("tokens.json", tokens.as_bytes())?;
    write("theme.css", tailwind_theme(t).as_bytes())?;
    write("shadcn.css", shadcn_css(t).as_bytes())?;
    write("variables.css", variables_css(&t.variables).as_bytes())?;
    if let Some(png) = screenshot_png {
        write("screenshot.png", png)?;
    }
    for c in scan.components.iter().filter(|c| safe_kind(&c.kind)) {
        std::fs::create_dir_all(dir.join("components"))?;
        write(&format!("components/{}.html", c.kind), c.html.as_bytes())?;
    }
    let manifest = serde_json::to_string_pretty(assets).map_err(std::io::Error::other)?;
    write("assets.json", manifest.as_bytes())?;

    // README lists the files, itself included.
    std::fs::write(dir.join("README.md"), "")?;
    let mut files = Vec::new();
    walk(dir, "", &mut files)?;
    files.sort();
    write("README.md", readme(t, &files).as_bytes())?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site_design::tokens::derive;

    fn scan() -> Scan {
        serde_json::from_str(include_str!("fixtures/scan.json")).unwrap()
    }

    fn fixture() -> Tokens {
        derive(&scan(), "2026-10-30")
    }

    fn mono() -> Tokens {
        let s: Scan = serde_json::from_str(include_str!("fixtures/mono.json")).unwrap();
        derive(&s, "2026-10-30")
    }

    #[test]
    fn design_md_front_matter() {
        let md = design_md(&fixture());
        assert!(md.starts_with(
            "---\nversion: alpha\nname: Apple design system\ndescription: Extracted from "
        ));
        assert!(md.contains("colors:\n  background: \"#ffffff\""));
        assert!(md.contains("  primary-600: \"#0071e3\""));
        assert!(md.contains("rounded:\n  sm: 4px"));
        assert!(
            md.contains(
                "components:\n  button-primary:\n    backgroundColor: \"{colors.primary}\""
            )
        );
    }

    #[test]
    fn design_md_sections_in_order() {
        let t = fixture();
        let md = design_md(&t);
        let heads = [
            "## Overview",
            "## Colors",
            "## Typography",
            "## Layout",
            "## Elevation & Depth",
            "## Shapes",
            "## Components",
            "## Do's and Don'ts",
        ];
        let idx: Vec<usize> = heads.iter().map(|h| md.find(h).expect(h)).collect();
        assert!(idx.windows(2).all(|w| w[0] < w[1]), "{idx:?}");
        let flat = Tokens {
            shadows: vec![],
            ..t
        };
        assert!(!design_md(&flat).contains("## Elevation & Depth"));
    }

    #[test]
    fn design_md_prose() {
        let md = design_md(&fixture());
        assert!(
            md.contains("- **Primary** `#0071e3`: used on 12 buttons; reserve it for actions.")
        );
        assert!(md.contains(
            "Body text is 17px/1.47 SF Pro Text; nothing on the page is set below 12px."
        ));
        assert!(md.contains("A light interface built on SF Pro Text, with 9 colors in use."));
        assert!(md.contains("- Do use `{colors.primary}` only for primary actions."));
        assert!(md.contains("- Don't set text below 12px."));
        assert!(md.contains("- Do keep spacing on the 4px grid."));
    }

    #[test]
    fn mono_design_md_omits_what_was_not_seen() {
        let md = design_md(&mono());
        assert!(!md.contains("Do use `{colors.primary}`"));
        assert!(!md.contains("## Shapes") && !md.contains("keep spacing"));
    }

    #[test]
    fn dtcg_shape() {
        let v = dtcg(&fixture());
        assert_eq!(v["color"]["primary"]["600"]["$value"], "#0071e3");
        assert_eq!(v["color"]["primary"]["600"]["$type"], "color");
        assert_eq!(v["color"]["background"]["$type"], "color");
        assert_eq!(v["font"]["size"]["base"]["$value"], "17px");
        assert_eq!(v["shadow"]["md"]["$value"]["blur"], "8px");
        assert_eq!(v["radius"]["sm"]["$value"], "4px");
    }

    #[test]
    fn tailwind_theme_lines() {
        let css = tailwind_theme(&fixture());
        for l in [
            "--color-primary-600: #0071e3;",
            "--text-base: 17px;",
            "--spacing: 4px;",
            "--radius-full: 9999px;",
            "--breakpoint-sm: 734px;",
            "--breakpoint-md: 1068px;",
        ] {
            assert!(css.contains(l), "{l}");
        }
    }

    #[test]
    fn shadcn_lines() {
        let css = shadcn_css(&fixture());
        assert!(css.contains("--primary: oklch(0.5629 0.1933 256.16);"));
        assert!(css.contains("--background: oklch(1 0 0);"));
        assert!(css.contains("Not seen on the page: destructive"));
        let m = shadcn_css(&mono());
        assert!(!m.contains("--primary:"));
        let note = m.split("Not seen on the page:").nth(1).unwrap();
        assert!(note.contains("primary"));
    }

    #[test]
    fn variables_css_lists_them() {
        assert_eq!(
            variables_css(&[("--a".into(), "1px".into())]),
            ":root {\n  --a: 1px;\n}\n"
        );
    }

    #[test]
    fn readme_has_the_license_line() {
        let r = readme(&fixture(), &["DESIGN.md".into()]);
        assert!(
            r.contains("Assets and fonts belong to their owners; check licenses before shipping.")
        );
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sawit-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn write_folder_replaces_old_export() {
        let dir = temp("replace");
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("old.txt"), "x").unwrap();
        std::fs::write(dir.join("assets/x.png"), "x").unwrap();
        let files = write_folder(&dir, &fixture(), &scan(), Some(b"png"), &[]).unwrap();
        assert!(!dir.join("old.txt").exists());
        assert!(dir.join("assets/x.png").exists());
        assert!(files.contains(&"assets/x.png".to_string()));
        assert!(files.contains(&"screenshot.png".to_string()));
        assert!(files.iter().any(|f| f.starts_with("components/")));
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);
        for f in &files {
            assert!(dir.join(f).exists(), "{f}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_folder_without_assets() {
        let dir = temp("bare");
        let files = write_folder(&dir, &fixture(), &scan(), None, &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("assets.json")).unwrap(),
            "[]"
        );
        assert!(!files.contains(&"screenshot.png".to_string()));
        for f in [
            "DESIGN.md",
            "tokens.json",
            "theme.css",
            "shadcn.css",
            "variables.css",
            "README.md",
        ] {
            assert!(files.contains(&f.to_string()), "{f}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
