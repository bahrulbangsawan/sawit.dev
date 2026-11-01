//! The grid of rows: a result set's columns and cells, a row number column, the checkboxes that
//! tick rows, and headers that sort on the server.

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::db::{ResultSet, SortDir};

use super::TableView;

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

/// Grid over an in-memory result set (SQL editor results).
pub struct ResultDelegate {
    columns: Vec<Column>,
    pub(super) set: ResultSet,
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
    pub(super) fn pickable(mut self, key: &[String], view: WeakEntity<TableView>) -> Self {
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

    pub(super) fn page_keys(&self) -> Vec<Vec<String>> {
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
pub(super) const CELL_PREVIEW_CHARS: usize = 256;

/// What a grid cell shows: one line, at most `CELL_PREVIEW_CHARS` long, so a
/// large json, text or bytea value costs no more per frame than a short one.
pub(super) fn cell_preview(value: &str) -> String {
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
