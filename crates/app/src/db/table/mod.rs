//! Read-only table/view browser: pages of 50 rows (or the 100 to 1000 picked
//! in the toolbar's Show menu, kept for every table) ordered by the primary
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
//!
//! The same view browses SQLite files and Cloudflare D1 ([`Conn`]): there a
//! read is a short read-only open of the file, and a change takes the write
//! route the toolbar shows (the dev server's Local Explorer, the file, or
//! Wrangler for a remote (production) database).

mod export;
mod filters;
mod grid;
mod load;
mod pager;
mod render;
mod selection;

use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::table::TableState;
use gpui_kit::*;
use sawit_core::db::{ResultSet, RowQuery};

use super::DbEvent;
use super::store::DbStore;
use crate::catalog::{self, PageSize};
use export::Exporting;
use filters::FilterRow;
pub(super) use grid::ResultDelegate;
use pager::Pager;
use selection::{Selection, UpdateForm};

/// Rows per page until the user picks another size.
pub const PAGE_SIZE: u64 = 50;

/// The sizes the toolbar's Show menu offers; never every row, as a table can hold millions.
const PAGE_SIZES: [PageSize; 5] = [
    PageSize::Rows(50),
    PageSize::Rows(100),
    PageSize::Rows(250),
    PageSize::Rows(500),
    PageSize::Rows(1000),
];

/// The Show menu's list (`page-sizes.json`): one size for every table.
const PAGE_SIZE_LIST: &str = "table";

/// Typing pause before the row search runs.
const SEARCH_DELAY: Duration = Duration::from_millis(400);

type ColumnSelect = SelectState<SearchableVec<String>>;
type OpSelect = SelectState<SearchableVec<&'static str>>;

fn plural(n: u64, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
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
            pager: Pager {
                size: saved_page_size(cx),
                ..Pager::default()
            },
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
}

/// The size the user last picked for tables, among the Show menu's; else [`PAGE_SIZE`].
fn saved_page_size(cx: &App) -> u64 {
    match catalog::saved_size(
        PAGE_SIZE_LIST,
        &PAGE_SIZES,
        PageSize::Rows(PAGE_SIZE as usize),
        cx,
    ) {
        PageSize::Rows(n) => n as u64,
        PageSize::All => PAGE_SIZE,
    }
}

#[cfg(test)]
mod tests;
