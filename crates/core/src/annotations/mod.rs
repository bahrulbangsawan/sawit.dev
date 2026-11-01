//! Visual feedback: what the user pointed at on a page, and the markdown
//! that carries it to a terminal or an agent.

mod markdown;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use markdown::markdown;

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

/// An element next to the annotated one, so an agent can find the spot in the markup:
/// its `tag#id.class` label and the start of its text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Neighbour {
    pub label: String,
    #[serde(default)]
    pub text: Option<String>,
}

/// Where the element sits among its neighbours.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Context {
    #[serde(default)]
    pub parent: Option<Neighbour>,
    /// The element sibling before it.
    #[serde(default)]
    pub before: Option<Neighbour>,
    /// The element sibling after it.
    #[serde(default)]
    pub after: Option<Neighbour>,
}

/// What the page reports about a picked element. The fields after `off_screen` came later;
/// all of them are optional, so an older page agent's pick still reads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// The box in the viewport, CSS px.
    pub rect: Rect,
    #[serde(default)]
    pub styles: Vec<(String, String)>,
    #[serde(default)]
    pub source: Option<SourceHint>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub off_screen: bool,
    /// The DOM path from `body`, one `tag#id.class` per level.
    #[serde(default)]
    pub path: Option<String>,
    /// The attributes that name or wire the element (`data-testid`, `aria-*`, `href`, `src`,
    /// `alt`, `role`, `name`, `type`…), in page order.
    #[serde(default)]
    pub attributes: Vec<(String, String)>,
    /// The accessible role, as the agent's snapshot computes it.
    #[serde(default)]
    pub role: Option<String>,
    /// The accessible name.
    #[serde(default)]
    pub name: Option<String>,
    /// The box in the page (the viewport box plus the scroll), CSS px.
    #[serde(default)]
    pub page_rect: Option<Rect>,
    /// The viewport's size when it was picked, CSS px.
    #[serde(default)]
    pub viewport: Option<(u32, u32)>,
    #[serde(default)]
    pub context: Option<Context>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub n: u32,
    pub url: String,
    /// The page's title when the element was picked.
    pub title: String,
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
