//! Read-only table/view browser: pages of 50 rows ordered by the primary
//! key, fetched over a read-only connection. The key and row estimate come
//! from the schema tree, the count rides behind the first page on the same
//! connection, and the next page is fetched ahead, so a remote database
//! costs about one round trip per page shown.
//!
//! Sorting (a column header), filters and the row search run on the server,
//! so they cover every row, not only the page shown.

use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{self, FilterOp, ResultSet, RowCount, RowFilter, RowQuery, SortDir};
use tokio_postgres::Client;

use super::DbEvent;
use super::store::DbStore;
use crate::rt;
use crate::states::Placeholder;

/// Rows per page (the most a page ever shows).
pub const PAGE_SIZE: u64 = 50;

/// Typing pause before the row search runs.
const SEARCH_DELAY: Duration = Duration::from_millis(400);

type ColumnSelect = SelectState<SearchableVec<String>>;
type OpSelect = SelectState<SearchableVec<&'static str>>;

/// One "column operator value" condition in the filter panel.
struct FilterRow {
    column: Entity<ColumnSelect>,
    op: Entity<OpSelect>,
    value: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl FilterRow {
    fn op(&self, cx: &App) -> FilterOp {
        let label = self.op.read(cx).selected_value().copied();
        FilterOp::ALL
            .into_iter()
            .find(|op| Some(op.label()) == label)
            .unwrap_or(FilterOp::Eq)
    }

    /// The condition, unless it is missing a column or a needed value.
    fn filter(&self, cx: &App) -> Option<RowFilter> {
        let column = self.column.read(cx).selected_value()?.clone();
        let op = self.op(cx);
        let value = self.value.read(cx).value().to_string();
        (!op.needs_value() || !value.is_empty()).then_some(RowFilter { column, op, value })
    }
}

/// Which page is shown and what is known about the total.
#[derive(Debug, Default, Clone)]
pub struct Pager {
    pub page: u64,
    /// `None` while the count is still running.
    pub total: Option<RowCount>,
    /// Rows on the current page.
    pub page_rows: usize,
}

impl Pager {
    pub fn offset(&self) -> u64 {
        self.page * PAGE_SIZE
    }

    /// Index of the last page, when the total is exact.
    pub fn last_page(&self) -> Option<u64> {
        match self.total {
            Some(RowCount { rows, exact: true }) => Some(rows.saturating_sub(1) / PAGE_SIZE),
            _ => None,
        }
    }

    pub fn has_prev(&self) -> bool {
        self.page > 0
    }

    pub fn has_next(&self) -> bool {
        match self.last_page() {
            Some(last) => self.page < last,
            None => self.page_rows as u64 == PAGE_SIZE,
        }
    }

