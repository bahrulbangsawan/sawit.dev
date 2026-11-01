//! The markdown that carries annotations to an agent: a section per page, a line per detail,
//! and the page's own text kept to data.

use super::{Annotation, Intent, Neighbour, PageGroup, PageInfo, Rect};
use crate::browser::ConsoleEntry;

/// Text longer than this is cut with `…`.
const TEXT_CUT: usize = 100;
/// HTML longer than this is cut (only the `copy` intent includes it).
const HTML_CUT: usize = 20_000;

/// The annotations as markdown for an agent. It is pasted into the agent's prompt as the
/// user's own words, so what the page wrote (its title, an element's id, classes, text or
/// source, console errors) is kept to data: one line each, code spans that its backticks
/// can't close, and console errors in a fenced block.
pub fn markdown(groups: &[PageGroup], errors: Option<(&str, &[ConsoleEntry])>) -> String {
    let mut sections: Vec<String> = Vec::new();
    for g in groups {
        let mut out = format!("# Visual feedback — {}\n", one_line(&g.page.title));
        let (w, h) = g.page.viewport;
        out.push_str(&format!("**URL:** {}", one_line(&g.page.url)));
        if let Some(route) = route(&g.page.url) {
            out.push_str(&format!(" · **Route:** {}", code(&route)));
        }
        out.push_str(&format!(" · **Viewport:** {w}×{h}"));
        if let Some(device) = g.page.device.as_deref().filter(|d| !d.is_empty()) {
            out.push_str(&format!(" ({})", one_line(device)));
        }
        out.push('\n');
        for item in &g.items {
            out.push('\n');
            out.push_str(&item_markdown(item, &g.page));
        }
        sections.push(out);
    }
    if let Some((url, list)) = errors.filter(|(_, l)| !l.is_empty()) {
        let mut out = format!("## Console errors on {} ({})\n", one_line(url), list.len());
        let body: Vec<String> = list.iter().map(error_lines).collect();
        out.push_str(&fenced("text", &body.join("\n")));
        sections.push(out);
    }
    sections.join("\n")
}

/// One console error: `+12.3s <first line> — app.js:88`, its other lines indented, so
/// none starts like markdown (or ends the block).
fn error_lines(e: &ConsoleEntry) -> String {
    let text = e
        .text
        .replace(['\r', '\u{85}', '\u{2028}', '\u{2029}'], "\n");
    let mut lines = text.split('\n');
    let first = lines.next().unwrap_or_default().trim_end();
    let mut out = format!("+{:.1}s {first}{}", e.time_ms / 1000., error_origin(e));
    for line in lines {
        out.push('\n');
        if !line.trim().is_empty() {
            out.push_str("    ");
            out.push_str(line.trim_end());
        }
    }
    out
}

