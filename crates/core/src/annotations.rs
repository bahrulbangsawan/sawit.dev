//! Visual feedback: what the user pointed at on a page, and the markdown
//! that carries it to a terminal or an agent.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::browser::ConsoleEntry;

/// Text longer than this is cut with `…`.
const TEXT_CUT: usize = 100;
/// HTML longer than this is cut (only the `copy` intent includes it).
const HTML_CUT: usize = 20_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    #[default]
    Fix,
    Change,
    Question,
    Copy,
}

impl Intent {
    pub const ALL: [Intent; 4] = [Intent::Fix, Intent::Change, Intent::Question, Intent::Copy];

    pub fn label(self) -> &'static str {
        match self {
            Intent::Fix => "fix",
            Intent::Change => "change",
            Intent::Question => "question",
            Intent::Copy => "copy",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceHint {
    pub file: String,
    #[serde(default)]
    pub line: Option<u32>,
    pub framework: String,
    #[serde(default)]
    pub component: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElementDetails {
    pub selector: String,
    pub label: String,
    pub tag: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub text: Option<String>,
    pub rect: Rect,
    #[serde(default)]
    pub styles: Vec<(String, String)>,
    #[serde(default)]
    pub source: Option<SourceHint>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub off_screen: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub n: u32,
    pub url: String,
    pub element: ElementDetails,
    pub comment: String,
    pub intent: Intent,
    pub screenshot: Option<PathBuf>,
    pub include_screenshot: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageInfo {
    pub title: String,
    pub url: String,
    pub viewport: (u32, u32),
    pub device: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageGroup {
    pub page: PageInfo,
    pub items: Vec<Annotation>,
}

pub fn markdown(groups: &[PageGroup], errors: Option<(&str, &[ConsoleEntry])>) -> String {
    let mut sections: Vec<String> = Vec::new();
    for g in groups {
        let mut out = format!("# Visual feedback — {}\n", g.page.title);
        let (w, h) = g.page.viewport;
        out.push_str(&format!("**URL:** {} · **Viewport:** {w}×{h}", g.page.url));
        if let Some(device) = g.page.device.as_deref().filter(|d| !d.is_empty()) {
            out.push_str(&format!(" ({device})"));
        }
        out.push('\n');
        for item in &g.items {
            out.push('\n');
            out.push_str(&item_markdown(item));
        }
        sections.push(out);
    }
    if let Some((url, list)) = errors.filter(|(_, l)| !l.is_empty()) {
        let mut out = format!("## Console errors on {url} ({})\n", list.len());
        for e in list {
            out.push_str(&format!(
                "- `+{:.1}s` {}{}\n",
                e.time_ms / 1000.,
                e.text,
                error_origin(e)
            ));
        }
        sections.push(out);
    }
    sections.join("\n")
}

fn error_origin(e: &ConsoleEntry) -> String {
    let Some(url) = e.url.as_deref().filter(|u| !u.is_empty()) else {
        return String::new();
    };
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path);
    match e.line {
        Some(line) => format!(" — {name}:{line}"),
        None => format!(" — {name}"),
    }
}

fn item_markdown(a: &Annotation) -> String {
    let el = &a.element;
    let mut out = format!("## {}. `{}` — {}\n", a.n, el.label, a.intent.label());
    if !a.comment.trim().is_empty() {
        out.push_str(&format!("- **Comment:** {}\n", a.comment));
    }
    out.push_str(&format!("- **Selector:** `{}`\n", el.selector));

    let mut tag = format!("<{}", el.tag);
    if let Some(id) = el.id.as_deref().filter(|i| !i.is_empty()) {
        tag.push_str(&format!(" id=\"{id}\""));
    }
    if !el.classes.is_empty() {
        tag.push_str(&format!(" class=\"{}\"", el.classes.join(" ")));
    }
    tag.push('>');
    out.push_str(&format!("- **Element:** `{tag}`"));
    if let Some(text) = el.text.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push_str(&format!(" \"{}\"", cut_text(text.trim())));
    }
    out.push('\n');

    if let Some(src) = &el.source {
        let component = src.component.as_deref().filter(|c| !c.is_empty());
        if !src.file.is_empty() {
            let mut at = src.file.clone();
            if let Some(line) = src.line {
                at.push_str(&format!(":{line}"));
            }
            let mut who = src.framework.clone();
            if let Some(c) = component {
                who.push_str(&format!(", <{c}>"));
            }
            out.push_str(&format!("- **Source:** `{at}` ({who})\n"));
        } else if let Some(c) = component {
            out.push_str(&format!("- **Source:** <{c}> ({})\n", src.framework));
        }
    }

    let r = &el.rect;
    out.push_str(&format!(
        "- **Box:** {}×{} at ({}, {})\n",
        r.width.round(),
        r.height.round(),
        r.x.round(),
        r.y.round()
    ));
    if !el.styles.is_empty() {
        let styles: Vec<String> = el.styles.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        out.push_str(&format!("- **Styles:** {}\n", styles.join("; ")));
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
        {
            out.push_str("\n```html\n");
            match html.char_indices().nth(HTML_CUT) {
                Some((i, _)) => {
                    out.push_str(&html[..i]);
                    out.push_str("<!-- cut -->");
                }
                None => out.push_str(html),
            }
            out.push_str("\n```\n");
        }
    }
    out
}

fn cut_text(text: &str) -> String {
    match text.char_indices().nth(TEXT_CUT) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

/// The next annotation number for `url`'s page (hash ignored).
pub fn next_number(existing: &[Annotation], url: &str) -> u32 {
    let key = page_key(url);
    existing
        .iter()
        .filter(|a| page_key(&a.url) == key)
        .map(|a| a.n)
        .max()
        .map_or(1, |n| n + 1)
}

/// The URL without `#…`.
pub fn page_key(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

/// Where screenshots of a day go: the project's `.nyawit/annotations/<date>`,
/// else the app cache.
pub fn screenshot_dir(project: Option<&Path>, cache: &Path, date: &str) -> PathBuf {
    match project {
        Some(p) => crate::nyawit::dir(p).join("annotations").join(date),
        None => cache.join("annotations").join(date),
    }
}

/// `<host>[-<port>]-<n>.png`.
pub fn screenshot_name(url: &str, n: u32) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host: String = authority
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let host = host.trim_matches('-');
    let host = if host.is_empty() { "page" } else { host };
    format!("{host}-{n}.png")
}

/// A source file as the page reports it, made project-relative where it can.
pub fn relative_source(file: &str, project: Option<&Path>) -> String {
    let mut f = file.split(['?', '#']).next().unwrap_or(file);
    if let Some(rest) = f.strip_prefix("webpack://") {
        // `webpack:///./src/a.tsx` or `webpack://name/./src/a.tsx`.
        f = rest.split_once('/').map_or(rest, |(_, r)| r);
    } else if let Some(rest) = f.strip_prefix("file://") {
        f = rest;
    }
    if let Some(rest) = f.strip_prefix("/@fs") {
        f = rest;
    }
    if let Some(root) = project {
        let root = root.to_string_lossy();
        let root = root.trim_end_matches('/');
        if let Some(rest) = f.strip_prefix(root).and_then(|r| r.strip_prefix('/')) {
            f = rest;
        }
    }
    f.strip_prefix("./").unwrap_or(f).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element() -> ElementDetails {
        ElementDetails {
            selector: "main > div > button.btn-primary".into(),
            label: "button.btn-primary".into(),
            tag: "button".into(),
            id: None,
            classes: vec!["btn-primary".into()],
            text: Some("Submit order".into()),
            rect: Rect {
                x: 350.,
                y: 680.,
                width: 120.,
                height: 40.,
            },
            styles: vec![
                ("display".into(), "flex".into()),
                ("background-color".into(), "rgb(34, 197, 94)".into()),
                ("font-size".into(), "14px".into()),
            ],
            source: Some(SourceHint {
                file: "src/components/Checkout.tsx".into(),
                line: Some(42),
                framework: "React".into(),
                component: Some("Checkout".into()),
            }),
            html: None,
            off_screen: false,
        }
    }

    fn annotation() -> Annotation {
        Annotation {
            n: 1,
            url: "http://localhost:5173/cart".into(),
            element: element(),
            comment: "The button should use the brand blue, not green.".into(),
            intent: Intent::Fix,
            screenshot: Some("/p/.nyawit/annotations/2026-10-29/localhost-5173-1.png".into()),
            include_screenshot: true,
        }
    }

    fn page() -> PageInfo {
        PageInfo {
            title: "Cart".into(),
            url: "http://localhost:5173/cart".into(),
            viewport: (393, 852),
            device: Some("iPhone 15".into()),
        }
    }

    fn group(items: Vec<Annotation>) -> PageGroup {
        PageGroup {
            page: page(),
            items,
        }
    }

    /// A lone annotation with nothing optional filled in.
    fn bare() -> Annotation {
        let mut a = annotation();
        a.comment = String::new();
        a.element.text = None;
        a.element.source = None;
        a.screenshot = None;
        a
    }

    #[test]
    fn markdown_matches_the_spec() {
        let err = ConsoleEntry {
            level: "error".into(),
            text: "Uncaught TypeError: x is undefined".into(),
            url: Some("http://localhost:5173/assets/app.js".into()),
            line: Some(88),
            time_ms: 12_300.,
        };
        let md = markdown(
            &[group(vec![annotation()])],
            Some(("http://localhost:5173/cart", &[err])),
        );
        let want = "\
# Visual feedback — Cart
**URL:** http://localhost:5173/cart · **Viewport:** 393×852 (iPhone 15)

## 1. `button.btn-primary` — fix
- **Comment:** The button should use the brand blue, not green.
- **Selector:** `main > div > button.btn-primary`
- **Element:** `<button class=\"btn-primary\">` \"Submit order\"
- **Source:** `src/components/Checkout.tsx:42` (React, <Checkout>)
- **Box:** 120×40 at (350, 680)
- **Styles:** display: flex; background-color: rgb(34, 197, 94); font-size: 14px
- **Screenshot:** /p/.nyawit/annotations/2026-10-29/localhost-5173-1.png

## Console errors on http://localhost:5173/cart (1)
- `+12.3s` Uncaught TypeError: x is undefined — app.js:88
";
        assert_eq!(md, want);
    }

    #[test]
    fn empty_fields_are_left_out() {
        let md = markdown(&[group(vec![bare()])], None);
        assert!(!md.contains("**Comment:**"));
        assert!(!md.contains("**Source:**"));
        assert!(!md.contains("**Screenshot:**"));
        assert!(!md.contains("\"Submit"));
        assert!(md.contains("**Selector:**"));
    }

    #[test]
    fn screenshot_line_needs_the_flag() {
        let mut a = annotation();
        a.include_screenshot = false;
        assert!(!markdown(&[group(vec![a])], None).contains("**Screenshot:**"));
    }

    #[test]
    fn copy_intent_adds_html() {
        let mut a = annotation();
        a.intent = Intent::Copy;
        a.element.html = Some("a".repeat(25_000));
        let md = markdown(&[group(vec![a])], None);
        let body = md
            .split("```html\n")
            .nth(1)
            .and_then(|r| r.split("\n```").next())
            .expect("a fenced html block");
        assert!(body.ends_with("<!-- cut -->"));
        assert_eq!(body.matches('a').count(), 20_000);
        assert_eq!(body.chars().count(), 20_000 + "<!-- cut -->".len());
    }

    #[test]
    fn html_only_for_copy() {
        let mut a = annotation();
        a.element.html = Some("<b>x</b>".into());
        assert!(!markdown(&[group(vec![a])], None).contains("```html"));
    }

    #[test]
    fn comment_is_verbatim() {
        let mut a = annotation();
        a.comment = "use `blue`\n## heading".into();
        let md = markdown(&[group(vec![a])], None);
        assert!(md.contains("- **Comment:** use `blue`\n## heading\n"));
    }

    #[test]
    fn off_screen_is_noted() {
        let mut a = annotation();
        a.element.off_screen = true;
        let md = markdown(&[group(vec![a])], None);
        assert!(md.contains("localhost-5173-1.png (partly off-screen)\n"));
    }

    #[test]
    fn long_text_is_cut() {
        let mut a = annotation();
        a.element.text = Some("x".repeat(150));
        let md = markdown(&[group(vec![a])], None);
        assert!(md.contains(&format!("\"{}…\"", "x".repeat(100))));
    }

    #[test]
    fn groups_keep_their_order() {
        let mut second = group(vec![annotation()]);
        second.page.title = "Home".into();
        let md = markdown(&[group(vec![annotation()]), second], None);
        assert_eq!(md.matches("# Visual feedback").count(), 2);
        assert!(md.find("Cart").unwrap() < md.find("Home").unwrap());
    }

    #[test]
    fn numbers_count_per_url() {
        let at = |url: &str, n| Annotation {
            n,
            url: url.into(),
            ..annotation()
        };
        let existing = vec![
            at("http://a/x", 1),
            at("http://a/x#top", 2),
            at("http://a/y", 1),
        ];
        assert_eq!(next_number(&existing, "http://a/x"), 3);
        assert_eq!(next_number(&existing, "http://a/x#other"), 3);
        assert_eq!(next_number(&existing, "http://a/z"), 1);
    }

    #[test]
    fn numbers_follow_the_highest_after_a_delete() {
        let at = |n| Annotation {
            n,
            url: "http://a/x".into(),
            ..annotation()
        };
        // 2 was deleted: the next is 4, so 3 keeps its number.
        assert_eq!(next_number(&[at(1), at(3)], "http://a/x"), 4);
    }

    #[test]
    fn screenshot_names_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let name = screenshot_name("http://localhost:5173/", 1);
        assert_eq!(name, "localhost-5173-1.png");
        let first = crate::nyawit::unique(dir.path(), &name);
        std::fs::write(&first, b"x").unwrap();
        let second = crate::nyawit::unique(dir.path(), &name);
        assert_eq!(first.file_name().unwrap(), "localhost-5173-1.png");
        assert_eq!(second.file_name().unwrap(), "localhost-5173-1-2.png");
    }

    #[test]
    fn screenshot_names() {
        assert_eq!(
            screenshot_name("http://localhost:5173/cart?x=1", 1),
            "localhost-5173-1.png"
        );
        assert_eq!(
            screenshot_name("https://www.apple.com/iphone/", 2),
            "www.apple.com-2.png"
        );
    }

    #[test]
    fn screenshot_dirs() {
        assert_eq!(
            screenshot_dir(Some(Path::new("/p")), Path::new("/c"), "2026-10-29"),
            Path::new("/p/.nyawit/annotations/2026-10-29")
        );
        assert_eq!(
            screenshot_dir(None, Path::new("/c"), "2026-10-29"),
            Path::new("/c/annotations/2026-10-29")
        );
    }

    #[test]
    fn relative_sources() {
        assert_eq!(
            relative_source("webpack:///./src/App.tsx", None),
            "src/App.tsx"
        );
        let app = Some(Path::new("/Users/me/app"));
        assert_eq!(
            relative_source("/Users/me/app/src/App.tsx", app),
            "src/App.tsx"
        );
        assert_eq!(
            relative_source("/@fs/Users/me/app/src/x.vue", app),
            "src/x.vue"
        );
        assert_eq!(relative_source("file:///opt/x.js", None), "/opt/x.js");
    }

    #[test]
    fn picked_payload_deserializes() {
        let json = r##"{"selector":"#a","label":"button#a","tag":"button","id":"a","classes":[],"text":"Go","rect":{"x":1,"y":2,"width":3,"height":4},"styles":[["display","flex"]],"source":{"file":"src/A.tsx","line":7,"framework":"React","component":"A"},"html":"<button id=\"a\">Go</button>","off_screen":false}"##;
        let d: ElementDetails = serde_json::from_str(json).unwrap();
        assert_eq!(d.styles[0], ("display".to_string(), "flex".to_string()));
        assert_eq!(d.source.unwrap().line, Some(7));
        assert!(!d.off_screen);
    }

    #[test]
    fn component_only_source_names_the_component() {
        let mut a = annotation();
        a.element.source = Some(SourceHint {
            file: String::new(),
            line: None,
            framework: "React".into(),
            component: Some("Checkout".into()),
        });
        let md = markdown(&[group(vec![a])], None);
        assert!(md.contains("- **Source:** <Checkout> (React)\n"));
        assert!(!md.contains("``"));
    }

    #[test]
    fn source_with_no_file_and_no_component_is_left_out() {
        let mut a = annotation();
        a.element.source = Some(SourceHint {
            file: String::new(),
            line: None,
            framework: "React".into(),
            component: None,
        });
        let md = markdown(&[group(vec![a])], None);
        assert!(!md.contains("**Source:**"));
    }
}