    pub fn label(&self) -> String {
        if self.page_rows == 0 && self.page == 0 {
            return "0 rows".into();
        }
        let range = format!(
            "{}–{}",
            self.offset() + 1,
            self.offset() + self.page_rows as u64
        );
        match self.total {
            Some(RowCount { rows, exact: true }) => format!("{range} of {rows}"),
            Some(RowCount { rows, exact: false }) => format!("{range} of ±{rows}"),
            None => range,
        }
    }
}

pub fn column_for(name: &str, data_type: &str) -> Column {
    let width = match data_type {
        t if t.contains("int") || t == "boolean" || t.contains("numeric") => 110.,
        t if t.contains("timestamp") => 200.,
        "uuid" => 290.,
        _ => 180.,
    };
    Column::new(
        SharedString::from(name.to_string()),
        SharedString::from(name.to_string()),
    )
    .width(px(width))
}

pub struct TableView {
    pub title: SharedString,
    db: Entity<DbStore>,
    profile: String,
    schema: String,
    table: String,
    /// Primary key, looked up with the first page.
    order_by: Option<Vec<String>>,
    pager: Pager,
    grid: Option<Entity<TableState<ResultDelegate>>>,
    loading: bool,
    error: Option<String>,
    /// Bumped per request so a slow, superseded response is ignored.
    generation: u64,
    /// Cancels the page query in flight.
    cancel: Option<tokio_postgres::CancelToken>,
    /// The connection died; reload once the store has reconnected.
    waiting_reconnect: bool,
    /// The page after the shown one, fetched ahead.
    prefetched: Option<(u64, ResultSet)>,
    /// Generation of the page load on the connection right now.
    in_flight: Option<u64>,
    /// Page asked for while another load was in flight; fetched when it
    /// finishes, so quick Next/Prev clicks don't queue a query each.
    queued: Option<u64>,
    /// The sort, filters and search the rows are loaded with.
    query: RowQuery,
    search: Entity<InputState>,
    /// Bumped per keystroke; a delayed search runs only if still current.
    search_rev: u64,
    filters: Vec<FilterRow>,
    show_filters: bool,
    /// The count for the current query was already asked for (a filtered
    /// count that timed out isn't retried on every page).
    counted: bool,
    /// Column names shown last, for the filter's column list.
    columns: Vec<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DbEvent> for TableView {}

impl TableView {
    /// Reads go through the profile's read-only connection in `db`.
    pub fn new(
        title: SharedString,
        db: Entity<DbStore>,
        profile: String,
        schema: String,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe_in(&db, window, |this: &mut Self, db, window, cx| {
            if this.waiting_reconnect && db.read(cx).is_connected(&this.profile) {
                this.waiting_reconnect = false;
                this.reload(window, cx);
            }
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search all columns"));
        let on_search = cx.subscribe_in(&search, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => this.apply_search(window, cx),
                InputEvent::Change => {
                    this.search_rev += 1;
                    let rev = this.search_rev;
                    cx.spawn_in(window, async move |this, window| {
                        window.background_executor().timer(SEARCH_DELAY).await;
                        let _ = this.update_in(window, |this, window, cx| {
                            if this.search_rev == rev {
                                this.apply_search(window, cx);
                            }
                        });
                    })
                    .detach();
                }
                _ => {}
            }
        });
        let mut this = Self {
            title,
            db,
            profile,
            schema,
            table,
            order_by: None,
            pager: Pager::default(),
            grid: None,
            loading: false,
            error: None,
            generation: 0,
            cancel: None,
            waiting_reconnect: false,
            prefetched: None,
            in_flight: None,
            queued: None,
            query: RowQuery::default(),
            search,
            search_rev: 0,
            filters: Vec::new(),
            show_filters: false,
            counted: false,
            columns: Vec::new(),
            _subscriptions: vec![observe, on_search],
        };
        this.reload(window, cx);
        this
    }

    /// Re-counts and reloads the current page.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pager.total = None;
        self.counted = false;
        self.order_by = None;
        self.prefetched = None;
        // A reload wants fresh data, not the result of a load started before.
        self.in_flight = None;
        self.queued = None;
        self.load_page(self.pager.page, window, cx);
    }

    /// The live read-only client, or `None` after asking the store to
    /// reconnect (the view reloads itself when that finishes).
    fn client(&mut self, cx: &mut Context<Self>) -> Option<Arc<Client>> {
        let client = self.db.read(cx).client(&self.profile, true);
        if client.is_none() {
            self.waiting_reconnect = true;
            let profile = self.profile.clone();
            self.db
                .update(cx, |s, cx| s.ensure_connecting(&profile, cx));
        }
        client
    }

