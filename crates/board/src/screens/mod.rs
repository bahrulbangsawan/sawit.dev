//! UI screens on the canvas: HTML pages kept in the project and shown in frames, where the rest
//! of the canvas (diagrams, stickies, illustrations) stays vector.
//!
//! A screen is an ordinary FigCraft frame whose plugin data [`SCREEN_KEY`] holds
//! `{"screen": "<name>", "viewport": {"w": …, "h": …}}`: the name of its folder, and the CSS size
//! it was made for. Its source is that folder in the canvas's screens folder ([`screens_dir`]),
//! `.nyawit/screens/<name>/index.html` for a project's canvas, with its CSS and images beside it.
//! HTML is the source because WebKit renders it with no build step.
//!
//! The app serves a screen's folder to WebKit through a URL scheme of its own ([`serve`]), so the
//! page has no network and reaches nothing outside its folder. Agents write a screen's files
//! ([`files`]); it is downloaded as one HTML file with its CSS and images inlined ([`inline`]), or
//! as a React component with its CSS ([`react`]).
//!
//! The frame's size is the page's viewport: resizing the frame lays the page out at the new size.
//! The plugin data's `viewport` keeps the size it was made at, which Preview opens at.

pub mod css;
pub mod export;
pub mod files;
pub mod inline;
pub mod react;
pub mod serve;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use figcraft_doc::{Document, Node, NodeId};
use serde::{Deserialize, Serialize};

/// The plugin data key of a screen's frame.
pub const SCREEN_KEY: &str = "sawit.screen";
/// The URL scheme the app serves screens with: `sawit-screen://<name>/index.html`.
pub const SCHEME: &str = "sawit-screen";
/// A screen's page, in its folder.
pub const INDEX: &str = "index.html";
/// The longest name a screen's folder gets.
const NAME_MAX: usize = 48;
/// The most a screen's file may hold to be served, read or exported.
pub const MAX_FILE: u64 = 64 * 1024 * 1024;

/// The CSS size a screen is laid out at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub w: f64,
    pub h: f64,
}

/// What a screen's frame holds in its plugin data ([`SCREEN_KEY`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenData {
    pub screen: String,
    pub viewport: Viewport,
}

impl ScreenData {
    /// The plugin data's value.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// The screen `node` is, if it is one: plugin data that reads, with a name a folder can have.
pub fn screen_of(node: &Node) -> Option<ScreenData> {
    let data: ScreenData = serde_json::from_str(node.plugin_data.get(SCREEN_KEY)?).ok()?;
    is_name(&data.screen).then_some(data)
}

/// A screen on the canvas: its frame, the frame's page, and its plugin data.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub frame: NodeId,
    pub page: NodeId,
    pub data: ScreenData,
}

/// Every screen in `doc`, by frame id. Two frames can show one screen (a copy of the frame).
pub fn screens(doc: &Document) -> Vec<Found> {
    let mut found: Vec<Found> = doc
        .nodes()
        .filter(|node| node.plugin_data.contains_key(SCREEN_KEY))
        .filter_map(|node| {
            Some(Found {
                frame: node.id,
                page: doc.page_of(node.id)?,
                data: screen_of(node)?,
            })
        })
        .collect();
    found.sort_by_key(|found| found.frame);
    found
}

/// The frame that shows screen `name` in `doc`: the first made, when copies show it too.
pub fn find(doc: &Document, name: &str) -> Option<Found> {
    screens(doc)
        .into_iter()
        .find(|found| found.data.screen == name)
}

/// Whether `name` can name a screen's folder, its URL's host too: 1 to [`NAME_MAX`] of `a`–`z`,
/// `0`–`9` and `-`, starting with a letter or a digit.
pub fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The folder name for a screen called `title` ("Pricing page" → `pricing-page`), not one of
/// `taken`: `-2`, `-3`, … after it when it is.
pub fn name_for(title: &str, taken: &[String]) -> String {
    let mut base = String::new();
    for c in title.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            base.push(c);
        } else if !base.is_empty() && !base.ends_with('-') {
            base.push('-');
        }
    }
    let mut base: String = base
        .trim_end_matches('-')
        .chars()
        .take(NAME_MAX - 4)
        .collect();
    base = base.trim_end_matches('-').to_string();
    if base.is_empty() {
        base = "screen".into();
    }
    let free = |name: &str| !taken.iter().any(|t| t == name);
    if free(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|name| free(name))
        .expect("a free name")
}

