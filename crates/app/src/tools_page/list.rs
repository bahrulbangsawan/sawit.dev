//! The Dev Tools list's search, filter tabs, category and sort, and its keys: the rows they
//! leave, and the row ↑ and ↓ select and ↩ acts on.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::dev_tools::{TOOLS, Tool};

use super::{Filter, Job, Sort, State, ToolsPage, category_name};
use crate::catalog;

impl ToolsPage {
    /// The search field: typing filters, ↩ moves the keyboard to the first row.
    pub(super) fn make_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search tools"));
        self._search = Some(cx.subscribe_in(
            &search,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                // Only when the text changed: the field also says so when it loses the
                // keyboard.
                InputEvent::Change if this.query(cx) != this.last_query => {
                    this.last_query = this.query(cx);
                    this.paging.page = 0;
                    this.cursor = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.cursor = None;
                    this.select(true, window, cx);
                }
                _ => {}
            },
        ));
        self.search = Some(search);
    }

    /// Gives the search field the keyboard, as opening the page does.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_none() {
            self.make_search(window, cx);
        }
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    /// Selects the next (or previous) row, turning the list's page to it, and
    /// gives the list the keyboard.
    pub(super) fn select(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.shown(&self.query(cx)).len();
        self.cursor = catalog::step(self.cursor, len, down);
        if let Some(ix) = self.cursor {
            self.paging.show(ix);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// ↩ on a row: what its main button does, when that's safe from one key: Install,
    /// Upgrade, Retry of an install or upgrade, and taking it out of the queue. Never
    /// Reinstall, Stop, Uninstall or Retry of an uninstall.
    pub(super) fn confirm(&mut self, cx: &mut Context<Self>) {
        let rows = self.shown(&self.query(cx));
        let Some(&(_, tool)) = self.cursor.and_then(|ix| rows.get(ix)) else {
            return;
        };
        let id = tool.id;
        match self.states.get(id) {
            Some(State::Queued { .. }) => self.dequeue(id, cx),
            Some(State::Failed(_, Some(Job::Install)) | State::Skipped(_)) => {
                self.enqueue(&[id], cx)
            }
            // An uninstall that failed, or a copy found after it: not from one key, and never
            // an install over the copy the user meant to remove.
            Some(State::Failed(..) | State::Installing | State::Uninstalling) => {}
            _ if self.newer(id).is_some() => self.upgrade(&[id], cx),
            _ if !self.installed(id) && !self.busy(id) => self.enqueue(&[id], cx),
            _ => {}
        }
    }

    /// What's typed in the search field, lowercased.
    pub(super) fn query(&self, cx: &App) -> String {
        self.search
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_lowercase())
            .unwrap_or_default()
    }

    fn matches(tool: &Tool, query: &str) -> bool {
        let has = |s: &str| s.to_lowercase().contains(query);
        query.is_empty()
            || has(tool.name)
            || has(tool.id)
            || has(tool.description)
            || has(category_name(tool.category))
    }

    /// Matches the search and the category: what the filter tabs count.
    pub(super) fn in_scope(&self, tool: &Tool, query: &str) -> bool {
        Self::matches(tool, query) && self.category.is_none_or(|c| tool.category == c)
    }

    pub(super) fn passes(&self, tool: &Tool, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Installed => self.installed(tool.id),
            Filter::Updates => self.newer(tool.id).is_some(),
            Filter::Missing => !self.installed(tool.id),
        }
    }

    /// Sorts by what needs attention: running or failed, then updates,
    /// then missing, then installed.
    fn rank(&self, tool: &Tool) -> u8 {
        match self.states.get(tool.id) {
            Some(State::Removed) | None => {}
            Some(State::Done) => return 3,
            Some(_) => return 0,
        }
        if self.newer(tool.id).is_some() {
            1
        } else if self.installed(tool.id) {
            3
        } else {
            2
        }
    }

    /// The rows the search, category, tab and sort leave, with their index
    /// in `TOOLS`.
    pub(super) fn shown(&self, query: &str) -> Vec<(usize, &'static Tool)> {
        let mut rows: Vec<(usize, &'static Tool)> = TOOLS
            .iter()
            .enumerate()
            .filter(|(_, t)| self.in_scope(t, query) && self.passes(t, self.filter))
            .collect();
        match self.sort {
            Sort::Category => rows.sort_by_key(|(ix, t)| (t.category as u8, *ix)),
            Sort::Name => rows.sort_by_key(|(_, t)| t.name.to_lowercase()),
            Sort::NameDesc => rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.name.to_lowercase())),
            Sort::Status => rows.sort_by_key(|(ix, t)| (self.rank(t), *ix)),
        }
        rows
    }

    /// Selectable rows (missing, idle, not handed to the terminal).
    pub(super) fn selectable(&self, rows: &[(usize, &'static Tool)]) -> Vec<&'static str> {
        rows.iter()
            .filter(|(_, t)| !t.interactive && !self.installed(t.id) && !self.busy(t.id))
            .map(|(_, t)| t.id)
            .collect()
    }

    pub(super) fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = Filter::All;
        self.category = None;
        self.paging.page = 0;
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }
}
