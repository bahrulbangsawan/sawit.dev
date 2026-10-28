//! Throwaway integration spike: a GPUI window with a DataTable filled by a
//! query that runs on the Tokio runtime, proving the UI never freezes.

use std::time::{Duration, Instant};

use gpui_kit::component::button::*;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::component::*;
use gpui_kit::*;
use pgdesk_core::rt::Runtime;

pub struct Rt(pub Runtime);
impl Global for Rt {}

struct Rows {
    columns: Vec<Column>,
    rows: Vec<(String, String)>,
}

impl TableDelegate for Rows {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let (name, value) = &self.rows[row_ix];
        match col_ix {
            0 => name.clone(),
            _ => value.clone(),
        }
    }
}

pub struct Spike {
    table: Entity<TableState<Rows>>,
    status: SharedString,
    clicks: usize,
    /// UI-thread ticks counted every 100 ms; they stall if the UI thread blocks.
    ticks: usize,
    renders: usize,
}

impl Spike {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let rows = Rows {
            columns: vec![
                Column::new("name", "Name").width(px(160.)),
                Column::new("value", "Value").width(px(640.)),
            ],
            rows: vec![],
        };
        Self {
            table: cx.new(|cx| TableState::new(rows, window, cx)),
            status: "idle".into(),
            clicks: 0,
            ticks: 0,
            renders: 0,
        }
    }

    /// `PGDESK_SPIKE_AUTORUN=1`: run the slow query, count UI ticks while it
    /// runs, print the evidence and quit.
    pub fn autorun(&mut self, cx: &mut Context<Self>) {
        self.run("SELECT pg_sleep(3), version()", cx);
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let Ok(done) = this.update(cx, |this, cx| {
                    this.ticks += 1;
                    cx.notify();
                    this.status.as_ref() != "loading: SELECT pg_sleep(3), version()"
                }) else {
                    return;
                };
                if done {
                    let _ = this.update(cx, |this, cx| {
                        let rows = this.table.read(cx).delegate().rows.clone();
                        println!(
                            "spike: status={} ticks_during_query={} renders={} wall={:?} rows={:?}",
                            this.status,
                            this.ticks,
                            this.renders,
                            started.elapsed(),
                            rows
                        );
                    });
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    cx.update(|cx| cx.quit());
                    return;
                }
            }
        })
        .detach();
    }

    fn run(&mut self, sql: &'static str, cx: &mut Context<Self>) {
        self.status = format!("loading: {sql}").into();
        cx.notify();
        let url = std::env::var("PGDESK_SPIKE_URL").unwrap_or_else(|_| {
            "host=localhost port=55432 user=postgres dbname=postgres".into()
        });
        let rx = cx.global::<Rt>().0.spawn(async move {
            let started = Instant::now();
            let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await?;
            tokio::spawn(conn);
            let row = client.query_one(sql, &[]).await?;
            let version: String = row.try_get("version")?;
            Ok::<_, tokio_postgres::Error>((version, started.elapsed()))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                let (rows, status): (Vec<(String, String)>, String) = match result {
                    Ok(Ok((version, took))) => (
                        vec![
                            ("version".into(), version),
                            ("duration".into(), format_duration(took)),
                        ],
                        "done".into(),
                    ),
                    Ok(Err(e)) => (vec![("error".into(), e.to_string())], "error".into()),
                    Err(_) => (vec![], "cancelled".into()),
                };
                this.status = status.into();
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().rows = rows;
                    table.refresh(cx);
                });
                cx.notify();
            });
        })
        .detach();
    }
}

fn format_duration(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

impl Render for Spike {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        if std::env::var_os("PGDESK_SPIKE_AUTORUN").is_some() {
            println!(
                "render #{} ticks={} active={}",
                self.renders,
                self.ticks,
                window.is_window_active()
            );
        }
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("run")
                            .primary()
                            .label("Run")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clicks += 1;
                                this.run("SELECT version()", cx)
                            })),
                    )
                    .child(Button::new("run-slow").label("Run slow (3 s)").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.run("SELECT pg_sleep(3), version()", cx)
                        }),
                    ))
                    .child(format!(
                        "status: {} · clicks: {} · ui ticks: {}",
                        self.status, self.clicks, self.ticks
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .child(DataTable::new(&self.table).bordered(true)),
            )
    }
}