/// Page text as one line: line breaks and other control characters become spaces.
fn one_line(text: &str) -> String {
    text.split(|c: char| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The longest run of backticks in `text`.
fn longest_ticks(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// `text` as an inline code span on one line, between more backticks than it holds.
fn code(text: &str) -> String {
    let text = one_line(text);
    let fence = "`".repeat(longest_ticks(&text) + 1);
    if text.starts_with('`') || text.ends_with('`') {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

/// `body` as a fenced block whose fence is longer than any backtick run in it.
fn fenced(info: &str, body: &str) -> String {
    let fence = "`".repeat(longest_ticks(body).max(2) + 1);
    format!("{fence}{info}\n{body}\n{fence}\n")
}

fn error_origin(e: &ConsoleEntry) -> String {
    let Some(url) = e.url.as_deref().filter(|u| !u.is_empty()) else {
        return String::new();
    };
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path);
    let name = one_line(name);
    match e.line {
        Some(line) => format!(" — {name}:{line}"),
        None => format!(" — {name}"),
    }
}

/// The path (with its query) of a URL, as a router names the page: `/cart?step=2`. `None`
/// for a URL with no host part (`about:blank`, `data:`).
pub(super) fn route(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let rest = rest.split('#').next().unwrap_or(rest);
    Some(match rest.find(['/', '?']) {
        Some(i) if rest[i..].starts_with('/') => rest[i..].to_string(),
        Some(i) => format!("/{}", &rest[i..]),
        None => "/".to_string(),
    })
}

/// `Rect` as `120×40 at (350, 680)`, rounded.
fn box_at(r: &Rect) -> String {
    format!(
        "{}×{} at ({}, {})",
        r.width.round(),
        r.height.round(),
        r.x.round(),
        r.y.round()
    )
}

/// A neighbour as the Context line names it: `` `div.actions` "Read the notes" ``.
fn neighbour(n: &Neighbour) -> String {
    let mut out = code(&n.label);
    if let Some(text) = n.text.as_deref().map(one_line).filter(|t| !t.is_empty()) {
        out.push_str(&format!(" \"{}\"", cut_at(&text, NEIGHBOUR_CUT)));
    }
    out
}

/// The attributes the Element line shows besides `id` and `class`, each cut short.
const ATTRIBUTE_CUT: usize = 120;
/// A neighbour's text, cut short: it only has to say which one it is.
const NEIGHBOUR_CUT: usize = 40;
/// Attributes whose value is a URL, whose secrets are hidden.
const URL_ATTRIBUTES: [&str; 5] = ["href", "src", "action", "formaction", "poster"];

fn item_markdown(a: &Annotation, page: &PageInfo) -> String {
    let el = &a.element;
    let mut out = format!("## {}. {} — {}\n", a.n, code(&el.label), a.intent.label());
    // The user's own words, as typed.
    if !a.comment.trim().is_empty() {
        out.push_str(&format!("- **Comment:** {}\n", a.comment));
    }
    out.push_str(&format!("- **Selector:** {}\n", code(&el.selector)));
    if let Some(path) = el.path.as_deref().filter(|p| !p.trim().is_empty()) {
        out.push_str(&format!("- **Path:** {}\n", code(path)));
    }

    let mut tag = format!("<{}", el.tag);
    if let Some(id) = el.id.as_deref().filter(|i| !i.is_empty()) {
        tag.push_str(&format!(" id=\"{id}\""));
    }
    if !el.classes.is_empty() {
        tag.push_str(&format!(" class=\"{}\"", el.classes.join(" ")));
    }
    for (name, value) in el
        .attributes
        .iter()
        .filter(|(name, _)| name != "id" && name != "class")
    {
        // A link's token or password stays out of the agent's prompt, as in the network log.
        let value = if URL_ATTRIBUTES.contains(&name.as_str()) {
            crate::browser::redact_url(value)
        } else {
            value.clone()
        };
        tag.push_str(&format!(
            " {}=\"{}\"",
            one_line(name),
            cut_at(&one_line(&value), ATTRIBUTE_CUT)
        ));
    }
    tag.push('>');
    out.push_str(&format!("- **Element:** {}", code(&tag)));
    if let Some(text) = el.text.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(&format!(" \"{}\"", cut_text(&one_line(text))));
    }
    out.push('\n');
    if let Some(role) = el.role.as_deref().map(one_line).filter(|r| !r.is_empty()) {
        out.push_str(&format!("- **Role:** {role}"));
        if let Some(name) = el.name.as_deref().map(one_line).filter(|n| !n.is_empty()) {
            out.push_str(&format!(" \"{}\"", cut_text(&name)));
        }
        out.push('\n');
    }

    if let Some(src) = &el.source {
        let component = src
            .component
            .as_deref()
            .map(one_line)
            .filter(|c| !c.is_empty());
        let framework = one_line(&src.framework);
        if !src.file.is_empty() {
            let mut at = src.file.clone();
            if let Some(line) = src.line {
                at.push_str(&format!(":{line}"));
            }
            let mut who = framework;
            if let Some(c) = component {
                who.push_str(&format!(", <{c}>"));
            }
            out.push_str(&format!("- **Source:** {} ({who})\n", code(&at)));
        } else if let Some(c) = component {
            out.push_str(&format!("- **Source:** <{c}> ({framework})\n"));
        }
    }

    let r = &el.rect;
    out.push_str(&format!("- **Box:** {}", box_at(r)));
    // Where it is on the page too, once the page has scrolled.
    if let Some(p) = el
        .page_rect
        .as_ref()
        .filter(|p| p.x.round() != r.x.round() || p.y.round() != r.y.round())
    {
        out.push_str(&format!(
            " in the viewport, ({}, {}) on the page",
            p.x.round(),
            p.y.round()
        ));
    }
    // The viewport it was picked in, when the header's is another.
    if let Some((w, h)) = el.viewport.filter(|v| *v != page.viewport && v.0 > 0) {
        out.push_str(&format!(", picked in a {w}×{h} viewport"));
    }
    out.push('\n');
    if !el.styles.is_empty() {
        let styles: Vec<String> = el
            .styles
            .iter()
            .map(|(k, v)| format!("{}: {}", one_line(k), one_line(v)))
            .collect();
        out.push_str(&format!("- **Styles:** {}\n", styles.join("; ")));
    }
    if let Some(cx) = &el.context {
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = cx.parent.as_ref().filter(|p| !p.label.is_empty()) {
            parts.push(format!("in {}", code(&p.label)));
        }
        if let Some(b) = cx.before.as_ref().filter(|b| !b.label.is_empty()) {
            parts.push(format!("after {}", neighbour(b)));
        }
        if let Some(n) = cx.after.as_ref().filter(|n| !n.label.is_empty()) {
            parts.push(format!("before {}", neighbour(n)));
        }
        if !parts.is_empty() {
            out.push_str(&format!("- **Context:** {}\n", parts.join(" · ")));
        }
    }
    // A hash route (`#/cart`) isn't part of the header's URL, which groups the page's notes.
    if let Some((_, fragment)) = a.url.split_once('#').filter(|(_, f)| !f.is_empty()) {
        let at = format!("{}#{fragment}", route(&a.url).unwrap_or_default());
        out.push_str(&format!("- **Route:** {}\n", code(&at)));
    }
    if let (true, Some(path)) = (a.include_screenshot, &a.screenshot) {
        out.push_str(&format!("- **Screenshot:** {}", path.display()));
        if el.off_screen {
            out.push_str(" (partly off-screen)");
        }
        out.push('\n');
    }
    if a.intent == Intent::Copy
        && let Some(html) = el.html.as_deref().filter(|h| !h.is_empty())
    {
        let html = match html.char_indices().nth(HTML_CUT) {
            Some((i, _)) => format!("{}<!-- cut -->", &html[..i]),
            None => html.to_string(),
        };
        out.push('\n');
        out.push_str(&fenced("html", &html));
    }
    out
}

fn cut_text(text: &str) -> String {
    cut_at(text, TEXT_CUT)
}

/// `text` cut at `max` characters with `…`.
fn cut_at(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}
