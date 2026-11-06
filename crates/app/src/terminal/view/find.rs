//! ⌘F in a terminal pane: the find bar over the pane's top right, searching the scrollback and the
//! screen, a line wrapped over several rows read as one. Every match on screen is highlighted and
//! the current one more strongly ([`sawit_core::terminal::FindMarks`], which each snapshot marks
//! from the grid as it is then, so new output never leaves a highlight behind); the count follows
//! the output a moment after it arrives. The search starts from the newest match, at the bottom.
//! ⌘F is Sawit's and never reaches the program; ⌃F stays the program's.

use std::time::Duration;

use gpui_kit::*;
use sawit_core::find::{FindState, Regex, matcher};
use sawit_core::terminal::{FindMarks, GridMatch};

use super::TerminalView;
use crate::find::{FindEvent, Finder, Supports};

/// The rows the find bar covers at the pane's top, which a match scrolled to stays below.
const BAR_ROWS: usize = 3;

/// How long after output the matches are counted again.
const RECOUNT_AFTER: Duration = Duration::from_millis(200);

/// A pane's find: its bar, and the matches it last counted.
#[derive(Default)]
pub(super) struct TerminalFind {
    pub(super) finder: Finder,
    regex: Option<Regex>,
    matches: Vec<GridMatch>,
    state: FindState,
    /// The lines in the scrollback when the matches were counted.
    history: usize,
    recount: Option<Task<()>>,
}

impl TerminalView {
    /// ⌘F: opens the bar, its query the selection if there is one.
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let seed = self.session.selection_text();
        self.find.finder.open(
            Some(Supports::ALL),
            seed.as_deref(),
            false,
            Self::on_find_event,
            window,
            cx,
        );
    }

    fn on_find_event(&mut self, event: &FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FindEvent::Changed => self.find_again(false, cx),
            FindEvent::Step(forward) => self.find_step(*forward, cx),
            FindEvent::Close => self.close_find(window, cx),
            FindEvent::ReplaceOne | FindEvent::ReplaceAll => {}
        }
    }

    /// Escape in the bar, or its ✕: the highlights go, and the keyboard comes back to the pane.
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.clone();
        if self.find.finder.close(&focus, window, cx) {
            self.find = TerminalFind::default();
            self.session.set_find(None);
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    /// ⌘G and ⇧⌘G, ↩ and ⇧↩ in the bar, its arrows: the next match down, or up.
    pub(super) fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.find.finder.is_open() {
            cx.propagate();
            return;
        }
        if forward {
            self.find.state.step_next();
        } else {
            self.find.state.step_previous();
        }
        self.show_current(true, cx);
    }

    /// Counts the matches again: for a new query (`keep` false: from the newest match), or for new
    /// output (`keep`: the same match stays current).
    fn find_again(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some(bar) = self.find.finder.bar().cloned() else {
            return;
        };
        let (query, options) = bar.read_with(cx, |bar, cx| (bar.query(cx), bar.options()));
        let regex = match matcher(&query, options) {
            Ok(regex) => regex,
            Err(problem) => {
                self.find.regex = None;
                self.find.matches.clear();
                self.session.set_find(None);
                bar.update(cx, |bar, cx| {
                    bar.set_found(FindState::default(), Some(problem), cx)
                });
                return;
            }
        };
        let Some(regex) = regex else {
            self.find.regex = None;
            self.find.matches.clear();
            self.find.state = FindState::default();
            self.session.set_find(None);
            bar.update(cx, |bar, cx| bar.set_found(FindState::default(), None, cx));
            cx.notify();
            return;
        };
        let current = self.current_match();
        let (matches, history) = self.session.find_all(&regex);
        // The match that was current, where the output has moved it.
        let moved = history as i32 - self.find.history as i32;
        let near = current.filter(|_| keep).and_then(|m| {
            let mut start = *m.start();
            start.line -= moved;
            matches.iter().position(|m| *m.start() == start)
        });
        let total = matches.len();
        self.find.state = FindState::found(total, near.or(total.checked_sub(1)));
        self.find.matches = matches;
        self.find.history = history;
        self.find.regex = Some(regex);
        self.show_current(!keep, cx);
    }

    /// The current match, as last counted.
    fn current_match(&self) -> Option<GridMatch> {
        let ix = self.find.state.current?;
        self.find.matches.get(ix).cloned()
    }

    /// Tells the screen what to highlight and the bar its count; scrolls the current match into
    /// view when `reveal`.
    fn show_current(&mut self, reveal: bool, cx: &mut Context<Self>) {
        let current = self.current_match();
        if let Some(regex) = self.find.regex.clone() {
            self.session.set_find(Some(FindMarks {
                regex,
                current: current.clone(),
                history: self.find.history,
            }));
        }
        if reveal && let Some(m) = &current {
            let moved = self.session.history_size() as i32 - self.find.history as i32;
            self.session.reveal_line(m.start().line.0 - moved, BAR_ROWS);
        }
        let state = self.find.state;
        if let Some(bar) = self.find.finder.bar() {
            bar.update(cx, |bar, cx| bar.set_found(state, None, cx));
        }
        cx.notify();
    }

    /// New output while the bar is open: the matches are counted again a moment later, once.
    pub(super) fn recount_soon(&mut self, cx: &mut Context<Self>) {
        if self.find.regex.is_none() || self.find.recount.is_some() {
            return;
        }
        self.find.recount = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RECOUNT_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.find.recount = None;
                this.find_again(true, cx);
            });
        }));
    }
}
