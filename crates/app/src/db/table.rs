//! Read-only table/view browser: pages of 50 rows ordered by the primary
//! key, fetched over a read-only connection. The key and row estimate come
//! from the schema tree, the count rides behind the first page on the same
//! connection, and the next page is fetched ahead, so a remote database
//! costs about one round trip per page shown.
//!
//! Sorting (a column header), filters and the row search run on the server,
//! so they cover every row, not only the page shown.
//!
//! Rows export to CSV or Excel: the page shown, or every row the query
//! matches (streamed over a connection of its own). On a table with a primary
//! key, rows can be ticked (on this page, across pages, or all that match)
//! for a bulk update or delete over the read-write connection.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{
    self, BulkChange, ExportFormat, ExportSummary, FilterOp, RelKind, ResultSet, RowCount,
    RowFilter, RowQuery, RowTarget, SortDir,
};
use tokio_postgres::Client;

use super::DbEvent;
use super::store::DbStore;
use crate::rt;
use crate::states::{Banner, Placeholder};

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

/// Rows ticked for a bulk change, by primary-key values (kept across pages).
#[derive(Debug, Default, Clone)]
struct Selection {
    rows: BTreeSet<Vec<String>>,
    /// Every row the current query matches, ticked or not.
    all: bool,
}

impl Selection {
    fn is_empty(&self) -> bool {
        !self.all && self.rows.is_empty()
    }
}

