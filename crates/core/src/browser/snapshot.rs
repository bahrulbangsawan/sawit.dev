//! The page as an outline (`browser_snapshot`), written from what the page agent's `snapshot` op
//! collects (`browser/page/agent/elements.js`): one [`Node`] per line, with its depth, role,
//! name, value and state. The page walks the DOM and shadow roots, works out accessible names
//! and visibility, and keeps the refs it hands out (it acts on them); the text is written here.
//!
//! The page sends names, values and text already clipped and quoted as `JSON.stringify` writes
//! them: it clips in UTF-16 units, which can split a surrogate pair, and only its own quoting
//! keeps such a half pair readable (as `\udXXX`) once the answer reaches Rust.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// At most this many lines; the page stops walking there.
pub const MAX_LINES: usize = 2000;

/// One line of the outline.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    /// Indentation, in levels.
    pub d: usize,
    /// A text node's text, quoted; a text line has nothing else.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The element's role (`button`, `clickable`, …).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// Its accessible name, quoted; empty when it has none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// A heading's level, as the page wrote the number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// A field's value, quoted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// `checked`, `expanded` or `collapsed`, `selected`, `disabled`, `required`, `focused`, in
    /// that order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    /// A link's address, clipped (not quoted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Its ref's number (`e<ref>`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<u32>,
    /// It holds the lines below it that are deeper (a container); a leaf doesn't.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub holds: bool,
}

impl Node {
    /// The line, without its indentation and its container colon.
    fn line(&self) -> String {
        if let Some(text) = &self.text {
            return format!("text: {text}");
        }
        let mut out = self.role.clone();
        if !self.name.is_empty() {
            out.push(' ');
            out.push_str(&self.name);
        }
        if let Some(level) = &self.level {
            out.push_str(&format!(" [level={level}]"));
        }
        if let Some(value) = &self.value {
            out.push_str(" value=");
            out.push_str(value);
        }
        for state in &self.states {
            out.push_str(&format!(" [{state}]"));
        }
        if let Some(url) = &self.url {
            out.push_str(" /url: ");
            out.push_str(url);
        }
        if let Some(r) = self.r#ref {
            out.push_str(&format!(" [ref=e{r}]"));
        }
        out
    }
}

/// The outline's text: a line per node, indented two spaces a level, a container's line ending
/// in `:` when lines are nested under it, and a note when the page stopped at [`MAX_LINES`].
pub fn outline(nodes: &[Node], truncated: bool) -> String {
    let mut out = String::new();
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for _ in 0..node.d {
            out.push_str("  ");
        }
        out.push_str("- ");
        out.push_str(&node.line());
        if node.holds && nodes.get(i + 1).is_some_and(|next| next.d > node.d) {
            out.push(':');
        }
    }
    if truncated {
        out.push_str(&format!(
            "\n… (cut at {MAX_LINES} lines; scroll or use a selector)"
        ));
    }
    out
}

/// The page's `snapshot` answer (`url`, `title`, `ready`, `refs`, `truncated`, `modal`,
/// `nodes`) as agents get it: `nodes` written as the `snapshot` text, the rest as it came.
pub fn answer(mut page: Value) -> Value {
    let Some(map) = page.as_object_mut() else {
        return page;
    };
    let Some(nodes) = map.remove("nodes") else {
        return page;
    };
    let nodes: Vec<Node> = serde_json::from_value(nodes).unwrap_or_default();
    let truncated = map
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // `nodes` comes last, so `snapshot` takes its place.
    map.insert("snapshot".into(), outline(&nodes, truncated).into());
    page
}

#[cfg(test)]
#[path = "tests/snapshot.rs"]
mod tests;
