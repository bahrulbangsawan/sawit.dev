//! The server list: search, the filter tabs with their counts, kind, sort and paging, and
//! moving through it from the keyboard.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_catalog::{Auth, SERVERS, Server};

use super::{Filter, Kind, McpPage, Sheet, Sort};
use crate::catalog;

impl McpPage {
    /// The search field: typing filters, ↩ moves the keyboard to the first row.
    pub(super) fn make_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search servers"));
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
        window.focus(&self.page_focus, cx);
        cx.notify();
    }

    /// ↩: in the install sheet, Install (when it can); after a sign-in, Done;
    /// on the list, the selected server's sheet.
    pub(super) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.sheet {
            Some(Sheet::Install(sheet)) => {
                let s = sheet.server;
                let busy = (0..self.targets.len()).any(|t| self.busy.contains(&(s, t)));
                if !busy && !self.install_blocked(s, cx) && !self.selected_for(false).is_empty() {
                    self.install_selected(cx);
                }
            }
            Some(Sheet::SignIn(flow)) => {
                if flow.pos >= flow.queue.len() {
                    self.close_sheet(cx);
                }
            }
            None => {
                let rows = self.shown(&self.query(cx));
                if !self.loading
                    && !self.targets.is_empty()
                    && let Some(&s) = self.cursor.and_then(|ix| rows.get(ix))
                    // As the row's Install: off for GitHub's server while signed out.
                    && !self.row_gated(s, cx)
                {
                    self.open_install(s, window, cx);
                }
            }
        }
    }

    /// Clients that have server `s`, counting other versions too.
    pub(super) fn installs(&self, s: usize) -> usize {
        (0..self.targets.len())
            .filter(|t| self.is_installed((s, *t)))
            .count()
    }

    pub(super) fn matches(s: usize, query: &str) -> bool {
        let server = &SERVERS[s];
        let has = |t: &str| t.to_lowercase().contains(query);
        query.is_empty()
            || has(server.name)
            || has(server.id)
            || has(server.about)
            || has(Kind::of(s).label())
            || Self::tags(server).iter().any(|t| has(t))
    }

    /// The tags a row shows beside the server's kind.
    pub(super) fn tags(server: &Server) -> Vec<&'static str> {
        match server.auth {
            Auth::Browser => vec!["Sign-in"],
            Auth::EnvKey(_) => vec!["API key"],
            Auth::GitHub => vec!["GitHub sign-in"],
            Auth::None | Auth::Program => Vec::new(),
        }
    }

    /// Matches the search and the kind: what the filter tabs count.
    fn in_scope(&self, s: usize, query: &str) -> bool {
        Self::matches(s, query) && self.kind.is_none_or(|k| Kind::of(s) == k)
    }

    fn passes(&self, s: usize, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Installed => self.installs(s) > 0,
            Filter::Missing => self.installs(s) == 0,
            Filter::SignIn => !self.pending_sign_ins(Some(s)).is_empty(),
        }
    }

    /// The servers the search, kind, tab and sort leave.
    pub(super) fn shown(&self, query: &str) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..SERVERS.len())
            .filter(|s| self.in_scope(*s, query) && self.passes(*s, self.filter))
            .collect();
        match self.sort {
            Sort::Catalog => rows.sort_by_key(|s| (Kind::of(*s), *s)),
            Sort::Name => rows.sort_by_key(|s| SERVERS[*s].name.to_lowercase()),
            Sort::NameDesc => {
                rows.sort_by_key(|s| std::cmp::Reverse(SERVERS[*s].name.to_lowercase()))
            }
            Sort::Installed => rows.sort_by_key(|s| (std::cmp::Reverse(self.installs(*s)), *s)),
        }
        rows
    }

    /// Search, the filter tabs with their counts, kind and sort.
    pub(super) fn render_toolbar(
        &self,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let scope: Vec<usize> = (0..SERVERS.len())
            .filter(|s| self.in_scope(*s, query))
            .collect();
        let choices = Filter::ALL
            .iter()
            .map(|f| {
                (
                    f.label(),
                    scope.iter().filter(|s| self.passes(**s, *f)).count(),
                )
            })
            .collect();
        let current = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut kinds = vec!["All Types"];
        kinds.extend(Kind::ALL.iter().map(|k| k.label()));
        let kind = self
            .kind
            .and_then(|k| Kind::ALL.iter().position(|x| *x == k))
            .map_or(0, |ix| ix + 1);
        let sort = Sort::ALL.iter().position(|s| *s == self.sort).unwrap_or(0);
        h_flex()
            .gap_2()
            .flex_wrap()
            .when_some(self.search.as_ref(), |el, search| {
                el.child(catalog::search(search, cx))
            })
            .child(catalog::tabs(
                "mcp-filter",
                choices,
                current,
                cx.listener(|this, ix: &usize, _, cx| {
                    this.filter = Filter::ALL[*ix];
                    this.paging.page = 0;
                    this.cursor = None;
                    cx.notify();
                }),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .ml_auto()
                    .child(catalog::menu(
                        "mcp-kind",
                        Lucide::Funnel,
                        kinds[kind].to_string(),
                        kinds,
                        kind,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.kind = ix.checked_sub(1).map(|ix| Kind::ALL[ix]);
                            this.paging.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    ))
                    .child(catalog::menu(
                        "mcp-sort",
                        Lucide::ArrowUpDown,
                        format!("Sort: {}", self.sort.label()),
                        Sort::ALL.iter().map(|s| s.label()).collect(),
                        sort,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.sort = Sort::ALL[*ix];
                            this.paging.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    )),
            )
    }

    /// The list card: column titles, a page of servers and the pager, or
    /// the empty state.
    pub(super) fn render_list(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        if self.loading {
            return h_flex()
                .py_12()
                .gap_2()
                .justify_center()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.border)
                .text_sm()
                .text_color(muted)
                .child(Spinner::new().small().color(muted))
                .child("Detecting clients…")
                .into_any_element();
        }
        let rows = self.shown(query);
        let total = rows.len();
        let card = v_flex()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        if total == 0 {
            let title = if query.is_empty() {
                "No servers here".to_string()
            } else {
                format!("No servers match “{query}”")
            };
            return card
                .child(catalog::empty(
                    title,
                    Button::new("mcp-clear-filters")
                        .outline()
                        .small()
                        .icon(Lucide::FunnelX)
                        .label("Clear Filters")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.clear_filters(window, cx)),
                        ),
                    cx,
                ))
                .into_any_element();
        }
        let selected = self.cursor.and_then(|c| rows.get(c)).copied();
        let (rows, paging) = catalog::page_of(rows, self.paging);
        card.child(catalog::header(None, "Server", cx))
            .children(
                rows.into_iter()
                    .map(|s| self.render_server(s, selected == Some(s), cx)),
            )
            .child(catalog::pager(
                "mcp",
                paging,
                total,
                if total == 1 { "server" } else { "servers" },
                self.cursor,
                cx.listener(|this, paging: &catalog::Paging, _, cx| {
                    // A turn of the page leaves the selected row behind; a new size keeps it
                    // shown.
                    if paging.size == this.paging.size {
                        this.cursor = None;
                    }
                    this.paging = *paging;
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element()
    }

    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = Filter::All;
        self.kind = None;
        self.paging.page = 0;
        if let Some(search) = &self.search {
            search.update(cx, |s, cx| s.set_value("", window, cx));
        }
        cx.notify();
    }
}
