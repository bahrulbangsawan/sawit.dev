//! `spec.md`: what a handoff's frames are made of, read from the document (an artboard's layers)
//! or from its files (a screen's HTML and CSS).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use figcraft_color::PaintKind;
use figcraft_doc::proto::Action;
use figcraft_doc::{Document, Node, NodeId, NodeKind};

use super::{Files, Target};
use crate::screens;

/// Words in a frame's or layer's name that say which state it draws.
const STATES: &[&str] = &[
    "empty", "loading", "error", "success", "hover", "pressed", "focus", "disabled", "selected",
    "skeleton", "offline",
];

/// The breakpoint a frame `width` wide is drawn for.
fn breakpoint(width: f64) -> &'static str {
    match width {
        w if w < 600.0 => "mobile",
        w if w < 1024.0 => "tablet",
        _ => "desktop",
    }
}

/// What the spec gathers across the frames.
#[derive(Default)]
struct Found {
    colors: BTreeMap<String, Vec<String>>,
    type_styles: BTreeMap<String, Vec<String>>,
    gaps: BTreeMap<String, usize>,
    radii: BTreeMap<String, usize>,
    variables: BTreeMap<String, String>,
    components: BTreeMap<String, Vec<String>>,
    copy: Vec<(String, String)>,
    states: Vec<String>,
}

/// The spec of `frames` of `doc`; a screen's files are in `screens_dir`.
pub(super) fn spec(doc: &Document, frames: &[(Target, Files)], screens_dir: &Path) -> String {
    let mut out = String::new();
    let titles: Vec<&str> = frames.iter().map(|(t, _)| t.title.as_str()).collect();
    let _ = writeln!(out, "# {}: handoff spec\n", titles.join(" → "));
    out.push_str(
        "Made by Sawit from the canvas. Values are the design's as drawn; build them with the \
         project's own tokens where they match.\n\n## Frames\n\n",
    );
    for (i, (t, _)) in frames.iter().enumerate() {
        let kind = match &t.screen {
            Some(name) => format!("HTML screen `{name}`"),
            None => "artboard".into(),
        };
        let _ = writeln!(
            out,
            "{}. **{}**: {kind}, {} × {}, {}",
            i + 1,
            t.title,
            t.size.0.round(),
            t.size.1.round(),
            breakpoint(t.size.0)
        );
    }
    let links = links(doc, frames);
    if !links.is_empty() {
        out.push_str("\n## Prototype links\n\n");
        for link in links {
            let _ = writeln!(out, "- {link}");
        }
    }
    let mut found = Found::default();
    out.push_str("\n## Layer tree\n");
    for (t, _) in frames {
        let _ = writeln!(out, "\n### {}\n", t.title);
        match &t.screen {
            Some(name) => {
                let folder = screens::folder(screens_dir, name);
                let _ = writeln!(
                    out,
                    "An HTML page: its elements are in {}/index.html.",
                    folder.display()
                );
                screen_values(&folder, &mut found);
            }
            None => tree(doc, t.frame, 0, &mut out, &mut found),
        }
        if STATES.iter().any(|s| t.title.to_lowercase().contains(s)) {
            found.states.push(t.title.clone());
        }
    }
    tokens(&found, &mut out);
    out
}

/// The prototype links from one frame of the flow (or a layer in it) to another.
fn links(doc: &Document, frames: &[(Target, Files)]) -> Vec<String> {
    let ids: Vec<NodeId> = frames.iter().map(|(t, _)| t.frame).collect();
    let name = |id: NodeId| doc.node(id).map_or_else(String::new, |n| n.name.clone());
    let mut out = Vec::new();
    for (t, _) in frames {
        for id in doc.subtree(t.frame) {
            let Some(node) = doc.node(id) else { continue };
            for interaction in &node.interactions {
                for action in &interaction.actions {
                    if let Action::Navigate { dest, .. } = action
                        && ids.contains(dest)
                    {
                        let trigger = format!("{:?}", interaction.trigger);
                        out.push(format!(
                            "“{}” in {} → {} ({trigger})",
                            node.name,
                            t.title,
                            name(*dest)
                        ));
                    }
                }
            }
        }
    }
    out
}

/// Layer `id` and what is under it, as a list, `depth` deep; what it uses is noted in `found`.
fn tree(doc: &Document, id: NodeId, depth: usize, out: &mut String, found: &mut Found) {
    let Some(n) = doc.node(id) else { return };
    if !n.visible {
        return;
    }
    let pad = "  ".repeat(depth);
    let _ = writeln!(out, "{pad}- {}", describe(doc, n, found));
    if depth > 0
        && STATES.iter().any(|s| {
            n.name
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w == *s)
        })
    {
        found.states.push(n.name.clone());
    }
    if matches!(n.kind, NodeKind::Instance { .. }) || depth >= 12 {
        return;
    }
    for child in &n.children {
        tree(doc, *child, depth + 1, out, found);
    }
}

