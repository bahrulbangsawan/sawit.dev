//! What the walk of a frame's tree ([`super::tree`]) asks of single layers: which are drawn as
//! SVG, an instance's text overrides, names as classes and identifiers, images as `data:` URLs.

use base64::Engine as _;
use figcraft_doc::{Document, Node, NodeId, NodeKind};

/// The texts instance `instance` shows otherwise than its main `main`, by the main's text names.
pub(super) fn overrides(doc: &Document, instance: NodeId, main: NodeId) -> Vec<(String, String)> {
    let texts = |root: NodeId| -> Vec<(String, String)> {
        doc.subtree(root)
            .into_iter()
            .filter_map(|id| {
                let n = doc.node(id)?;
                let NodeKind::Text(t) = &n.kind else {
                    return None;
                };
                Some((n.name.clone(), t.characters.clone()))
            })
            .collect()
    };
    let (shown, own) = (texts(instance), texts(main));
    let mut out = Vec::new();
    for ((_, shown), (name, own)) in shown.iter().zip(&own) {
        if shown != own && !out.iter().any(|(n, _): &(String, String)| n == name) {
            out.push((name.clone(), shown.clone()));
        }
    }
    out
}

/// A property's name as a TypeScript identifier: `Has icon` → `hasIcon`, not one of `taken`.
pub(super) fn ident(label: &str, taken: &[String]) -> String {
    let words = label
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty());
    let mut out = String::new();
    for (i, word) in words.enumerate() {
        let mut chars = word.chars();
        let first = chars.next().unwrap_or_default();
        match i {
            0 => out.push(first.to_ascii_lowercase()),
            _ => out.push(first.to_ascii_uppercase()),
        }
        out.extend(chars);
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'v');
    }
    while taken.contains(&out) {
        out.push('_');
    }
    out
}

/// `name` as a class name's words: lowercase ASCII letters and digits, a hyphen between words,
/// 40 characters at most.
pub(super) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.chars().take(40).collect();
    out.trim_end_matches('-').to_string()
}

/// Whether `n` is drawn as an SVG: a vector shape, or a small group or frame of vectors only (an
/// icon).
pub(super) fn is_vector(doc: &Document, n: &Node) -> bool {
    let shape = |n: &Node| {
        matches!(
            n.kind,
            NodeKind::Vector { .. }
                | NodeKind::BooleanOp { .. }
                | NodeKind::Star { .. }
                | NodeKind::Polygon { .. }
                | NodeKind::Line
        )
    };
    if shape(n) {
        return true;
    }
    let small = n.width <= 96.0 && n.height <= 96.0;
    let grouped = matches!(n.kind, NodeKind::Group | NodeKind::Frame) && n.layout.is_none();
    let below: Vec<NodeId> = doc
        .subtree(n.id)
        .into_iter()
        .filter(|id| *id != n.id)
        .collect();
    let leaves = below
        .iter()
        .filter_map(|id| doc.node(*id))
        .filter(|c| c.children.is_empty());
    let mut any = false;
    for leaf in leaves {
        if !shape(leaf) {
            return false;
        }
        any = true;
    }
    small && grouped && any
}

/// `bytes` as a `data:` URL, its type sniffed.
pub(super) fn data_url(bytes: &[u8]) -> String {
    let mime = match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, ..] => "image/jpeg",
        [b'G', b'I', b'F', ..] => "image/gif",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "image/webp",
        _ if bytes.trim_ascii_start().starts_with(b"<") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

pub(super) fn px(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}px", r as i64)
    } else {
        format!("{r}px")
    }
}
