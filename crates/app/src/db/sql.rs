//! SQL editor tab: write SQL, Cmd-Enter to run the selection (or everything),
//! see results, errors and timing.

use std::sync::Arc;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Editor, EditorState, TabSize};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::db::{self, QueryOutcome};
use tokio_postgres::Client;

use super::table::ResultDelegate;
use crate::rt;

const MAX_ROWS: usize = 10_000;

actions!(sql_editor, [RunQuery]);

pub fn init(cx: &mut App) {
    // "SqlEditor > Input" matches as deep as the editor's own cmd-enter and is
    // registered later, so it wins over inserting a newline.
    cx.bind_keys([
        KeyBinding::new("cmd-enter", RunQuery, Some("SqlEditor > Input")),
        KeyBinding::new("cmd-enter", RunQuery, Some("SqlEditor")),
    ]);
}

pub struct SqlView {
    pub title: SharedString,
    client: Arc<Client>,
    editor: Entity<EditorState>,
    result: Option<Entity<TableState<ResultDelegate>>>,
    summary: Option<String>,
    error: Option<String>,
    running: bool,
}

impl SqlView {
    pub fn new(
        title: SharedString,
        client: Arc<Client>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("sql")
                .line_number(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                .placeholder("-- Tulis SQL lalu tekan ⌘↩ untuk menjalankan")
        });
        Self {
            title,
            client,
            editor,
            result: None,
            summary: None,
            error: None,
            running: false,
        }
    }

    fn run(&mut self, _: &RunQuery, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let state = self.editor.read(cx);
        let selected = state.selected_text().to_string();
        let sql = if selected.trim().is_empty() {
            state.value().to_string()
        } else {
            selected
        };
        if sql.trim().is_empty() {
            return;
        }
        self.running = true;
        self.error = None;
        cx.notify();
        let client = self.client.clone();
        let rx = rt::spawn(
            cx,
            async move { db::run_sql(&client, &sql, MAX_ROWS).await },
        );
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                this.running = false;
                match result {
                    Ok(Ok(outcome)) => this.show(outcome, window, cx),
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show(&mut self, outcome: QueryOutcome, window: &mut Window, cx: &mut Context<Self>) {
        let ms = outcome.duration.as_secs_f64() * 1000.;
        let mut parts = vec![format!("{ms:.1} ms")];
        if !outcome.affected.is_empty() {
            let total: u64 = outcome.affected.iter().sum();
            parts.push(format!("{total} baris terdampak"));
        }
        match outcome.results.into_iter().last() {
            Some(set) => {
                parts.push(format!(
                    "{} baris{}",
                    set.rows.len(),
                    if set.truncated { " (dipotong)" } else { "" }
                ));
                self.result =
                    Some(cx.new(|cx| TableState::new(ResultDelegate::new(set), window, cx)));
            }
            None => self.result = None,
        }
        self.summary = Some(parts.join(" · "));
    }
}

impl Render for SqlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("SqlEditor")
            .on_action(cx.listener(Self::run))
            .size_full()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .flex_1()
                            .child(self.title.clone()),
                    )
                    .child(
                        Button::new("run-sql")
                            .primary()
                            .xsmall()
                            .icon(IconName::Play)
                            .label("Jalankan ⌘↩")
                            .loading(self.running)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.run(&RunQuery, window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(220.))
                    .child(Editor::new(&self.editor).size_full().bordered(false)),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .border_y_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.running {
                        "menjalankan…".to_string()
                    } else {
                        self.summary
                            .clone()
                            .unwrap_or_else(|| "Belum ada hasil.".into())
                    }),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
            .when_some(self.result.clone(), |el, table| {
                el.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(DataTable::new(&table).stripe(true)),
                )
            })
    }
}