/// One layer: its name, kind, size, and what it is drawn with.
fn describe(doc: &Document, n: &Node, found: &mut Found) -> String {
    let mut parts = vec![format!("{} × {}", n.width.round(), n.height.round())];
    if let Some(layout) = &n.layout {
        parts.push(format!(
            "auto layout {:?}, gap {}, padding {}",
            layout.mode,
            layout.gap,
            layout.padding.map(|p| p.to_string()).join(" ")
        ));
        *found.gaps.entry(format!("{}", layout.gap)).or_default() += 1;
        for p in layout.padding {
            *found.gaps.entry(format!("{p}")).or_default() += 1;
        }
    }
    if let Some(r) = n.corners.uniform().filter(|r| *r > 0.0) {
        parts.push(format!("radius {r}"));
        *found.radii.entry(format!("{r}")).or_default() += 1;
    }
    for paint in n.fills.iter().filter(|p| p.visible) {
        if let PaintKind::Solid { color } = &paint.kind {
            let hex = color.to_css().to_lowercase();
            parts.push(hex.clone());
            found.colors.entry(hex).or_default().push(n.name.clone());
        }
    }
    let kind = match &n.kind {
        NodeKind::Text(t) => {
            let s = &t.style;
            let style = format!("{} {}px, weight {}", s.font_family, s.font_size, s.weight());
            found
                .type_styles
                .entry(style.clone())
                .or_default()
                .push(n.name.clone());
            found.copy.push((n.name.clone(), t.characters.clone()));
            let text: String = t.characters.chars().take(80).collect();
            parts.push(style);
            format!("text “{}”", text.replace('\n', " "))
        }
        NodeKind::Instance { component, .. } => {
            let main = doc.node(*component);
            let set = main.and_then(|m| m.parent).and_then(|p| doc.node(p));
            let set = set.filter(|s| matches!(s.kind, NodeKind::ComponentSet { .. }));
            let label = set
                .or(main)
                .map_or("a component".into(), |c| c.name.clone());
            let variant = main.map(|m| m.name.clone()).unwrap_or_default();
            found
                .components
                .entry(label.clone())
                .or_default()
                .push(variant.clone());
            format!("instance of {label} ({variant})")
        }
        other => other.label().to_lowercase(),
    };
    for (field, variable) in bound(doc, n) {
        found.variables.insert(variable, field);
    }
    format!("**{}**: {kind}, {}", n.name, parts.join(", "))
}

/// The variables bound to `n`: the field and the variable's name.
fn bound(doc: &Document, n: &Node) -> Vec<(String, String)> {
    let fields = [
        "fill",
        "fontSize",
        "fontFamily",
        "gap",
        "padding",
        "cornerRadius",
        "width",
        "height",
    ];
    fields
        .iter()
        .filter_map(|f| {
            let v = figcraft_engine::devinspect::bound_variable(doc, n, f)?;
            Some((f.to_string(), v.name.clone()))
        })
        .collect()
}

/// What a screen's CSS uses: its custom properties and colours, its fonts; its text is its copy.
fn screen_values(folder: &Path, found: &mut Found) {
    let mut css = String::new();
    let page = std::fs::read_to_string(folder.join(screens::INDEX)).unwrap_or_default();
    css.push_str(&page);
    if let Ok(entries) = std::fs::read_dir(folder.join("css")) {
        for entry in entries.flatten() {
            css.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
        }
    }
    for line in css.split([';', '{', '}']) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.starts_with("--") {
            found.variables.insert(name.to_string(), value.to_string());
        } else if name == "font-family" {
            found.type_styles.entry(value.to_string()).or_default();
        }
        for word in value.split(|c: char| c.is_whitespace() || c == ',') {
            if word.starts_with('#') && matches!(word.len(), 4 | 7 | 9) {
                found.colors.entry(word.to_lowercase()).or_default();
            }
        }
    }
}

/// The tokens, components, copy and states sections.
fn tokens(found: &Found, out: &mut String) {
    out.push_str("\n## Tokens\n\n");
    let list = |values: Vec<String>| match values.is_empty() {
        true => "none found".to_string(),
        false => values.join(", "),
    };
    let colors = found.colors.iter().map(|(c, used)| match used.len() {
        0 => c.clone(),
        n => format!("{c} ({n} layer{})", if n == 1 { "" } else { "s" }),
    });
    let _ = writeln!(out, "- Colours: {}", list(colors.collect()));
    let _ = writeln!(
        out,
        "- Type: {}",
        list(found.type_styles.keys().cloned().collect())
    );
    let _ = writeln!(
        out,
        "- Spacing (gaps and paddings): {}",
        list(found.gaps.keys().map(|g| format!("{g}px")).collect())
    );
    let _ = writeln!(
        out,
        "- Radii: {}",
        list(found.radii.keys().map(|r| format!("{r}px")).collect())
    );
    let vars: Vec<String> = found
        .variables
        .iter()
        .map(|(k, v)| format!("`{k}`: {v}"))
        .collect();
    let _ = writeln!(out, "- Variables: {}", list(vars));
    out.push_str("\n## Components and variants\n\n");
    if found.components.is_empty() {
        out.push_str("None used.\n");
    }
    for (component, variants) in &found.components {
        let _ = writeln!(out, "- {component}: {}", variants.join("; "));
    }
    out.push_str("\n## Copy\n\n");
    if found.copy.is_empty() {
        out.push_str("In the HTML of each screen.\n");
    }
    for (name, text) in &found.copy {
        let _ = writeln!(out, "- {name}: “{}”", text.replace('\n', " / "));
    }
    out.push_str("\n## States drawn\n\n");
    match found.states.is_empty() {
        true => out.push_str(
            "Only the default state is drawn: design the empty, loading and error states the \
             screen implies.\n",
        ),
        false => {
            for state in &found.states {
                let _ = writeln!(out, "- {state}");
            }
        }
    }
}