/// The folder a canvas file's screens are kept in: `.nyawit/screens/` beside a project's canvas
/// (`.nyawit/canvas.sawit`), else `<stem>.screens/` beside the canvas file (`flow.screens/` for
/// `flow.sawit`), so that two canvas files in one folder never share screens.
pub fn screens_dir(canvas_file: &Path) -> PathBuf {
    let dir = canvas_file.parent().unwrap_or(Path::new("."));
    let in_nyawit = dir
        .file_name()
        .is_some_and(|n| n == sawit_core::nyawit::DIR);
    if in_nyawit {
        return dir.join("screens");
    }
    let stem = canvas_file.file_stem().map_or_else(
        || "canvas".to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    dir.join(format!("{stem}.screens"))
}

/// Screen `name`'s folder in the screens folder `dir`.
pub fn folder(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

/// The names of the screens' folders in `dir`, sorted: what a new screen's name must not take.
pub fn folders(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_name(name))
        .collect();
    names.sort();
    names
}

/// The URL of screen `name`'s page.
pub fn url(name: &str) -> String {
    format!("{SCHEME}://{name}/{INDEX}")
}

/// `href`, a reference in the file `base` of a screen's folder (a path from the folder, such as
/// `index.html` or `css/site.css`), as a path from the folder: `None` when it leaves the folder,
/// names another place (`https:`, `//host/…`, `data:`, any scheme), or is empty. Its query and
/// fragment go; what is percent-encoded is decoded. A path from the folder's root (`/img/a.png`)
/// is from the screen's own root.
pub fn resolve(base: &str, href: &str) -> Option<String> {
    segments(base, href)
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.join("/"))
}

/// [`resolve`]'s path as its parts: none for the folder itself (`/`, `.`); `None` when `href`
/// leaves the folder, names another place or is broken.
pub fn segments(base: &str, href: &str) -> Option<Vec<String>> {
    let href = href.trim();
    let href = href.split(['#', '?']).next().unwrap_or_default();
    if href.is_empty() || href.starts_with("//") || has_scheme(href) || href.contains('\\') {
        return None;
    }
    let decoded = percent_decode(href)?;
    let mut parts: Vec<String> = Vec::new();
    if !decoded.starts_with('/') {
        let mut dir: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
        dir.pop();
        parts.extend(dir.into_iter().map(str::to_string));
    }
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part.contains('\0') || part.contains('\\') => return None,
            part => parts.push(part.to_string()),
        }
    }
    Some(parts)
}

/// Whether `href` starts with a URL scheme (`https:`, `data:`, `mailto:`): letters, then letters,
/// digits, `+`, `-` or `.`, then `:`.
pub fn has_scheme(href: &str) -> bool {
    let Some((scheme, _)) = href.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `text` with its `%xx` escapes decoded, as UTF-8; `None` when an escape is broken or the bytes
/// aren't UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Reads `rel` (a path from the folder, as [`resolve`] gives it) in screen folder `folder`: a
/// regular file of at most [`MAX_FILE`] whose real place, links followed, is inside the folder.
pub fn read(folder: &Path, rel: &str) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind};
    let path = folder.join(rel);
    let real = path.canonicalize()?;
    let root = folder.canonicalize()?;
    if !real.starts_with(&root) {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "it is outside the screen's folder",
        ));
    }
    let meta = std::fs::metadata(&real)?;
    if !meta.is_file() {
        return Err(Error::new(ErrorKind::InvalidInput, "it isn't a file"));
    }
    if meta.len() > MAX_FILE {
        return Err(Error::new(ErrorKind::InvalidInput, "it is too large"));
    }
    std::fs::read(&real)
}

/// The media type of a file, by its extension.
pub fn mime(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// A React component's name for screen `name`: `pricing-page` → `PricingPage`, and `Screen…`
/// in front of one that would start with a digit.
pub fn component_name(name: &str) -> String {
    let mut out = String::new();
    for word in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if !out.starts_with(|c: char| c.is_ascii_alphabetic()) {
        out.insert_str(0, "Screen");
    }
    out
}