/// An export of every matching row, writing in the background.
struct Exporting {
    format: ExportFormat,
    rows: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

/// The "set a column to a value" form of a bulk update.
struct UpdateForm {
    column: Entity<ColumnSelect>,
    value: Entity<InputState>,
    null: bool,
    _subscription: Subscription,
}

/// How often the export's row count refreshes.
const PROGRESS_TICK: Duration = Duration::from_millis(250);

fn plural(n: u64, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
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
    selection: Selection,
    /// Key values of the rows shown, so ticks are read without the grid
    /// (whose cells ask this view while it draws them).
    shown_keys: Vec<Vec<String>>,
    updating: Option<UpdateForm>,
    /// A bulk update or delete is running.
    changing: bool,
    exporting: Option<Exporting>,
    /// Outcome of the last export or bulk change; `true` for a failure.
    notice: Option<(bool, String)>,
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
        let on_search = cx.subscribe_in(
            &search,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
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
            },
        );
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
            selection: Selection::default(),
            shown_keys: Vec::new(),
            updating: None,
            changing: false,
            exporting: None,
            notice: None,
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
                    Err(_) => {
                        this.error = Some("The query stopped unexpectedly. Try again.".into())
                    }
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
        let key = self.key(cx);
        let view = cx.entity().downgrade();
        let mut delegate = ResultDelegate::numbered(set, first_row)
            .sortable(self.query.sort.clone(), view.clone());
        if let Some(key) = key {
            delegate = delegate.pickable(&key, view);
        }
        self.shown_keys = delegate.page_keys();
        self.grid = Some(cx.new(|cx| TableState::new(delegate, window, cx)));
    }

    /// Loads the first page again with the current query.
    fn requery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // "All matching" and ticks made under another filter would mislead.
        self.selection = Selection::default();
        self.pager.page = 0;
        self.reload(window, cx);
    }

    /// From a column header: descending, then ascending, then the key order.
    fn set_sort(
        &mut self,
        column: String,
        sort: ColumnSort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let op = cx
            .new(|cx| OpSelect::new(SearchableVec::new(ops), Some(IndexPath::new(0)), window, cx));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let subscriptions = vec![
            cx.subscribe(
                &op,
                |_, _, _: &SelectEvent<SearchableVec<&'static str>>, cx| cx.notify(),
            ),
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

/// Selection and bulk changes.
impl TableView {
    /// The primary key rows are ticked by; `None` for views and keyless
    /// tables, which can't be changed row by row.
    fn key(&self, cx: &App) -> Option<Vec<String>> {
        let relation = self
            .db
            .read(cx)
            .relation(&self.profile, &self.schema, &self.table);
        if relation.is_some_and(|r| r.kind != RelKind::Table) {
            return None;
        }
        self.order_by.clone().filter(|k| !k.is_empty())
    }

    fn page_keys(&self) -> Vec<Vec<String>> {
        self.shown_keys.clone()
    }

    fn is_picked(&self, key: &[String]) -> bool {
        self.selection.all || self.selection.rows.contains(key)
    }

    /// Every row on this page is ticked.
    fn page_picked(&self) -> bool {
        let keys = self.page_keys();
        !keys.is_empty() && keys.iter().all(|k| self.is_picked(k))
    }

    /// The grid draws ticks from this view, so it redraws with it.
    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(grid) = &self.grid {
            grid.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    fn pick_row(&mut self, key: Vec<String>, on: bool, cx: &mut Context<Self>) {
        if self.selection.all && !on {
            // Unticking one of "all": keep the rest of this page ticked.
            self.selection.all = false;
            self.selection.rows = self.page_keys().into_iter().collect();
        }
        if on {
            self.selection.rows.insert(key);
        } else {
            self.selection.rows.remove(&key);
        }
        self.selection_changed(cx);
    }

    fn pick_page(&mut self, on: bool, cx: &mut Context<Self>) {
        let keys = self.page_keys();
        if on {
            self.selection.rows.extend(keys);
        } else {
            self.selection.all = false;
            for k in &keys {
                self.selection.rows.remove(k);
            }
        }
        self.selection_changed(cx);
    }

    fn pick_all(&mut self, cx: &mut Context<Self>) {
        self.selection = Selection {
            rows: BTreeSet::new(),
            all: true,
        };
        self.selection_changed(cx);
    }

    fn clear_picks(&mut self, cx: &mut Context<Self>) {
        self.selection = Selection::default();
        self.updating = None;
        self.selection_changed(cx);
    }

    /// "3 rows", "all 1200 rows", "all matching rows".
    fn picked_label(&self) -> String {
        if !self.selection.all {
            return plural(self.selection.rows.len() as u64, "row");
        }
        let scope = if self.query.is_filtered() {
            " matching"
        } else {
            ""
        };
        match self.pager.total {
            Some(RowCount { rows, exact: true }) => format!("all {rows}{scope} rows"),
            _ => format!("all{scope} rows"),
        }
    }

    fn target(&self) -> Option<RowTarget> {
        if self.selection.all {
            return Some(RowTarget::Matching(self.query.clone()));
        }
        Some(RowTarget::Keys {
            key: self.key_cached()?,
            rows: self.selection.rows.iter().cloned().collect(),
        })
    }

    fn key_cached(&self) -> Option<Vec<String>> {
        self.order_by.clone().filter(|k| !k.is_empty())
    }

    fn open_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key_cached().unwrap_or_default();
        // Key columns are left out: changing them is rarely meant in bulk.
        let columns: Vec<String> = self
            .column_names(cx)
            .into_iter()
            .filter(|c| !key.contains(c))
            .collect();
        let first = (!columns.is_empty()).then(|| IndexPath::new(0));
        let column = cx.new(|cx| ColumnSelect::new(SearchableVec::new(columns), first, window, cx));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("New value"));
        let subscription =
            cx.subscribe_in(&value, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_update(window, cx);
                }
            });
        value.update(cx, |s, cx| s.focus(window, cx));
        self.updating = Some(UpdateForm {
            column,
            value,
            null: false,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn submit_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.updating else { return };
        let Some(column) = form.column.read(cx).selected_value().cloned() else {
            return;
        };
        let value = (!form.null).then(|| form.value.read(cx).value().to_string());
        self.confirm_change(BulkChange::Update { column, value }, window, cx);
    }

    /// Asks first, then runs `change` over the ticked rows.
    fn confirm_change(&mut self, change: BulkChange, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.target() else { return };
        let relation = format!("{}.{}", self.schema, self.table);
        let whole_table = self.selection.all && !self.query.is_filtered();
        let rows = self.picked_label();
        let (message, detail, verb) = match &change {
            BulkChange::Delete if whole_table => (
                format!("Delete every row in {relation}?"),
                format!("This deletes {rows}, not only the ones on this page. It can't be undone."),
                "Delete All Rows",
            ),
            BulkChange::Delete => (
                format!("Delete {rows} from {relation}?"),
                "This can't be undone.".to_string(),
                "Delete Rows",
            ),
            BulkChange::Update { column, value } => (
                format!(
                    "Set {column} to {} in {rows}?",
                    value
                        .as_deref()
                        .map(|v| format!("“{v}”"))
                        .unwrap_or_else(|| "NULL".into())
                ),
                if whole_table {
                    "Every row in the table changes. This can't be undone.".to_string()
                } else {
                    "This can't be undone.".to_string()
                },
                "Update Rows",
            ),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&detail),
            &[verb, "Cancel"],
            cx,
        );
        let sql = db::bulk_sql(&self.schema, &self.table, &target, &change);
        let deleting = matches!(change, BulkChange::Delete);
        cx.spawn_in(window, async move |this, window| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update_in(window, |this, window, cx| {
                this.run_change(sql, deleting, window, cx)
            });
        })
        .detach();
    }

    fn run_change(
        &mut self,
        sql: String,
        deleting: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rx) = super::actions::execute(&self.db, &self.profile, sql, cx) else {
            self.notice = Some((true, "Not connected. Reconnect and try again.".into()));
            cx.notify();
            return;
        };
        self.changing = true;
        self.notice = None;
        cx.notify();
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                this.changing = false;
                this.notice = Some(match result {
                    Ok(Ok(n)) => {
                        this.selection = Selection::default();
                        this.updating = None;
                        this.reload(window, cx);
                        let verb = if deleting { "Deleted" } else { "Updated" };
                        (false, format!("{verb} {}.", plural(n, "row")))
                    }
                    Ok(Err(e)) => (true, format!("Nothing was changed. {e}")),
                    Err(_) => (
                        true,
                        "The change stopped unexpectedly. Reload to see what happened.".into(),
                    ),
                });
                cx.notify();
            });
        })
        .detach();
    }
}

