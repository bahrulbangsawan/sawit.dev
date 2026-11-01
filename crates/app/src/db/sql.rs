//! SQL editor tab: write SQL, Cmd-Enter to run the selection (or everything),
//! see results, errors and timing.
//!
//! On SQLite and Cloudflare D1, SQL that only reads runs read-only on the
//! file; SQL that writes is asked about first, saying where it goes (the dev
//! server's Local Explorer, the file, or a remote (production) database), and
//! the summary says where it went.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Editor, EditorState, TabSize};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::{Conn, DbKind, Lite, QueryOutcome, ReadOnlyTry, WriteRoute};

use super::store::DbStore;
use super::table::ResultDelegate;
use crate::rt;
use crate::states::Placeholder;

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
    db: Entity<DbStore>,
    profile: String,
    /// Cancels the statement in flight.
    cancel: Option<tokio_postgres::CancelToken>,
    editor: Entity<EditorState>,
    result: Option<Entity<TableState<ResultDelegate>>>,
    summary: Option<String>,
    error: Option<String>,
    running: bool,
}

impl SqlView {
    pub fn new(
        title: SharedString,
        db: Entity<DbStore>,
        profile: String,
        sql: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language("sql")
                .line_number(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                .placeholder(
                    match crate::keymap::keys_for(&RunQuery, "SqlEditor", window) {
                        keys if keys.is_empty() => "-- Write SQL, then run it".to_string(),
                        keys => format!("-- Write SQL, then press {keys} to run"),
                    },
                );
            crate::highlight::install(&mut state, cx);
            match sql {
                Some(sql) => state.default_value(sql),
                None => state,
            }
        });
        Self {
            title,
            db,
            profile,
            cancel: None,
            editor,
            result: None,
            summary: None,
            error: None,
            running: false,
        }
    }

    /// Runs the editor's SQL, as ⌘↩ does.
    pub fn run_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(&RunQuery, window, cx);
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
        let Some(client) = self.db.read(cx).client(&self.profile, false) else {
            // Not re-run automatically: a write may have reached the server
            // before the connection dropped.
            let profile = self.profile.clone();
            self.db
                .update(cx, |s, cx| s.ensure_connecting(&profile, cx));
            self.error = Some(
                "Connection lost. Reconnecting now; run the query again when the dot turns green."
                    .into(),
            );
            cx.notify();
            return;
        };
        self.running = true;
        self.error = None;
        self.cancel = client.cancel_token();
        cx.notify();
        if let Conn::Lite(lite) = client {
            self.run_lite(lite, sql, window, cx);
            return;
        }
        let rx = rt::spawn(cx, async move { client.run_sql(&sql, MAX_ROWS).await });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                this.running = false;
                this.cancel = None;
                match result {
                    Ok(Ok(outcome)) => this.show(outcome, None, window, cx),
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// SQLite and D1: runs `sql` read-only when it only reads, else asks first.
    fn run_lite(
        &mut self,
        lite: std::sync::Arc<Lite>,
        sql: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (handle, text) = (lite.clone(), sql.clone());
        let rx = rt::spawn(
            cx,
            async move { handle.run_if_read_only(&text, MAX_ROWS).await },
        );
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                match result {
                    Ok(Ok(ReadOnlyTry::Read(outcome))) => {
                        this.running = false;
                        this.show(outcome, None, window, cx);
                    }
                    Ok(Ok(_)) => this.confirm_write(lite, sql, window, cx),
                    Ok(Err(e)) => {
                        this.running = false;
                        this.error = Some(e.to_string());
                    }
                    Err(_) => this.running = false,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks before SQL that changes a SQLite or D1 database, saying where the change goes: the
    /// same question Sawit asks before bulk changes, and a critical one for a remote
    /// (production) database.
    fn confirm_write(
        &mut self,
        lite: std::sync::Arc<Lite>,
        sql: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.db.read(cx);
        let name = store.accessible_name(&self.profile);
        let route = store.route(&self.profile).cloned();
        let remote = lite.profile.kind == DbKind::D1Remote;
        let message = if remote {
            format!("Change {name}? It is remote (production).")
        } else {
            format!("Change {name}?")
        };
        let detail = format!(
            "{}\n\nThis SQL changes the database, and it can't be undone.",
            route
                .as_ref()
                .map(WriteRoute::describe)
                .unwrap_or_else(|| "Sawit looks for a running dev server first.".into())
        );
        let level = if remote {
            PromptLevel::Critical
        } else {
            PromptLevel::Warning
        };
        let answer = window.prompt(level, &message, Some(&detail), &["Run", "Cancel"], cx);
        cx.spawn_in(window, async move |this, window| {
            let go = answer.await == Ok(0);
            let _ = this.update_in(window, |this, window, cx| {
                if go {
                    this.write(lite, sql, window, cx);
                } else {
                    this.running = false;
                    this.summary = Some("Canceled: nothing was changed.".into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn write(
        &mut self,
        lite: std::sync::Arc<Lite>,
        sql: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = rt::spawn(cx, async move { lite.write(&sql, MAX_ROWS).await });
        cx.spawn_in(window, async move |this, window| {
            let result = rx.await;
            let _ = this.update_in(window, |this, window, cx| {
                this.running = false;
                match result {
                    Ok(Ok((outcome, route))) => this.show(outcome, Some(&route), window, cx),
                    Ok(Err(e)) => this.error = Some(format!("{e}\nNothing was changed.")),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.cancel.take()
            && let Some(profile) = self.db.read(cx).profile(&self.profile).cloned()
        {
            // Fire and forget: the running query reports "Query canceled.".
            drop(rt::spawn(cx, async move {
                sawit_core::db::cancel(&token, &profile).await
            }));
        }
    }

    /// Shows what the SQL returned; `route` is where a SQLite or D1 change went.
    fn show(
        &mut self,
        outcome: QueryOutcome,
        route: Option<&WriteRoute>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ms = outcome.duration.as_secs_f64() * 1000.;
        let mut parts = vec![format!("{ms:.1} ms")];
        if let Some(route) = route {
            parts.push(format!("written {}", route.label()));
        }
        if !outcome.affected.is_empty() {
            let total: u64 = outcome.affected.iter().sum();
            let noun = if total == 1 { "row" } else { "rows" };
            parts.push(format!("{total} {noun} affected"));
        }
        match outcome.results.into_iter().last() {
            Some(set) => {
                let n = set.rows.len();
                parts.push(format!(
                    "{n} {}{}",
                    if n == 1 { "row" } else { "rows" },
                    if set.truncated { " (truncated)" } else { "" }
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let run_keys = crate::keymap::keys_for(&RunQuery, "SqlEditor", window);
        let (run_label, run_hint) = if run_keys.is_empty() {
            (
                "Run".to_string(),
                "Write a query above and run it.".to_string(),
            )
        } else {
            (
                format!("Run {run_keys}"),
                format!("Write a query above and press {run_keys} to run it."),
            )
        };
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
                    .child({
                        // The target: its kind's logo, named in the tooltip.
                        let store = self.db.read(cx);
                        let kind = store
                            .profile(&self.profile)
                            .map_or(DbKind::Postgres, |p| p.kind);
                        let name: SharedString = store.accessible_name(&self.profile).into();
                        div()
                            .id("sql-target")
                            .child(super::logos::logo(kind, 14.))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(name.clone())
                                    .build(window, cx)
                            })
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .flex_1()
                            .child(self.title.clone()),
                    )
                    .children(super::panel::route_badge(
                        "sql-route",
                        self.db.read(cx).route(&self.profile),
                        cx,
                    ))
                    .when(self.running, |el| {
                        el.child(
                            Button::new("cancel-sql")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::X)
                                .label("Cancel")
                                .tooltip("Cancel the Query on the Server")
                                .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                        )
                    })
                    .child(
                        Button::new("run-sql")
                            .primary()
                            .xsmall()
                            .icon(IconName::Play)
                            .label(run_label)
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
                        "Running…".to_string()
                    } else {
                        self.summary.clone().unwrap_or_else(|| "Ready".into())
                    }),
            )
            .child(div().flex_1().min_h_0().map(|el| {
                match (self.error.clone(), self.result.clone()) {
                    (Some(e), _) => {
                        el.child(Placeholder::error("The query didn't complete").detail(e))
                    }
                    (None, Some(table)) => el.child(DataTable::new(&table).stripe(true).small()),
                    (None, None) if self.running => {
                        el.child(Placeholder::loading("Running query…"))
                    }
                    (None, None) if let Some(summary) = self.summary.clone() => {
                        el.child(Placeholder::new(Lucide::Check, "Query complete").detail(summary))
                    }
                    (None, None) => {
                        el.child(Placeholder::new(Lucide::Play, "No results yet").detail(run_hint))
                    }
                }
            }))
    }
}
