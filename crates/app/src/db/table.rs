//! Read-only table/view browser: pages of 50 rows ordered by the primary
//! key, fetched over a read-only connection.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::component::*;
use gpui_kit::*;
use pgdesk_core::db::{self, ResultSet, RowCount};
use tokio_postgres::Client;

use crate::rt;

/// Rows per page (the most a page ever shows).
pub const PAGE_SIZE: u64 = 50;

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
            return "0 baris".into();
        }
        let range = format!(
            "{}–{}",
            self.offset() + 1,
            self.offset() + self.page_rows as u64
        );
        match self.total {
            Some(RowCount { rows, exact: true }) => format!("{range} dari {rows}"),
            Some(RowCount { rows, exact: false }) => format!("{range} dari ±{rows}"),
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
    client: Arc<Client>,
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
}

impl TableView {
    /// `client` should be a read-only connection.
    pub fn new(
        title: SharedString,
        client: Arc<Client>,
        schema: String,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            title,
            client,
            schema,
            table,
            order_by: None,
            pager: Pager::default(),
            grid: None,
            loading: false,
            error: None,
            generation: 0,
        };
        this.reload(window, cx);
        this
    }

    /// Re-counts and reloads the current page.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pager.total = None;
        self.order_by = None;
        self.load_page(self.pager.page, window, cx);
    }

    fn load_page(&mut self, page: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        cx.notify();
        let (client, schema, table) =
            (self.client.clone(), self.schema.clone(), self.table.clone());
        let known_order = self.order_by.clone();
        let count = self.pager.total.is_none();
        let rx = rt::spawn(cx, async move {
            let order = match known_order {
                Some(o) => o,
                None => db::primary_key(&client, &schema, &table).await?,
            };
            let set = db::fetch_page(
                &client,
                &schema,
                &table,
                &order,
                page * PAGE_SIZE,
                PAGE_SIZE,
            )
            .await?;
            Ok::<_, pgdesk_core::CoreError>((order, set))
        });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(Ok((order, set))) => {
                        this.order_by = Some(order);
                        this.pager.page = page;
                        this.show(set, window, cx);
                        if count {
                            this.count(cx);
                        }
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => this.error = Some("Runtime database berhenti.".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show(&mut self, set: ResultSet, window: &mut Window, cx: &mut Context<Self>) {
        self.pager.page_rows = set.rows.len();
        let first_row = self.pager.offset();
        self.grid = Some(
            cx.new(|cx| TableState::new(ResultDelegate::numbered(set, first_row), window, cx)),
        );
    }

    /// Counts rows after the first page is on screen (the count may be slow).
    fn count(&mut self, cx: &mut Context<Self>) {
        let (client, schema, table) =
            (self.client.clone(), self.schema.clone(), self.table.clone());
        let rx = rt::spawn(
            cx,
            async move { db::row_count(&client, &schema, &table).await },
        );
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(total)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.pager.total = Some(total);
                    cx.notify();
                });
            }
        })
        .detach();
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
            Some(last) => format!("Hal. {} / {}", pager.page + 1, last + 1),
            None => format!("Hal. {}", pager.page + 1),
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
            .child(if busy {
                "memuat…".to_string()
            } else {
                pager.label()
            })
            .child(
                nav(
                    "page-first",
                    Lucide::ChevronsLeft,
                    "Halaman pertama",
                    pager.has_prev(),
                )
                .on_click(cx.listener(|this, _, window, cx| this.go(0, window, cx))),
            )
            .child(
                nav(
                    "page-prev",
                    Lucide::ChevronLeft,
                    "Sebelumnya",
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
                    "Berikutnya",
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
                    "Halaman terakhir",
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
                    .tooltip("Muat ulang")
                    .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
            );
        let body = if let Some(e) = self.error.clone() {
            div()
                .p_3()
                .text_sm()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(cx.theme().danger)
                .whitespace_normal()
                .child(e)
                .into_any_element()
        } else if !busy && pager.page_rows == 0 && self.grid.is_some() {
            div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if pager.page == 0 {
                    "Tabel ini kosong."
                } else {
                    "Tidak ada baris di halaman ini."
                })
                .into_any_element()
        } else if let Some(grid) = &self.grid {
            DataTable::new(grid).stripe(true).small().into_any_element()
        } else {
            div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Memuat…")
                .into_any_element()
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
    }
}

/// Grid over an in-memory result set (SQL editor results).
pub struct ResultDelegate {
    columns: Vec<Column>,
    set: ResultSet,
    /// Row number of the first row, when a leading "#" column is shown.
    first_row: Option<u64>,
}

impl ResultDelegate {
    pub fn new(set: ResultSet) -> Self {
        Self {
            columns: set.columns.iter().map(|c| column_for(c, "")).collect(),
            set,
            first_row: None,
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
    use super::{PAGE_SIZE, Pager};
    use pgdesk_core::db::RowCount;

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
        assert_eq!(p.label(), "1–50 dari 123");
        p.page = 2;
        p.page_rows = 23;
        assert_eq!(p.offset(), 100);
        assert!(!p.has_next());
        assert!(p.has_prev());
        assert_eq!(p.label(), "101–123 dari 123");
    }

    #[test]
    fn empty_and_unknown_totals() {
        let p = Pager {
            total: exact(0),
            ..Default::default()
        };
        assert_eq!(p.last_page(), Some(0));
        assert!(!p.has_next());
        assert_eq!(p.label(), "0 baris");
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
        assert_eq!(p.label(), "1–50 dari ±2000000");
        p.page_rows = 7;
        assert!(!p.has_next());
    }
}
