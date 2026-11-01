//! The Skills list's search, filter tabs, category and sort, and its keys: the packs they
//! leave, and the row ↑ and ↓ select and ↩ opens.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::skills::{Action, Category, PACKS, Pack};

use super::{Filter, Pick, SkillsPage, Sort};
use crate::catalog;

impl SkillsPage {
    /// The search field: typing filters, ↩ moves the keyboard to the first row.
    pub(super) fn make_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
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
        if self.sheet.is_some() {
            return;
        }
        if self.search.is_none() {
            self.make_search(window, cx);
        }
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    pub(super) fn query(&self, cx: &App) -> String {
        self.search
            .as_ref()
            .map(|s| s.read(cx).value().trim().to_lowercase())
            .unwrap_or_default()
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

    /// ↩: in the sheet, Install (when it can); on the list, the selected
    /// pack's sheet. Never Remove.
    pub(super) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sheet) = &self.sheet {
            let pack = sheet.pack;
            let install = self.picked(Pick::Install);
            if !install.is_empty() && !self.busy(pack.id) && self.blocked(sheet.method).is_none() {
                self.run_sheet(Action::Install, cx);
            }
            return;
        }
        let rows = self.shown(&self.query(cx));
        if !self.loading
            && let Some(&pack) = self.cursor.and_then(|ix| rows.get(ix))
        {
            self.open_sheet(pack, window, cx);
        }
    }

    // The list

    fn matches(pack: &Pack, query: &str) -> bool {
        let has = |s: &str| s.to_lowercase().contains(query);
        query.is_empty()
            || has(pack.name)
            || has(pack.id)
            || has(pack.by)
            || has(pack.about)
            || has(pack.category.name())
            || pack.skills.iter().any(|s| has(s))
    }

    /// Matches the search and the category: what the filter tabs count.
    pub(super) fn in_scope(&self, pack: &Pack, query: &str) -> bool {
        Self::matches(pack, query) && self.category.is_none_or(|c| pack.category == c)
    }

    pub(super) fn passes(&self, pack: &Pack, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Installed => !self.having(pack).is_empty(),
            Filter::Missing => self.having(pack).is_empty(),
        }
    }

    /// The packs the search, category, tab and sort leave.
    pub(super) fn shown(&self, query: &str) -> Vec<&'static Pack> {
        let mut rows: Vec<(usize, &'static Pack)> = PACKS
            .iter()
            .enumerate()
            .filter(|(_, p)| self.in_scope(p, query) && self.passes(p, self.filter))
            .collect();
        match self.sort {
            Sort::Catalog => rows
                .sort_by_key(|(ix, p)| (Category::ALL.iter().position(|c| *c == p.category), *ix)),
            Sort::Name => rows.sort_by_key(|(_, p)| p.name.to_lowercase()),
            Sort::NameDesc => rows.sort_by_key(|(_, p)| std::cmp::Reverse(p.name.to_lowercase())),
            Sort::Installed => {
                rows.sort_by_key(|(ix, p)| (std::cmp::Reverse(self.having(p).len()), *ix))
            }
        }
        rows.into_iter().map(|(_, p)| p).collect()
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
