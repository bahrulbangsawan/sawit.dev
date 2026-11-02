//! Lane 2 (mesurer-agents): mesurer's comments and markup for agents.
//!
//! - Comment threads are rows of the Annotate panel, one list with Sawit's notes ([`rows`]): each
//!   workspace mesurer saves is read for the page's threads, which add, update and drop rows. A row
//!   removed in the panel deletes its thread, and Undo puts it back ([`edit`]).
//! - The page's other markup (text notes, arrows, strokes, guides…) goes out with the notes, its
//!   marks named by the elements under them, which the page's agent finds ([`markup`]).
//! - `comments.copied` and `comments.resolved` from the world note "Copy for agent" and "Resolve
//!   all" in the panel ([`handle`]).
//! - Agents' `mesurer` op beyond `state` (`open`, `close`, `set`, `clear`), which `op.rs` hands
//!   here ([`op`]).

mod edit;
mod markup;
mod rows;

use std::collections::{HashMap, HashSet};

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::annotations::Near;
use sawit_core::annotations::mesurer::edit::Taken;
use sawit_core::browser::mesurer::{FromMesurer, origin_key, page_key};
use serde_json::{Map, Value};

use super::super::engine::WorldReply;
use super::super::{BrowserView, OpResult};

/// Lane 2's state: the threads made rows, the ones taken by the panel, what the page's marks are
/// on.
#[derive(Default)]
pub(in crate::browser) struct AgentsState {
    /// The pages (origin key, page key) whose threads were made rows this session. A thread new
    /// to one of them was just written, and its row gets a screenshot; on a page's first look
    /// (after a launch), its threads were there before, and theirs don't.
    synced: HashSet<(String, String)>,
    /// Threads whose rows are being made (their element read, their screenshot taken).
    making: HashSet<String>,
    /// Threads a removal in the panel took from mesurer, kept while Undo can put them back, by
    /// id: the origin and page they were on, and where.
    taken: HashMap<String, (String, String, Taken)>,
    /// What the page's agent named under the marks of the page `near_page` (origin, page key),
    /// by point key.
    near: HashMap<String, Option<Near>>,
    near_page: Option<(String, String)>,
    resolving: Option<Task<()>>,
    /// The last snapshot given to mesurer (`SawitMesurer.apply`), or that would have been while
    /// it wasn't on the page.
    pub last_apply: Option<Value>,
}

/// How many taken threads are kept for Undo; past it, the oldest go.
const TAKEN_KEPT: usize = 64;

/// The page on show, by mesurer's keys: its address, origin key and page key.
fn here(view: &BrowserView) -> (String, String, String) {
    let url = view.current_url();
    let origin = origin_key(&url);
    let page = page_key(&url);
    (url, origin, page)
}

/// mesurer's workspace changed (it saved, a mount read it, Sawit changed it): the page's
/// threads and the rows follow, and the elements under its marks are looked for again.
pub(super) fn workspace_changed(view: &mut BrowserView, cx: &mut Context<BrowserView>) {
    rows::sync(view, cx);
    markup::resolve_soon(view, cx);
}

/// Answers a comments kind ([`sawit_core::browser::mesurer::Lane::Agents`]).
pub(super) fn handle(
    view: &mut BrowserView,
    message: FromMesurer,
    reply: WorldReply,
    cx: &mut Context<BrowserView>,
) {
    match message {
        // mesurer's text is on the pasteboard already (`clipboard.text`).
        FromMesurer::CommentsCopied { .. } => {
            let (_, origin, page) = here(view);
            (view.annotate).update(cx, |a, cx| a.comments_copied(&origin, &page, cx));
        }
        FromMesurer::CommentsResolved {} => {
            // Before the save that follows: the threads it resolved are still open here.
            let n = view.mesurer.summary.comments_open;
            view.annotate.update(cx, |a, cx| {
                a.set_note(
                    match n {
                        1 => "Resolved 1 comment".to_string(),
                        n => format!("Resolved {n} comments"),
                    },
                    cx,
                )
            });
        }
        other => {
            reply.err(format!("“{}” isn't the agents'.", other.kind()));
            return;
        }
    }
    reply.ok_value(&Value::Null);
}

/// Agents' `mesurer` op, `action` with `args`; the receiver gets the answer.
pub(super) fn op(
    view: &mut BrowserView,
    action: &str,
    args: &Map<String, Value>,
    cx: &mut Context<BrowserView>,
) -> oneshot::Receiver<OpResult> {
    let (tx, rx) = oneshot::channel();
    let answer = match action {
        "open" => return edit::open(view, cx),
        "close" => Ok(edit::close(view, cx)),
        "set" => return edit::set(view, args, cx),
        "clear" => edit::clear(view, args, cx),
        other => Err(format!(
            "unknown action “{other}”: mesurer's op takes state, open, close, set or clear."
        )),
    };
    let _ = tx.send(answer);
    rx
}
