//! Comments: threads pinned to a point of a page, or to a layer they follow, each a first message
//! and its replies, resolved or open. The Comment tool (C) asks for a new thread where it is
//! clicked (`comment.new`); the canvas draws each open thread's pin while View ▸ Comments is on
//! ([`pins`]), and the Comments window lists the page's threads ([`threads`]). Each edit is one of
//! the engine's `comment.*` commands, one undo step.

use figcraft_doc::{Document, NodeId};
use serde_json::json;

use super::Edit;

/// One message of a thread.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageRow {
    pub author: String,
    pub text: String,
    /// Written by an AI agent.
    pub agent: bool,
}

/// One thread as the window shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub id: u64,
    /// Where its pin is, in page coordinates.
    pub at: (f64, f64),
    pub resolved: bool,
    /// The first message, then the replies.
    pub messages: Vec<MessageRow>,
    /// The layer it is pinned to.
    pub anchor: Option<NodeId>,
}

/// The threads on `page`, oldest first.
pub fn threads(doc: &Document, page: NodeId) -> Vec<Thread> {
    doc.comments
        .iter()
        .filter(|c| c.page == page)
        .map(|c| Thread {
            id: c.id,
            at: c.position(doc),
            resolved: c.resolved,
            messages: std::iter::once(&c.first)
                .chain(&c.replies)
                .map(|m| MessageRow {
                    author: m.author.clone(),
                    text: m.text.clone(),
                    agent: m.agent,
                })
                .collect(),
            anchor: c.anchor,
        })
        .collect()
}

/// The open threads' pins on `page`: each its id, where it is, and its number (1, 2, …, in the
/// order of all the page's threads).
pub fn pins(doc: &Document, page: NodeId) -> Vec<(u64, (f64, f64), usize)> {
    threads(doc, page)
        .into_iter()
        .enumerate()
        .filter(|(_, t)| !t.resolved)
        .map(|(n, t)| (t.id, t.at, n + 1))
        .collect()
}

/// The thread whose pin is within `reach` page units of (`x`, `y`) on `page`, the nearest.
pub fn pin_at(doc: &Document, page: NodeId, x: f64, y: f64, reach: f64) -> Option<u64> {
    pins(doc, page)
        .into_iter()
        .map(|(id, (px, py), _)| (id, (px - x).hypot(py - y)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// The edit that starts a thread saying `text` at page point (`x`, `y`), pinned to the innermost
/// frame there so that it follows it.
pub fn add(x: f64, y: f64, text: &str) -> Edit {
    Edit {
        command: "comment.add",
        params: json!({"x": x, "y": y, "text": text, "anchor": true}),
    }
}

/// The edit that replies `text` to thread `id`.
pub fn reply(id: u64, text: &str) -> Edit {
    Edit {
        command: "comment.reply",
        params: json!({"id": id, "text": text}),
    }
}

/// The edit that resolves thread `id`, or opens it again.
pub fn resolve(id: u64, resolved: bool) -> Edit {
    Edit {
        command: "comment.resolve",
        params: json!({"id": id, "resolved": resolved}),
    }
}

/// The edit that deletes thread `id` and its replies.
pub fn delete(id: u64) -> Edit {
    Edit {
        command: "comment.delete",
        params: json!({"id": id}),
    }
}

#[cfg(test)]
mod tests;