/// Exports.
impl TableView {
    /// Asks where to save, then writes this page or every matching row.
    fn export(
        &mut self,
        format: ExportFormat,
        all: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = format!("{}.{}", self.table, format.extension());
        let picked = cx.prompt_for_new_path(&super::actions::save_dir(), Some(&name));
        cx.spawn_in(window, async move |this, window| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let _ = this.update_in(window, |this, window, cx| {
                if all {
                    this.export_all(format, path, window, cx);
                } else {
                    this.export_page(format, path, window, cx);
                }
            });
        })
        .detach();
    }

    fn column_types(&self, cx: &App) -> Vec<db::ColumnInfo> {
        self.db
            .read(cx)
            .relation(&self.profile, &self.schema, &self.table)
            .map(|r| r.columns.clone())
            .unwrap_or_default()
    }

    fn export_page(
        &mut self,
        format: ExportFormat,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(set) = self
            .grid
            .as_ref()
            .map(|g| g.read(cx).delegate().set.clone())
        else {
            return;
        };
        let (types, sheet) = (self.column_types(cx), self.table.clone());
        let target = path.clone();
        let rx = rt::spawn_blocking(cx, move || {
            db::export_result_set(&set, format, &sheet, &types, &target)
        });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, _, cx| this.exported(&path, result, cx));
        })
        .detach();
    }

    /// Streams over a connection of its own, so browsing stays responsive.
    fn export_all(
        &mut self,
        format: ExportFormat,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let Some(rx) = super::actions::export_relation(
            &self.db,
            &self.profile,
            self.schema.clone(),
            self.table.clone(),
            self.order_by.clone().unwrap_or_default(),
            self.query.clone(),
            format,
            path.clone(),
            rows.clone(),
            stop.clone(),
            cx,
        ) else {
            return;
        };
        self.exporting = Some(Exporting { format, rows, stop });
        self.notice = None;
        cx.notify();
        cx.spawn_in(window, async move |this, window| {
            let mut rx = rx;
            // Redraws the row count until the export ends.
            let result = loop {
                let tick = window.background_executor().timer(PROGRESS_TICK);
                match futures::future::select(rx, Box::pin(tick)).await {
                    futures::future::Either::Left((result, _)) => break result,
                    futures::future::Either::Right((_, pending)) => {
                        rx = pending;
                        if this.update(window, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                }
            };
            let _ = this.update_in(window, |this, _, cx| {
                this.exporting = None;
                this.exported(&path, result, cx);
            });
        })
        .detach();
    }

    fn exported(
        &mut self,
        path: &std::path::Path,
        result: Result<sawit_core::Result<ExportSummary>, oneshot::Canceled>,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.notice = Some(match result {
            Ok(Ok(ExportSummary {
                rows,
                truncated: false,
            })) => (false, format!("Saved {} to {name}.", plural(rows, "row"))),
            Ok(Ok(ExportSummary {
                rows,
                truncated: true,
            })) => (
                false,
                format!(
                    "Saved the first {rows} rows to {name}: Excel holds no more. Export as CSV for every row."
                ),
            ),
            Ok(Err(e)) => (true, format!("Unable to export {name}. {e}")),
            Err(_) => (true, "The export stopped unexpectedly. Try again.".into()),
        });
        cx.notify();
    }

    fn export_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity().downgrade();
        let page_rows = self.pager.page_rows as u64;
        let filtered = self.query.is_filtered();
        let all_label = match self.pager.total {
            Some(RowCount { rows, exact: true }) if filtered => format!("All {rows} Matching Rows"),
            Some(RowCount { rows, exact: true }) => format!("All {rows} Rows"),
            _ if filtered => "All Matching Rows".to_string(),
            _ => "All Rows".to_string(),
        };
        let has_rows = page_rows > 0;
        Button::new("table-export")
            .ghost()
            .xsmall()
            .icon(Lucide::Download)
            .label("Export")
            .tooltip("Save Rows as CSV or Excel")
            .disabled(!has_rows || self.exporting.is_some())
            .dropdown_menu(move |mut menu, _, _| {
                let item = |label: String, icon: Lucide, format: ExportFormat, all: bool| {
                    let view = view.clone();
                    PopupMenuItem::new(label)
                        .icon(icon)
                        .on_click(move |_, window, cx| {
                            let _ =
                                view.update(cx, |this, cx| this.export(format, all, window, cx));
                        })
                };
                menu = menu.label(format!("This Page ({})", plural(page_rows, "row")));
                for format in [ExportFormat::Csv, ExportFormat::Xlsx] {
                    menu = menu.item(item(
                        format!("{}…", format.label()),
                        icon_for(format),
                        format,
                        false,
                    ));
                }
                menu = menu.separator().label(all_label.clone());
                for format in [ExportFormat::Csv, ExportFormat::Xlsx] {
                    menu = menu.item(item(
                        format!("{}…", format.label()),
                        icon_for(format),
                        format,
                        true,
                    ));
                }
                menu
            })
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn icon_for(format: ExportFormat) -> Lucide {
    match format {
        ExportFormat::Csv => Lucide::FileText,
        ExportFormat::Xlsx => Lucide::FileSpreadsheet,
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
            .when(self.key(cx).is_none(), |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .px_1p5()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().secondary)
                        .child(Icon::new(Lucide::Lock).xsmall())
                        .child("read-only"),
                )
            })
            .child(div().flex_1())
            .map(|el| match &self.exporting {
                Some(e) => el
                    .child(format!(
                        "Exporting {}… {}",
                        e.format.label(),
                        plural(e.rows.load(Ordering::Relaxed), "row")
                    ))
                    .child(
                        Button::new("table-export-cancel")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .tooltip("Stop the Export")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(e) = &this.exporting {
                                    e.stop.store(true, Ordering::Relaxed);
                                }
                                cx.notify();
                            })),
                    ),
                None => el.child(self.export_menu(cx)),
            })
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
                .when(customized, |p| {
                    p.action(reset("table-error-reset", cx).primary())
                })
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
        let picks_bar = (!self.selection.is_empty()).then(|| self.render_picks_bar(cx));
        let update_form = self.updating.is_some().then(|| self.render_update_form(cx));
        let notice = self.notice.clone().map(|(failed, message)| {
            let banner = if failed {
                Banner::error("table-notice", message)
            } else {
                Banner::info("table-notice", message)
            };
            banner.on_dismiss(cx.listener(|this, _, _, cx| {
                this.notice = None;
                cx.notify();
            }))
        });
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(toolbar)
            .child(filter_bar)
            .children(filter_panel)
            .children(picks_bar)
            .children(update_form)
            .children(notice)
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
                    .tooltip(if self.show_filters {
                        "Hide Filters"
                    } else {
                        "Show Filters"
                    })
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
            .when(
                self.query.is_filtered() || self.query.sort.is_some(),
                |el| {
                    el.child(
                        Button::new("table-reset")
                            .small()
                            .ghost()
                            .icon(Lucide::FunnelX)
                            .label("Reset")
                            .tooltip("Show Every Row in Key Order")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.reset_query(window, cx)),
                            ),
                    )
                },
            )
            .into_any_element()
    }

    /// What is ticked, widening to every matching row, and what to do with it.
    fn render_picks_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.changing || self.loading;
        // Gmail-style: a whole page ticked offers every row beyond it.
        let offer_all = !self.selection.all
            && self.page_picked()
            && (self.pager.has_next() || self.pager.has_prev());
        let all_label = match self.pager.total {
            Some(RowCount { rows, exact: true }) => format!("Select All {rows}"),
            _ => "Select All".to_string(),
        };
        h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .text_xs()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().accent)
            .child(
                div()
                    .font_semibold()
                    .child(format!("{} selected", capitalize(&self.picked_label()))),
            )
            .when(offer_all, |el| {
                el.child(
                    Button::new("table-pick-all")
                        .link()
                        .xsmall()
                        .label(format!(
                            "{all_label}{}",
                            if self.query.is_filtered() {
                                " Matching Rows"
                            } else {
                                " Rows"
                            }
                        ))
                        .on_click(cx.listener(|this, _, _, cx| this.pick_all(cx))),
                )
            })
            .child(
                Button::new("table-pick-clear")
                    .ghost()
                    .xsmall()
                    .label("Clear Selection")
                    .on_click(cx.listener(|this, _, _, cx| this.clear_picks(cx))),
            )
            .child(div().flex_1())
            .when(self.changing, |el| el.child("Applying…"))
            .child(
                Button::new("table-bulk-update")
                    .small()
                    .icon(Lucide::Pencil)
                    .label("Update…")
                    .tooltip("Set a Column in the Selected Rows")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| this.open_update(window, cx))),
            )
            .child(
                Button::new("table-bulk-delete")
                    .small()
                    .danger()
                    .icon(Lucide::Trash)
                    .label("Delete…")
                    .tooltip("Delete the Selected Rows")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_change(BulkChange::Delete, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn render_update_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = &self.updating else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        h_flex()
            .px_3()
            .py_2()
            .gap_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(div().text_xs().text_color(muted).child("Set"))
            .child(
                div()
                    .w(px(200.))
                    .child(Select::new(&form.column).small().placeholder("Column")),
            )
            .child(div().text_xs().text_color(muted).child("to"))
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&form.value).small().disabled(form.null)),
            )
            .child(
                Checkbox::new("table-update-null")
                    .label("NULL")
                    .checked(form.null)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        if let Some(form) = &mut this.updating {
                            form.null = *checked;
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new("table-update-cancel")
                    .ghost()
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.updating = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("table-update-apply")
                    .small()
                    .primary()
                    .label("Update Rows…")
                    .disabled(self.changing || form.column.read(cx).selected_value().is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.submit_update(window, cx))),
            )
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
                .child(
                    div()
                        .w(px(200.))
                        .child(Select::new(&f.column).small().placeholder("Column")),
                )
                .child(div().w(px(150.)).child(Select::new(&f.op).small()))
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&f.value).small().disabled(!needs_value)),
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
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_filter(window, cx)),
                            ),
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
                            .on_click(
                                cx.listener(|this, _, window, cx| this.apply_filters(window, cx)),
                            ),
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
    /// A leading checkbox column ticking rows in this view, by the positions
    /// of the key columns in `set` (table browser on a keyed table).
    picks: Option<(WeakEntity<TableView>, Vec<usize>)>,
}

