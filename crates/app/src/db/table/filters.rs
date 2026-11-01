//! Narrowing and ordering the rows on the server: the row search, the filter panel's
//! conditions, and the sort a column header sets.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent};
use gpui_kit::component::table::ColumnSort;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{FilterOp, RowFilter, RowQuery, SortDir};

use super::{ColumnSelect, OpSelect, Selection, TableView};

/// One "column operator value" condition in the filter panel.
pub(super) struct FilterRow {
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

impl TableView {
    /// Loads the first page again with the current query.
    fn requery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // "All matching" and ticks made under another filter would mislead.
        self.selection = Selection::default();
        self.pager.page = 0;
        self.reload(window, cx);
    }

    /// From a column header: descending, then ascending, then the key order.
    pub(super) fn set_sort(
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

    pub(super) fn apply_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn column_names(&self, cx: &App) -> Vec<String> {
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
    pub(super) fn reset_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filters.clear();
        self.show_filters = false;
        // Matches the query below, so the search's Change doesn't reload again.
        self.query = RowQuery::default();
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.requery(window, cx);
    }

    pub(super) fn render_filter_bar(&self, cx: &mut Context<Self>) -> AnyElement {
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

    pub(super) fn render_filters(&self, cx: &mut Context<Self>) -> AnyElement {
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
