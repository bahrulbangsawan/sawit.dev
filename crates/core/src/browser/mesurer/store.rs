//! Where mesurer's settings and workspaces are kept, and what a workspace holds for a page.
//!
//! Settings are app-wide (`<data>/browser/mesurer/settings.json`). A workspace is per project and
//! origin (`<project>/.nyawit/browser/mesurer/<origin-key>.json`; with no project,
//! `<data>/cache/browser/mesurer/…`): mesurer's `PagedWorkspaceStore`, its session chrome and, by
//! mesurer's page key ([`page_key`]), each page's guides, drawings, measurements and comments.
//! Files are written whole (a temporary file renamed over the old one), and never through a link.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::instances::AppPaths;

/// The file of mesurer's settings.
pub fn settings_path(paths: &AppPaths) -> PathBuf {
    paths
        .root
        .join("browser")
        .join("mesurer")
        .join("settings.json")
}

/// The workspace file of `origin` (an [`origin_key`]) in `project`, or in Sawit's cache folder.
pub fn workspace_path(project: Option<&Path>, paths: &AppPaths, origin: &str) -> PathBuf {
    let dir = match project {
        Some(project) => crate::nyawit::dir(project).join("browser").join("mesurer"),
        None => paths.cache_dir().join("browser").join("mesurer"),
    };
    dir.join(format!("{origin}.json"))
}

/// A URL's parts, as far as the keys need them; `None` for text with no scheme.
struct Parts<'a> {
    scheme: String,
    host: Option<String>,
    port: Option<&'a str>,
    path: &'a str,
    query: &'a str,
    fragment: Option<&'a str>,
}

fn split(url: &str) -> Option<Parts<'_>> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let valid = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return None;
    }
    let mut rest = &url[colon + 1..];
    let fragment = rest.find('#').map(|at| {
        let f = &rest[at + 1..];
        rest = &rest[..at];
        f
    });
    let query = match rest.find('?') {
        Some(at) => {
            let q = &rest[at + 1..];
            rest = &rest[..at];
            q
        }
        None => "",
    };
    let (mut host, mut port) = (None, None);
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find('/').unwrap_or(after.len());
        let authority = &after[..end];
        rest = &after[end..];
        let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (h, p) = if hostport.starts_with('[') {
            match hostport.find(']') {
                Some(at) => (&hostport[..=at], hostport[at + 1..].strip_prefix(':')),
                None => (hostport, None),
            }
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (hostport, None),
            }
        };
        host = Some(h.to_ascii_lowercase());
        port = p.filter(|p| !p.is_empty());
    }
    Some(Parts {
        scheme: scheme.to_ascii_lowercase(),
        host,
        port,
        path: rest,
        query,
        fragment,
    })
}

/// The name of an origin's workspace file: `http_localhost_3000`, `https_my_app_test`, `file`.
/// Only `[a-z0-9_-]`, so it can't leave its folder.
pub fn origin_key(url: &str) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .map(|c| {
                let c = c.to_ascii_lowercase();
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let Some(parts) = split(url.trim()) else {
        return "page".into();
    };
    if parts.scheme == "file" {
        return "file".into();
    }
    let default_port = match parts.scheme.as_str() {
        "http" | "ws" => Some("80"),
        "https" | "wss" => Some("443"),
        _ => None,
    };
    let mut key = clean(&parts.scheme);
    if let Some(host) = parts.host.as_deref().filter(|h| !h.is_empty()) {
        key.push('_');
        key.push_str(&clean(host));
        if let Some(port) = parts.port.filter(|p| Some(*p) != default_port) {
            key.push('_');
            key.push_str(&clean(port));
        }
    }
    if key.is_empty() { "page".into() } else { key }
}

/// mesurer's key for the page at `url` (`core/workspace.ts` `getPageKey`): the path without its
/// trailing slashes, the query's pairs sorted by name (as `URLSearchParams.sort()` then
/// `toString()`), and the fragment when it is a route (`#/…`).
pub fn page_key(url: &str) -> String {
    let Some(parts) = split(url.trim()) else {
        return "/".into();
    };
    let path = parts.path.trim_end_matches('/');
    let mut key = if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };
    let mut pairs = form_pairs(parts.query);
    // Stable, by UTF-16 code units, as the URL standard sorts them.
    pairs.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    if !pairs.is_empty() {
        key.push('?');
        let query: Vec<String> = pairs
            .iter()
            .map(|(name, value)| format!("{}={}", form_encode(name), form_encode(value)))
            .collect();
        key.push_str(&query.join("&"));
    }
    if let Some(fragment) = parts.fragment.filter(|f| f.starts_with('/')) {
        key.push('#');
        key.push_str(fragment);
    }
    key
}

/// `application/x-www-form-urlencoded` pairs, as `URLSearchParams` reads them.
fn form_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (form_decode(name), form_decode(value))
        })
        .collect()
}