impl ResultDelegate {
    pub fn new(set: ResultSet) -> Self {
        Self {
            columns: set.columns.iter().map(|c| column_for(c, "")).collect(),
            set,
            first_row: None,
            sorts: None,
            picks: None,
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
            picks: None,
        }
    }

    /// Leading columns before the data: the checkbox and the row number.
    fn lead(&self) -> usize {
        usize::from(self.picks.is_some()) + usize::from(self.first_row.is_some())
    }

    /// Adds the checkbox column, unless the page lacks a `key` column.
    fn pickable(mut self, key: &[String], view: WeakEntity<TableView>) -> Self {
        let positions: Option<Vec<usize>> = key
            .iter()
            .map(|k| self.set.columns.iter().position(|c| c == k))
            .collect();
        if let Some(positions) = positions {
            self.columns.insert(
                0,
                Column::new("pick", "")
                    .width(px(36.))
                    .resizable(false)
                    .movable(false),
            );
            self.picks = Some((view, positions));
        }
        self
    }

    /// Key values of a row; `None` if one is NULL (never, for a primary key).
    fn row_key(&self, row_ix: usize) -> Option<Vec<String>> {
        let (_, positions) = self.picks.as_ref()?;
        let row = self.set.rows.get(row_ix)?;
        positions
            .iter()
            .map(|&i| row.get(i).cloned().flatten())
            .collect()
    }

