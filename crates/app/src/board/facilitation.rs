//! A board's timer and voting, in a pill at the canvas's top right (`board.timer`, `board.vote`,
//! `board.voteResults`): the countdown with Start 5 min, Pause or Resume, +1 min and Stop; and
//! Start vote (3 votes each), Vote on the selection, End vote and the results, most votes first.
//! The countdown is the board's own, the same for everyone in a live session; the pill draws
//! itself again each second while it runs.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use figcraft_doc::BoardTimer;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use serde_json::{Value, json};

use super::BoardView;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn clock(ms: u64) -> String {
    let s = ms.div_ceil(1000);
    format!("{}:{:02}", s / 60, s % 60)
}

impl BoardView {
    fn facilitate(&mut self, command: &'static str, params: Value, cx: &mut Context<Self>) {
        self.choose(command, params, cx);
        self.tick_timer(cx);
    }

    /// Draws the pill again in a second while the timer runs.
    fn tick_timer(&mut self, cx: &mut Context<Self>) {
        if self.timer_ticking {
            return;
        }
        let running = self
            .board()
            .and_then(|b| b.session.ds().ok())
            .is_some_and(|ds| matches!(ds.doc.timer, Some(BoardTimer::Running { .. })));
        if !running {
            return;
        }
        self.timer_ticking = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |view, cx| {
                view.timer_ticking = false;
                cx.notify();
                view.tick_timer(cx);
            });
        })
        .detach();
    }

    fn show_vote_results(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.board_mut() else {
            return;
        };
        let Ok(results) = board.execute("board.voteResults", &json!({})) else {
            return;
        };
        let lines: Vec<String> = results
            .get("results")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .take(5)
                    .map(|r| {
                        let name = r.get("name").and_then(Value::as_str).unwrap_or("Item");
                        let votes = r.get("votes").and_then(Value::as_u64).unwrap_or(0);
                        format!("{name}: {votes}")
                    })
                    .collect()
            })
            .unwrap_or_default();
        let said = if lines.is_empty() {
            "No votes yet".to_string()
        } else {
            format!("Votes: {}", lines.join(" · "))
        };
        self.show_toast(said, cx);
    }
}

/// The pill, on a board.
pub(super) fn render_facilitation(
    view: &mut BoardView,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    let board = view.board()?;
    if board.kind() != sawit_board::DocKind::Board {
        return None;
    }
    let ds = board.session.ds().ok()?;
    let timer = ds.doc.timer;
    let voting = ds.doc.vote.as_ref().map(|v| !v.ended);
    let has_selection = !ds.selection.is_empty();
    let theme = cx.theme().clone();
    let now = now_ms();
    let button = |id: &'static str, label: String| Button::new(id).ghost().xsmall().label(label);
    let mut pill = h_flex()
        .id("board-facilitation")
        .debug_selector(|| "board-facilitation".into())
        .occlude()
        .absolute()
        .top(px(8.))
        .right(px(8.))
        .gap_1()
        .px_2()
        .py_1()
        .items_center()
        .rounded(px(10.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow_md()
        .text_xs();
    match timer {
        None => {
            pill = pill.child(
                button("timer-start", "Timer 5:00".into()).on_click(cx.listener(
                    |view, _, _, cx| {
                        view.facilitate("board.timer", json!({"action": "start", "minutes": 5}), cx)
                    },
                )),
            );
        }
        Some(t) => {
            let left = t.left(now);
            let running = matches!(t, BoardTimer::Running { .. });
            pill = pill
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if left == 0 {
                            theme.danger
                        } else {
                            theme.popover_foreground
                        })
                        .child(clock(left)),
                )
                .child(
                    button(
                        "timer-pause",
                        if running { "Pause" } else { "Resume" }.into(),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| {
                        let action = if running { "pause" } else { "resume" };
                        view.facilitate("board.timer", json!({"action": action}), cx)
                    })),
                )
                .child(
                    button("timer-add", "+1".into()).on_click(cx.listener(|view, _, _, cx| {
                        view.facilitate("board.timer", json!({"action": "add", "minutes": 1}), cx)
                    })),
                )
                .child(button("timer-stop", "Stop".into()).on_click(cx.listener(
                    |view, _, _, cx| view.facilitate("board.timer", json!({"action": "stop"}), cx),
                )));
            if running {
                view.tick_timer(cx);
            }
        }
    }
    pill = pill.child(div().w(px(1.)).h(px(16.)).bg(theme.border));
    pill = match voting {
        Some(true) => pill
            .child(
                button("vote-cast", "Vote on selection".into())
                    .disabled(!has_selection)
                    .on_click(cx.listener(|view, _, _, cx| {
                        let ids = view
                            .board()
                            .map(|b| b.session.selection().to_vec())
                            .unwrap_or_default();
                        view.facilitate("board.vote", json!({"action": "cast", "ids": ids}), cx)
                    })),
            )
            .child(button("vote-end", "End vote".into()).on_click(cx.listener(
                |view, _, _, cx| {
                    view.facilitate("board.vote", json!({"action": "end"}), cx);
                    view.show_vote_results(cx);
                },
            ))),
        _ => pill
            .child(
                button("vote-start", "Start vote".into()).on_click(cx.listener(
                    |view, _, _, cx| {
                        view.facilitate("board.vote", json!({"action": "start", "votes": 3}), cx)
                    },
                )),
            )
            .child(
                button("vote-results", "Results".into())
                    .on_click(cx.listener(|view, _, _, cx| view.show_vote_results(cx))),
            ),
    };
    Some(pill.into_any_element())
}