    fn load_page(&mut self, page: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((p, set)) = self.prefetched.take()
            && p == page
            && self.error.is_none()
        {
            // Supersedes anything in flight or queued.
            self.generation += 1;
            self.queued = None;
            self.cancel = None;
            self.loading = false;
            self.pager.page = page;
            self.show(set, window, cx);
            self.prefetch(cx);
            cx.notify();
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();
        if self.in_flight.is_some() {
            // Cancelling the old query could race and cancel this one (the
            // server cancels whatever runs on the connection): wait instead.
            self.queued = Some(page);
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        let Some(client) = self.client(cx) else {
            return;
        };
        self.in_flight = Some(generation);
        self.cancel = Some(client.cancel_token());
        let (schema, table) = (self.schema.clone(), self.table.clone());
        let dead = client.clone();
        let cached = self
            .db
            .read(cx)
            .relation(&self.profile, &schema, &table)
            .map(|r| (r.primary_key.clone(), r.estimated_rows));
        let known_order = self
            .order_by
            .clone()
            .or(cached.as_ref().map(|c| c.0.clone()));
        let estimate = cached.and_then(|c| c.1);
        let count = self.pager.total.is_none() && !self.counted;
        self.counted = true;
        let query = self.query.clone();
        let (page_tx, page_rx) = oneshot::channel();
        let count_rx = rt::spawn(cx, async move {
            let pipelined = known_order.is_some();
            let page_done = async {
                let result = async {
                    let order = match known_order {
                        Some(o) => o,
                        None => db::primary_key(&client, &schema, &table).await?,
                    };
                    let set = db::fetch_rows(
                        &client,
                        &schema,
                        &table,
                        &order,
                        &query,
                        page * PAGE_SIZE,
                        PAGE_SIZE,
                    )
                    .await?;
                    Ok::<_, sawit_core::CoreError>((order, set))
                }
                .await;
                let ok = result.is_ok();
                let _ = page_tx.send(result);
                ok
            };
            let counted = async {
                match count {
                    // The estimate says nothing about a filtered subset.
                    true if query.is_filtered() => {
                        db::row_count_matching(&client, &schema, &table, &query)
                            .await
                            .ok()
                            .flatten()
                    }
                    true => db::row_count_estimated(&client, &schema, &table, estimate)
                        .await
                        .ok(),
                    false => None,
                }
            };
            // The page is sent first, so the server answers it before
            // starting the (possibly slow) count queued behind it.
            if pipelined {
                futures::join!(page_done, counted).1
            } else if page_done.await {
                counted.await
            } else {
                None
            }
        });
        cx.spawn_in(window, async move |this, window| {
            let result = page_rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                if this.in_flight == Some(generation) {
                    this.in_flight = None;
                    if let Some(next) = this.queued.take()
                        && next != page
                    {
                        this.load_page(next, window, cx);
                        return;
                    }
                }
                if this.generation != generation {
                    return;
                }
                this.cancel = None;
                if dead.is_closed() {
                    // Dropped mid-request: reconnect and load again.
                    this.pager.total = None;
                    this.counted = false;
                    this.order_by = None;
                    if this.client(cx).is_none() {
                        cx.notify();
                        return;
                    }
                }
                this.loading = false;
                match result {
                    Ok(Ok((order, set))) => {
                        this.order_by = Some(order);
                        this.pager.page = page;
                        this.show(set, window, cx);
                        this.prefetch(cx);
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => this.error = Some("The query stopped unexpectedly. Try again.".into()),
                }
                cx.notify();
            });
            if let Ok(Some(total)) = count_rx.await {
                let _ = this.update(window, |this, cx| {
                    if this.pager.total.is_none() {
                        this.pager.total = Some(total);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Fetches the next page in the background so paging forward is instant.
    fn prefetch(&mut self, cx: &mut Context<Self>) {
        let next = self.pager.page + 1;
        let (Some(order), true) = (self.order_by.clone(), self.pager.has_next()) else {
            return;
        };
        let Some(client) = self.db.read(cx).client(&self.profile, true) else {
            return;
        };
        let generation = self.generation;
        let (schema, table, query) = (self.schema.clone(), self.table.clone(), self.query.clone());
        let rx = rt::spawn(cx, async move {
            db::fetch_rows(
                &client,
                &schema,
                &table,
                &order,
                &query,
                next * PAGE_SIZE,
                PAGE_SIZE,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(set)) = rx.await {
                let _ = this.update(cx, |this, _| {
                    if this.generation == generation {
                        this.prefetched = Some((next, set));
                    }
                });
            }
        })
        .detach();
    }

    fn show(&mut self, set: ResultSet, window: &mut Window, cx: &mut Context<Self>) {
        self.pager.page_rows = set.rows.len();
        if !set.columns.is_empty() {
            self.columns = set.columns.clone();
        }
        let first_row = self.pager.offset();
        let delegate = ResultDelegate::numbered(set, first_row)
            .sortable(self.query.sort.clone(), cx.entity().downgrade());
        self.grid = Some(cx.new(|cx| TableState::new(delegate, window, cx)));
    }

    /// Loads the first page again with the current query.
    fn requery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pager.page = 0;
        self.reload(window, cx);
    }

    /// From a column header: descending, then ascending, then the key order.
    fn set_sort(&mut self, column: String, sort: ColumnSort, window: &mut Window, cx: &mut Context<Self>) {
        self.query.sort = match sort {
            ColumnSort::Ascending => Some((column, SortDir::Asc)),
            ColumnSort::Descending => Some((column, SortDir::Desc)),
            ColumnSort::Default => None,
        };
        self.requery(window, cx);
    }

    fn apply_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = self.search.read(cx).value().trim().to_string();
        if search != self.query.search {
            self.query.search = search;
            self.requery(window, cx);
        }
    }

    fn apply_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let filters: Vec<RowFilter> = self.filters.iter().filter_map(|f| f.filter(cx)).collect();
        if filters != self.query.filters {
            self.query.filters = filters;
            self.requery(window, cx);
        }
    }

    /// Column names for the filter: the schema tree's, else the last page's.
    fn column_names(&self, cx: &App) -> Vec<String> {
        self.db
            .read(cx)
            .relation(&self.profile, &self.schema, &self.table)
            .map(|r| r.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| self.columns.clone())
    }

    fn add_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let columns = self.column_names(cx);
        let first = (!columns.is_empty()).then(|| IndexPath::new(0));
        let column = cx.new(|cx| ColumnSelect::new(SearchableVec::new(columns), first, window, cx));
        let ops: Vec<&'static str> = FilterOp::ALL.iter().map(|op| op.label()).collect();
        let op = cx.new(|cx| OpSelect::new(SearchableVec::new(ops), Some(IndexPath::new(0)), window, cx));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let subscriptions = vec![
            cx.subscribe(&op, |_, _, _: &SelectEvent<SearchableVec<&'static str>>, cx| cx.notify()),
            cx.subscribe_in(&value, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.apply_filters(window, cx);
                }
            }),
        ];
        value.update(cx, |s, cx| s.focus(window, cx));
        self.filters.push(FilterRow {
            column,
            op,
            value,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn remove_filter(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.filters.len() {
            self.filters.remove(ix);
        }
        self.apply_filters(window, cx);
        cx.notify();
    }

    /// Back to every row in key order.
    fn reset_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filters.clear();
        self.show_filters = false;
        // Matches the query below, so the search's Change doesn't reload again.
        self.query = RowQuery::default();
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.requery(window, cx);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.queued = None;
        if let Some(token) = self.cancel.take()
            && let Some(profile) = self.db.read(cx).profile(&self.profile).cloned()
        {
            // Fire and forget: the page request itself reports the outcome.
            drop(rt::spawn(
                cx,
                async move { db::cancel(&token, &profile).await },
            ));
        }
    }

    fn go(&mut self, page: u64, window: &mut Window, cx: &mut Context<Self>) {
        if page != self.pager.page || self.error.is_some() {
            self.load_page(page, window, cx);
        }
    }
}

impl Render for TableView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pager = self.pager.clone();
        let busy = self.loading;
        let page_label = match pager.last_page() {
            Some(last) => format!("Page {} / {}", pager.page + 1, last + 1),
            None => format!("Page {}", pager.page + 1),
        };
        let nav = |id: &'static str, icon: Lucide, tooltip: &'static str, enabled: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tooltip)
                .disabled(!enabled || busy)
        };
        let toolbar = h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_color(cx.theme().foreground)
                    .font_semibold()
                    .child(self.title.clone()),
            )
            .child(
                h_flex()
                    .gap_1()
                    .px_1p5()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().secondary)
                    .child(Icon::new(Lucide::Lock).xsmall())
                    .child("read-only"),
            )
            .child(div().flex_1())
            .child(
                Button::new("page-query")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::SquareTerminal)
                    .label("Query")
                    .tooltip("Open in a SQL Editor")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let sql = format!(
                            "SELECT *\nFROM {}.{}\nLIMIT 100;",
                            db::quote_ident(&this.schema),
                            db::quote_ident(&this.table)
                        );
                        cx.emit(DbEvent::OpenSql {
                            profile: this.profile.clone(),
                            sql: Some(sql),
                        });
                    })),
            )
            .when(busy && self.cancel.is_some(), |el| {
                el.child(
                    Button::new("page-cancel")
                        .ghost()
                        .xsmall()
                        .label("Cancel")
                        .tooltip("Cancel the Query on the Server")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                )
            })
            .child(if self.waiting_reconnect {
                "Reconnecting…".to_string()
            } else if busy {
                "Loading…".to_string()
            } else if self.error.is_some() {
                String::new()
            } else {
                pager.label()
            })
            .child(
                nav(
                    "page-first",
                    Lucide::ChevronsLeft,
                    "First Page",
                    pager.has_prev(),
                )
                .on_click(cx.listener(|this, _, window, cx| this.go(0, window, cx))),
            )
            .child(
                nav(
                    "page-prev",
                    Lucide::ChevronLeft,
                    "Previous Page",
                    pager.has_prev(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let p = this.pager.page.saturating_sub(1);
                    this.go(p, window, cx)
                })),
            )
            .child(div().min_w(px(64.)).text_center().child(page_label))
            .child(
                nav(
                    "page-next",
                    Lucide::ChevronRight,
                    "Next Page",
                    pager.has_next(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let p = this.pager.page + 1;
                    this.go(p, window, cx)
                })),
            )
            .child(
                nav(
                    "page-last",
                    Lucide::ChevronsRight,
                    "Last Page",
                    pager.last_page().is_some_and(|l| pager.page < l),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(last) = this.pager.last_page() {
                        this.go(last, window, cx)
                    }
                })),
            )
            .child(
                Button::new("page-reload")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Reload")
                    .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
            );
        let narrowed = self.query.is_filtered();
        let customized = narrowed || self.query.sort.is_some();
        let reset = |id: &'static str, cx: &mut Context<Self>| {
            Button::new(id)
                .small()
                .icon(Lucide::FunnelX)
                .label("Reset Filters and Sort")
                .on_click(cx.listener(|this, _, window, cx| this.reset_query(window, cx)))
        };
        let body = if let Some(e) = self.error.clone() {
            Placeholder::error("Unable to load rows")
                .detail(if customized {
                    format!("{e}\nCheck the filter values and sort column, or reset them.")
                } else {
                    e
                })
                .when(customized, |p| p.action(reset("table-error-reset", cx).primary()))
                .action(
                    Button::new("table-retry")
                        .small()
                        .icon(Lucide::RefreshCw)
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                )
                .into_any_element()
        } else if !busy && pager.page_rows == 0 && self.grid.is_some() {
            if narrowed {
                Placeholder::new(IconName::Search, "No matching rows")
                    .detail("No row matches the search and filters.")
                    .action(reset("table-empty-reset", cx))
                    .into_any_element()
            } else if pager.page == 0 {
                Placeholder::new(Lucide::Table, "This table is empty")
                    .detail("Rows inserted into it will show up here when you reload.")
                    .into_any_element()
            } else {
                Placeholder::new(Lucide::Table, "No rows on this page")
                    .detail("Rows may have been deleted since the last page loaded.")
                    .into_any_element()
            }
        } else if let Some(grid) = &self.grid {
            DataTable::new(grid).stripe(true).small().into_any_element()
        } else {
            Placeholder::loading("Loading rows…").into_any_element()
        };
        let filter_bar = self.render_filter_bar(cx);
        let filter_panel = self.show_filters.then(|| self.render_filters(cx));
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(toolbar)
            .child(filter_bar)
            .children(filter_panel)
            .child(div().flex_1().min_h_0().child(body))
    }
}

