//! Ticking rows (on this page, across pages, or all that match) and changing them in bulk: the
//! bar that says what is ticked, the update form, and the question before a change runs.

use std::collections::BTreeSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{self, BulkChange, DbKind, RelKind, RowCount, RowTarget};

use super::{ColumnSelect, TableView, plural};
use crate::db::actions;

/// Rows ticked for a bulk change, by primary-key values (kept across pages).
#[derive(Debug, Default, Clone)]
pub(super) struct Selection {
    rows: BTreeSet<Vec<String>>,
    /// Every row the current query matches, ticked or not.
    all: bool,
}

impl Selection {
    pub(super) fn is_empty(&self) -> bool {
        !self.all && self.rows.is_empty()
    }
}

/// The "set a column to a value" form of a bulk update.
pub(super) struct UpdateForm {
    column: Entity<ColumnSelect>,
    value: Entity<InputState>,
    null: bool,
    _subscription: Subscription,
}

/// Selection and bulk changes.
impl TableView {
    /// What kind of database the table is in.
    pub(super) fn kind(&self, cx: &App) -> DbKind {
        self.db
            .read(cx)
            .profile(&self.profile)
            .map_or(DbKind::Postgres, |p| p.kind)
    }

    /// The primary key rows are ticked by; `None` for views and keyless
    /// tables, which can't be changed row by row.
    pub(super) fn key(&self, cx: &App) -> Option<Vec<String>> {
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

    pub(super) fn is_picked(&self, key: &[String]) -> bool {
        self.selection.all || self.selection.rows.contains(key)
    }

    /// Every row on this page is ticked. The grid's header asks on every frame: the keys are
    /// read in place, not copied.
    pub(super) fn page_picked(&self) -> bool {
        let keys = &self.shown_keys;
        !keys.is_empty() && keys.iter().all(|k| self.is_picked(k))
    }

    /// The grid draws ticks from this view, so it redraws with it.
    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(grid) = &self.grid {
            grid.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    pub(super) fn pick_row(&mut self, key: Vec<String>, on: bool, cx: &mut Context<Self>) {
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

    pub(super) fn pick_page(&mut self, on: bool, cx: &mut Context<Self>) {
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
        let (level, detail) = match self.db.read(cx).route(&self.profile) {
            Some(route @ db::WriteRoute::Remote { .. }) => (
                PromptLevel::Critical,
                format!("{detail}\n\n{}", route.describe()),
            ),
            Some(route) => (
                PromptLevel::Warning,
                format!("{detail}\n\n{}", route.describe()),
            ),
            None => (PromptLevel::Warning, detail),
        };
        let answer = window.prompt(level, &message, Some(&detail), &[verb, "Cancel"], cx);
        let columns = self.column_names(cx);
        let sql = db::bulk_sql_for(
            self.kind(cx),
            &self.schema,
            &self.table,
            &columns,
            &target,
            &change,
        );
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
        let Some(rx) = actions::execute(&self.db, &self.profile, sql, cx) else {
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

impl TableView {
    /// What is ticked, widening to every matching row, and what to do with it.
    pub(super) fn render_picks_bar(&self, cx: &mut Context<Self>) -> AnyElement {
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

    pub(super) fn render_update_form(&self, cx: &mut Context<Self>) -> AnyElement {
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
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