fn form_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for &b in text.as_bytes() {
        match b {
            b' ' => out.push('+'),
            b'*' | b'-' | b'.' | b'_' => out.push(b as char),
            b if b.is_ascii_alphanumeric() => out.push(b as char),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Reads a JSON file; `None` when there is none. A link is refused, never read through.
pub fn read_json(path: &Path) -> Result<Option<Value>, String> {
    const MAX: u64 = 64 << 20;
    let shown = path.display();
    let meta = match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{shown} can't be read: {e}")),
        Ok(meta) => meta,
    };
    if meta.file_type().is_symlink() {
        return Err(format!(
            "{shown} is a link; Sawit doesn't read mesurer's files through links."
        ));
    }
    if !meta.is_file() || meta.len() > MAX {
        return Err(format!("{shown} isn't a mesurer file."));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown} can't be read: {e}"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{shown} isn't JSON: {e}"))
}

/// Refuses when the file at `path`, or a folder between `base` and it, is a link.
pub fn no_links(base: &Path, path: &Path) -> Result<(), String> {
    let mut at = path;
    while at != base && at.starts_with(base) {
        if std::fs::symlink_metadata(at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!(
                "{} is a link; Sawit doesn't write mesurer's files through links.",
                at.display()
            ));
        }
        match at.parent() {
            Some(parent) => at = parent,
            None => break,
        }
    }
    Ok(())
}

/// Writes `value` to `path` whole: to a new file beside it, renamed over it. Refused when the
/// file or its folder is a link.
pub fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().ok_or("A mesurer file needs a folder.")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{} can't be made: {e}", dir.display()))?;
    no_links(dir.parent().unwrap_or(dir), path)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mesurer");
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".{name}.{}-{n}.tmp", std::process::id()));
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_data()?;
        std::fs::rename(&temp, path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{} can't be written: {e}", path.display()));
    }
    Ok(())
}

/// Writes `value` to `path` as [`write_json`] does, refused when the file or a folder between
/// `base` and it is a link: checked before the folders are made, and again before the write.
pub fn write_json_within(base: &Path, path: &Path, value: &Value) -> Result<(), String> {
    no_links(base, path)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("{} can't be made: {e}", dir.display()))?;
    }
    no_links(base, path)?;
    write_json(path, value)
}

/// Removes the file at `path`, if there is one; a link is left as it is.
pub fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{} can't be removed: {e}", path.display())),
        Ok(m) if m.file_type().is_symlink() => Err(format!(
            "{} is a link; Sawit leaves it as it is.",
            path.display()
        )),
        Ok(_) => std::fs::remove_file(path)
            .map_err(|e| format!("{} can't be removed: {e}", path.display())),
    }
}

/// The session chrome of a workspace (`enabled`, `minimized`, `xrayVisible`, `toolMode`,
/// `rulersVisible`, `guideOrientation`).
pub fn chrome_of(envelope: &Value) -> Value {
    const KEYS: [&str; 6] = [
        "enabled",
        "minimized",
        "xrayVisible",
        "toolMode",
        "rulersVisible",
        "guideOrientation",
    ];
    let mut chrome = Map::new();
    for key in KEYS {
        if let Some(value) = envelope.get(key) {
            chrome.insert(key.into(), value.clone());
        }
    }
    Value::Object(chrome)
}

/// What a workspace holds for the page `key` (`MesurerPageArtifacts`), `null` for none.
pub fn page_artifacts(envelope: &Value, key: &str) -> Value {
    envelope["pages"][key].clone()
}

/// What mesurer has on a page, in counts: the panel's "Page tools" line, `app_state`, the
/// `state` op.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// mesurer's tool (`none`, `select`, `guides`, `comment`, …).
    pub tool: String,
    pub enabled: bool,
    pub minimized: bool,
    pub xray: bool,
    pub rulers: bool,
    /// Comment threads not resolved.
    pub comments_open: usize,
    pub arrows: usize,
    pub texts: usize,
    pub strokes: usize,
    pub guides: usize,
    pub layout_guides: usize,
    /// Pinned distances.
    pub distances: usize,
}

impl Summary {
    /// The summary of `envelope` (a `PagedWorkspaceStore`) on the page `key`.
    pub fn of(envelope: &Value, key: &str) -> Summary {
        let page = &envelope["pages"][key];
        let count = |name: &str| page[name].as_array().map_or(0, Vec::len);
        let flag = |name: &str| envelope[name].as_bool().unwrap_or(false);
        let comments_open = page["comments"].as_array().map_or(0, |all| {
            all.iter().filter(|c| c["status"] != "resolved").count()
        });
        Summary {
            tool: envelope["toolMode"].as_str().unwrap_or("none").to_string(),
            enabled: flag("enabled"),
            minimized: flag("minimized"),
            xray: flag("xrayVisible"),
            rulers: flag("rulersVisible"),
            comments_open,
            arrows: count("arrows"),
            texts: count("textAnnotations"),
            strokes: count("penStrokes"),
            guides: count("guides"),
            layout_guides: count("layoutGuides"),
            distances: count("heldDistances"),
        }
    }

    /// How many pieces of markup the page has (comments, drawings, guides, distances).
    pub fn markup(&self) -> usize {
        self.comments_open
            + self.arrows
            + self.texts
            + self.strokes
            + self.guides
            + self.layout_guides
            + self.distances
    }
}