impl TableView {
    fn render_filter_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.query.filters.len();
        let muted = cx.theme().muted_foreground;
        h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div().w(px(260.)).child(
                    Input::new(&self.search)
                        .small()
                        .prefix(Icon::new(IconName::Search).xsmall())
                        .cleanable(true),
                ),
            )
            .child(
                Button::new("table-filters")
                    .small()
                    .ghost()
                    .selected(self.show_filters)
                    .icon(Lucide::Funnel)
                    .label(match active {
                        0 => "Filter".to_string(),
                        n => format!("Filter · {n}"),
                    })
                    .tooltip(if self.show_filters { "Hide Filters" } else { "Show Filters" })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_filters = !this.show_filters;
                        if this.show_filters && this.filters.is_empty() {
                            this.add_filter(window, cx);
                        }
                        cx.notify();
                    })),
            )
            .when_some(self.query.sort.clone(), |el, (column, dir)| {
                el.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(muted)
                        .child(Icon::new(Lucide::ArrowUpDown).xsmall())
                        .child(format!(
                            "Sorted by {column}, {}",
                            match dir {
                                SortDir::Asc => "ascending",
                                SortDir::Desc => "descending",
                            }
                        )),
                )
            })
            .child(div().flex_1())
            .when(self.query.is_filtered() || self.query.sort.is_some(), |el| {
                el.child(
                    Button::new("table-reset")
                        .small()
                        .ghost()
                        .icon(Lucide::FunnelX)
                        .label("Reset")
                        .tooltip("Show Every Row in Key Order")
                        .on_click(cx.listener(|this, _, window, cx| this.reset_query(window, cx))),
                )
            })
            .into_any_element()
    }

    fn render_filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let rows = self.filters.iter().enumerate().map(|(ix, f)| {
            let needs_value = f.op(cx).needs_value();
            h_flex()
                .gap_1p5()
                .child(
                    div()
                        .w(px(44.))
                        .text_xs()
                        .text_color(muted)
                        .child(if ix == 0 { "where" } else { "and" }),
                )
                .child(div().w(px(200.)).child(Select::new(&f.column).small().placeholder("Column")))
                .child(div().w(px(150.)).child(Select::new(&f.op).small()))
                .child(
                    div().flex_1().child(
                        Input::new(&f.value)
                            .small()
                            .disabled(!needs_value),
                    ),
                )
                .child(
                    Button::new(("table-filter-remove", ix))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::X)
                        .tooltip("Remove Condition")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.remove_filter(ix, window, cx)
                        })),
                )
        });
        let pending = self
            .filters
            .iter()
            .filter_map(|f| f.filter(cx))
            .collect::<Vec<_>>()
            != self.query.filters;
        v_flex()
            .px_3()
            .py_2()
            .gap_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .children(rows)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("table-filter-add")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .label("Add Condition")
                            .on_click(cx.listener(|this, _, window, cx| this.add_filter(window, cx))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Values compare as the column's type. Press Enter to apply."),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("table-filter-apply")
                            .small()
                            .primary()
                            .label("Apply")
                            .disabled(!pending)
                            .on_click(cx.listener(|this, _, window, cx| this.apply_filters(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

/// Grid over an in-memory result set (SQL editor results).
pub struct ResultDelegate {
    columns: Vec<Column>,
    set: ResultSet,
    /// Row number of the first row, when a leading "#" column is shown.
    first_row: Option<u64>,
    /// Header clicks sort through this view (table browser only).
    sorts: Option<WeakEntity<TableView>>,
}

impl ResultDelegate {
    pub fn new(set: ResultSet) -> Self {
        Self {
            columns: set.columns.iter().map(|c| column_for(c, "")).collect(),
            set,
            first_row: None,
            sorts: None,
        }
    }

    /// With a leading row-number column starting after `offset`.
    pub fn numbered(set: ResultSet, offset: u64) -> Self {
        let mut columns = vec![Column::new("#", "#").width(px(64.))];
        columns.extend(set.columns.iter().map(|c| column_for(c, "")));
        Self {
            columns,
            set,
            first_row: Some(offset),
            sorts: None,
        }
    }

    /// Headers sort on the server through `view`; `sort` is the current one.
    pub fn sortable(mut self, sort: Option<(String, SortDir)>, view: WeakEntity<TableView>) -> Self {
        let skip = usize::from(self.first_row.is_some());
        for (ix, column) in self.columns.iter_mut().enumerate().skip(skip) {
            let name = &self.set.columns[ix - skip];
            let state = match &sort {
                Some((c, SortDir::Asc)) if c == name => ColumnSort::Ascending,
                Some((c, SortDir::Desc)) if c == name => ColumnSort::Descending,
                _ => ColumnSort::Default,
            };
            *column = column.clone().sort(state);
        }
        self.sorts = Some(view);
        self
    }
}

impl TableDelegate for ResultDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.set.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let skip = usize::from(self.first_row.is_some());
        let (Some(view), Some(name)) = (
            self.sorts.clone(),
            col_ix.checked_sub(skip).and_then(|i| self.set.columns.get(i)).cloned(),
        ) else {
            return;
        };
        // The view replaces this grid once the sorted page arrives.
        cx.defer_in(window, move |_, window, cx| {
            let _ = view.update(cx, |view, cx| view.set_sort(name, sort, window, cx));
        });
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let col_ix = match self.first_row {
            Some(offset) if col_ix == 0 => {
                return div()
                    .text_color(cx.theme().muted_foreground)
                    .child((offset + row_ix as u64 + 1).to_string());
            }
            Some(_) => col_ix - 1,
            None => col_ix,
        };
        match self.set.rows.get(row_ix).and_then(|r| r.get(col_ix)) {
            Some(Some(v)) => div().truncate().child(cell_preview(v)),
            _ => div()
                .text_color(cx.theme().muted_foreground)
                .italic()
                .child("NULL"),
        }
    }
}

/// Most characters a grid cell lays out; it is truncated to its width anyway.
const CELL_PREVIEW_CHARS: usize = 256;

/// What a grid cell shows: one line, at most `CELL_PREVIEW_CHARS` long, so a
/// large json, text or bytea value costs no more per frame than a short one.
fn cell_preview(value: &str) -> String {
    let mut out: String = value
        .chars()
        .take(CELL_PREVIEW_CHARS)
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if out.len() < value.len() && value.chars().nth(CELL_PREVIEW_CHARS).is_some() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
    use super::{CELL_PREVIEW_CHARS, PAGE_SIZE, Pager, cell_preview};

    #[test]
    fn cell_preview_is_one_short_line() {
        assert_eq!(cell_preview("a\nb"), "a b");
        let long = "x".repeat(10_000);
        let shown = cell_preview(&long);
        assert_eq!(shown.chars().count(), CELL_PREVIEW_CHARS + 1);
        assert!(shown.ends_with('…'));
        assert_eq!(cell_preview(&"y".repeat(CELL_PREVIEW_CHARS)).chars().count(), CELL_PREVIEW_CHARS);
    }
    use sawit_core::db::RowCount;

    fn exact(rows: u64) -> Option<RowCount> {
        Some(RowCount { rows, exact: true })
    }

    #[test]
    fn page_size_is_fifty() {
        assert_eq!(PAGE_SIZE, 50);
    }

    #[test]
    fn exact_totals_bound_navigation() {
        let mut p = Pager {
            total: exact(123),
            page_rows: 50,
            ..Default::default()
        };
        assert_eq!(p.offset(), 0);
        assert!(!p.has_prev());
        assert!(p.has_next());
        assert_eq!(p.last_page(), Some(2));
        assert_eq!(p.label(), "1–50 of 123");
        p.page = 2;
        p.page_rows = 23;
        assert_eq!(p.offset(), 100);
        assert!(!p.has_next());
        assert!(p.has_prev());
        assert_eq!(p.label(), "101–123 of 123");
    }

    #[test]
    fn empty_and_unknown_totals() {
        let p = Pager {
            total: exact(0),
            ..Default::default()
        };
        assert_eq!(p.last_page(), Some(0));
        assert!(!p.has_next());
        assert_eq!(p.label(), "0 rows");
        // Count still running or only estimated: a full page means "maybe more".
        let mut p = Pager {
            page_rows: 50,
            ..Default::default()
        };
        assert!(p.has_next());
        assert_eq!(p.last_page(), None);
        assert_eq!(p.label(), "1–50");
        p.total = Some(RowCount {
            rows: 2_000_000,
            exact: false,
        });
        assert_eq!(p.label(), "1–50 of ±2000000");
        p.page_rows = 7;
        assert!(!p.has_next());
    }
}