    fn page_keys(&self) -> Vec<Vec<String>> {
        (0..self.set.rows.len())
            .filter_map(|ix| self.row_key(ix))
            .collect()
    }

    /// Headers sort on the server through `view`; `sort` is the current one.
    pub fn sortable(
        mut self,
        sort: Option<(String, SortDir)>,
        view: WeakEntity<TableView>,
    ) -> Self {
        let skip = self.lead();
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
        let skip = self.lead();
        let (Some(view), Some(name)) = (
            self.sorts.clone(),
            col_ix
                .checked_sub(skip)
                .and_then(|i| self.set.columns.get(i))
                .cloned(),
        ) else {
            return;
        };
        // The view replaces this grid once the sorted page arrives.
        cx.defer_in(window, move |_, window, cx| {
            let _ = view.update(cx, |view, cx| view.set_sort(name, sort, window, cx));
        });
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some((view, _)) = self.picks.clone().filter(|_| col_ix == 0) else {
            return div().size_full().child(self.columns[col_ix].name.clone());
        };
        let checked = view.upgrade().is_some_and(|v| v.read(cx).page_picked());
        div().size_full().child(
            Checkbox::new("pick-page")
                .checked(checked)
                .tooltip(if checked {
                    "Unselect This Page"
                } else {
                    "Select This Page"
                })
                .on_click(move |checked, _, cx| {
                    let _ = view.update(cx, |view, cx| view.pick_page(*checked, cx));
                }),
        )
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let col_ix = match &self.picks {
            Some((view, _)) if col_ix == 0 => {
                let Some(key) = self.row_key(row_ix) else {
                    return div();
                };
                let checked = view.upgrade().is_some_and(|v| v.read(cx).is_picked(&key));
                let view = view.clone();
                return div().child(Checkbox::new(("pick", row_ix)).checked(checked).on_click(
                    move |checked, _, cx| {
                        let key = key.clone();
                        let _ = view.update(cx, |view, cx| view.pick_row(key, *checked, cx));
                    },
                ));
            }
            Some(_) => col_ix - 1,
            None => col_ix,
        };
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
        assert_eq!(
            cell_preview(&"y".repeat(CELL_PREVIEW_CHARS))
                .chars()
                .count(),
            CELL_PREVIEW_CHARS
        );
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
