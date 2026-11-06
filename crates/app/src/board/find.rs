//! ⌘F on the canvas: the find bar above the canvas finds the layers whose text (text layers,
//! stickies, shapes' labels) or name matches, on the page shown or, with All Pages, on every page.
//! The current match is selected and zoomed to, its page opened first. The layers panel's filter
//! narrows the panel by name; this steps through the canvas. A live web frame, entered, gets its
//! keys from AppKit, ⌘F among them as the browser's page does: the canvas's bar is for the canvas.

use gpui_kit::*;
use sawit_board::layers::find::find_layers;
use sawit_core::find::{FindOptions, FindState, matcher};
use serde_json::json;

use super::BoardView;
use crate::find::{FindEvent, Finder, Supports};
use figcraft_doc::NodeId;

/// The canvas's find: its bar, and the layers it found.
#[derive(Default)]
pub(super) struct BoardFind {
    pub(super) finder: Finder,
    found: Vec<NodeId>,
    state: FindState,
}

impl BoardView {
    /// ⌘F: the bar above the canvas.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let supports = Supports {
            pages: true,
            ..Supports::ALL
        };
        self.find
            .finder
            .open(Some(supports), None, false, Self::on_find_event, window, cx);
    }

    fn on_find_event(&mut self, event: &FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FindEvent::Changed => self.find_again(cx),
            FindEvent::Step(forward) => self.find_step(*forward, cx),
            FindEvent::Close => self.close_find(window, cx),
            FindEvent::ReplaceOne | FindEvent::ReplaceAll => {}
        }
    }

    /// Escape in the bar or its ✕: the keyboard goes back to the canvas; what was found stays
    /// selected.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.clone();
        if self.find.finder.close(&focus, window, cx) {
            self.find = BoardFind::default();
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    /// ⌘G, ⇧⌘G (while the bar has the keyboard; on the canvas they group and ungroup), ↩ and ⇧↩
    /// in the bar, its arrows.
    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.find.finder.is_open() {
            cx.propagate();
            return;
        }
        if forward {
            self.find.state.next();
        } else {
            self.find.state.previous();
        }
        self.show_found(cx);
    }

    /// Finds the layers again, for a new query or option.
    fn find_again(&mut self, cx: &mut Context<Self>) {
        let Some(bar) = self.find.finder.bar().cloned() else {
            return;
        };
        let (query, options, all): (String, FindOptions, bool) = bar.read_with(cx, |bar, cx| {
            (bar.query(cx), bar.options(), bar.all_pages())
        });
        let regex = match matcher(&query, options) {
            Ok(regex) => regex,
            Err(problem) => {
                self.find.found.clear();
                self.find.state = FindState::default();
                bar.update(cx, |bar, cx| {
                    bar.set_found(FindState::default(), Some(problem), cx)
                });
                return;
            }
        };
        let found = match (regex, self.board()) {
            (Some(regex), Some(board)) => match board.doc() {
                Ok(doc) => {
                    let current = board.session.page().ok();
                    let pages: Vec<NodeId> = if all {
                        doc.pages.clone()
                    } else {
                        current.into_iter().collect()
                    };
                    find_layers(&doc, &pages, &regex)
                }
                Err(_) => Vec::new(),
            },
            _ => Vec::new(),
        };
        self.find.state = FindState::found(found.len(), None);
        self.find.found = found;
        self.show_found(cx);
    }

    /// Selects the current match and zooms to it, opening its page first; tells the bar.
    fn show_found(&mut self, cx: &mut Context<Self>) {
        let state = self.find.state;
        if let Some(id) = state
            .current
            .and_then(|ix| self.find.found.get(ix).copied())
        {
            let page = self
                .board()
                .and_then(|b| Some((b.doc().ok()?.page_of(id)?, b.session.page().ok()?)));
            if let Some((page, shown)) = page
                && page != shown
            {
                self.choose("page.select", json!({ "id": page }), cx);
            }
            self.choose("select.set", json!({ "ids": [id] }), cx);
            self.choose("view.zoomSelection", json!({}), cx);
        }
        if let Some(bar) = self.find.finder.bar() {
            bar.update(cx, |bar, cx| bar.set_found(state, None, cx));
        }
        cx.notify();
    }
}
