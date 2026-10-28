//! Read-only data grid for one table: virtual rows, pages of 200 fetched
//! only for the visible range.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::db::{self, ColumnInfo, ResultSet};
use tokio_postgres::Client;

use crate::rt;

const PAGE: u64 = 200;
/// Pages kept in memory; older ones are dropped as you scroll.
const MAX_PAGES: usize = 40;

/// Pages (of `PAGE` rows) touched by a visible row range.
fn pages_for(visible: &Range<usize>) -> std::ops::RangeInclusive<u64> {
    let first = visible.start as u64 / PAGE;
    let last = visible.end.saturating_sub(1).max(visible.start) as u64 / PAGE;
    first..=last
}

/// Loaded pages, least recently viewed first.
struct PageLru {
    order: Vec<u64>,
    capacity: usize,
}

impl PageLru {
    fn new(capacity: usize) -> Self {
        Self {
            order: Vec::new(),
            capacity,
        }
    }

    fn touch(&mut self, page: u64) {
        if let Some(i) = self.order.iter().position(|p| *p == page) {
            let p = self.order.remove(i);
            self.order.push(p);
        }
    }

    /// Records a newly loaded page; returns the page to evict, if any.
    fn insert(&mut self, page: u64) -> Option<u64> {
        self.order.retain(|p| *p != page);
        self.order.push(page);
        (self.order.len() > self.capacity).then(|| self.order.remove(0))
    }
}

/// Wait this long after scrolling stops before fetching, so dragging the
/// scrollbar across 300k rows doesn't queue a query per page passed over.
const FETCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);

pub struct RowsDelegate {
    columns: Vec<Column>,
    total: usize,
    pages: HashMap<u64, Vec<Vec<Option<String>>>>,
    pending: HashSet<u64>,
    lru: PageLru,
    visible: Range<usize>,
    fetch_scheduled: bool,
    client: Arc<Client>,
    schema: String,
    table: String,
    pub error: Option<String>,
}

impl RowsDelegate {
    fn request(&mut self, page: u64, cx: &mut Context<TableState<Self>>) {
        if self.pages.contains_key(&page) || !self.pending.insert(page) {
            return;
        }
        let (client, schema, table) =
            (self.client.clone(), self.schema.clone(), self.table.clone());
        let rx = rt::spawn(cx, async move {
            db::fetch_page(&client, &schema, &table, page * PAGE, PAGE).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |state, cx| {
                let d = state.delegate_mut();
                d.pending.remove(&page);
                match result {
                    Ok(Ok(set)) => {
                        d.pages.insert(page, set.rows);
                        if let Some(old) = d.lru.insert(page) {
                            d.pages.remove(&old);
                        }
                    }
                    Ok(Err(e)) => d.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn cell(&self, row: usize, col: usize) -> Option<Option<&str>> {
        let page = self.pages.get(&(row as u64 / PAGE))?;
        let r = page.get(row % PAGE as usize)?;
        Some(r.get(col).and_then(|v| v.as_deref()))
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

impl TableDelegate for RowsDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.total
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        match self.cell(row_ix, col_ix) {
            Some(Some(v)) => div().truncate().child(v.to_string()),
            Some(None) => div()
                .text_color(cx.theme().muted_foreground)
                .italic()
                .child("NULL"),
            None => div().text_color(cx.theme().muted_foreground).child("…"),
        }
    }

    fn visible_rows_changed(
        &mut self,
        visible: Range<usize>,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if visible.is_empty() {
            return;
        }
        self.visible = visible;
        if self.fetch_scheduled {
            return;
        }
        self.fetch_scheduled = true;
        cx.spawn(async move |this, cx| {
            let mut last = None;
            // Fetch once the visible range has been stable for one debounce period.
            loop {
                cx.background_executor().timer(FETCH_DEBOUNCE).await;
                let Ok(range) = this.update(cx, |state, _| state.delegate().visible.clone()) else {
                    return;
                };
                if last.as_ref() == Some(&range) {
                    break;
                }
                last = Some(range);
            }
            let _ = this.update(cx, |state, cx| {
                let d = state.delegate_mut();
                d.fetch_scheduled = false;
                let pages = pages_for(&d.visible);
                for page in pages {
                    d.lru.touch(page);
                    d.request(page, cx);
                }
            });
        })
        .detach();
    }
}

pub struct TableView {
    state: Entity<TableState<RowsDelegate>>,
    pub title: SharedString,
    loading: bool,
}

impl TableView {
    pub fn new(
        client: Arc<Client>,
        schema: String,
        table: String,
        columns: Vec<ColumnInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title: SharedString = format!("{schema}.{table}").into();
        let delegate = RowsDelegate {
            columns: columns
                .iter()
                .map(|c| column_for(&c.name, &c.data_type))
                .collect(),
            total: 0,
            pages: HashMap::new(),
            pending: HashSet::new(),
            lru: PageLru::new(MAX_PAGES),
            visible: 0..0,
            fetch_scheduled: false,
            client: client.clone(),
            schema: schema.clone(),
            table: table.clone(),
            error: None,
        };
        let state = cx.new(|cx| TableState::new(delegate, window, cx));
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        let rx = rt::spawn(
            cx,
            async move { db::row_count(&client, &schema, &table).await },
        );
        let st = state.clone();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                st.update(cx, |state, cx| {
                    match result {
                        Ok(Ok(n)) => state.delegate_mut().total = n as usize,
                        Ok(Err(e)) => state.delegate_mut().error = Some(e.to_string()),
                        Err(_) => {}
                    }
                    state.refresh(cx);
                    // Load the first page right away.
                    state.delegate_mut().request(0, cx);
                });
                cx.notify();
            });
        })
        .detach();
        Self {
            state,
            title,
            loading: true,
        }
    }
}

impl Render for TableView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.state.read(cx).delegate();
        let summary = if self.loading {
            "menghitung baris…".to_string()
        } else {
            format!("{} baris · read-only", d.total)
        };
        let error = d.error.clone();
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
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
                    .child(summary),
            )
            .when_some(error, |el, e| {
                el.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(e),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.state).stripe(true)),
            )
    }
}

/// Grid over an in-memory result set (SQL editor results).
pub struct ResultDelegate {
    columns: Vec<Column>,
    set: ResultSet,
}

impl ResultDelegate {
    pub fn new(set: ResultSet) -> Self {
        Self {
            columns: set.columns.iter().map(|c| column_for(c, "")).collect(),
            set,
        }
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

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        match self.set.rows.get(row_ix).and_then(|r| r.get(col_ix)) {
            Some(Some(v)) => div().truncate().child(v.clone()),
            _ => div()
                .text_color(cx.theme().muted_foreground)
                .italic()
                .child("NULL"),
        }
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui_kit::*` exports a `test` macro that shadows #[test].
    use super::{PageLru, pages_for};

    #[test]
    fn pages_cover_visible_rows() {
        assert_eq!(pages_for(&(0..10)), 0..=0);
        assert_eq!(pages_for(&(195..410)), 0..=2);
    }

    #[test]
    fn lru_evicts_least_recently_viewed() {
        let mut lru = PageLru::new(2);
        assert_eq!(lru.insert(1), None);
        assert_eq!(lru.insert(2), None);
        lru.touch(1); // page 1 is on screen again
        assert_eq!(lru.insert(3), Some(2));
    }
}
