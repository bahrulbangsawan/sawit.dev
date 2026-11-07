//! A board's timer and voting, in a pill at the canvas's bottom right, above the toolbar, shown only
//! while one runs or a vote has results (`board.timer`, `board.vote`,
//! `board.voteResults`): the countdown with Start 5 min, Pause or Resume, +1 min and Stop; and
//! Start vote (3 votes each), Vote on the selection, End vote and the results, most votes first.
//! The countdown is the board's own, the same for everyone in a live session; the pill draws
//! itself again each second while it runs.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use figcraft_doc::BoardTimer;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::*;
use serde_json::{Value, json};

use super::BoardView;
use super::float_bar::{BarButton, FloatBar};
use crate::menu_icons;

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

    /// The main menu's Start Timer (5 min) or Start Vote (3 votes each): on a board only.
    pub(super) fn start_facilitation(&mut self, what: &str, cx: &mut Context<Self>) {
        let board = self
            .board()
            .is_some_and(|b| b.kind() == sawit_board::DocKind::Board);
        if !board {
            self.show_toast("Timers and votes are for boards", cx);
            return;
        }
        match what {
            "timer" => self.facilitate("board.timer", json!({"action": "start", "minutes": 5}), cx),
            _ => self.facilitate("board.vote", json!({"action": "start", "votes": 3}), cx),
        }
        cx.notify();
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

/// The pill, on a board, while a timer or a vote is on: the canvas's one floating bar
/// ([`FloatBar`]), at its bottom right above the toolbar, its
/// buttons collapsing to icons when the canvas is narrow.
pub(super) fn render_facilitation(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    // Never over Preview or Present.
    if view.preview_open() || view.player.is_some() {
        return None;
    }
    let board = view.board()?;
    if board.kind() != sawit_board::DocKind::Board {
        return None;
    }
    let ds = board.session.ds().ok()?;
    let timer = ds.doc.timer;
    let voting = ds.doc.vote.as_ref().map(|v| !v.ended);
    // Only while a timer or a vote is on, or a vote has results (B15): the main menu's View ›
    // Start Timer and Start Vote begin them.
    if timer.is_none() && voting.is_none() {
        return None;
    }
    let has_selection = !ds.selection.is_empty();
    // Half the canvas at most.
    let room = (ds.view.vw / 2.0) as f32;
    let now = now_ms();
    let this = cx.entity().downgrade();
    let act = |command: &'static str, params: Value| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |view, cx| view.facilitate(command, params.clone(), cx));
        }
    };
    let mut pill = FloatBar::new("board-facilitation");
    match timer {
        None => {
            pill = pill.button(
                BarButton::new("timer-start", Lucide::Timer, "Timer 5:00")
                    .tooltip("Start a 5-minute timer for everyone on the board")
                    .on_click(act("board.timer", json!({"action": "start", "minutes": 5}))),
            );
        }
        Some(t) => {
            let left = t.left(now);
            let running = matches!(t, BoardTimer::Running { .. });
            let (icon, label, action) = match running {
                true => (Lucide::Pause, "Pause", "pause"),
                false => (Lucide::Play, "Resume", "resume"),
            };
            pill = pill
                .text("timer-left", clock(left), left == 0)
                .button(
                    BarButton::new("timer-pause", icon, label)
                        .on_click(act("board.timer", json!({"action": action}))),
                )
                .button(
                    BarButton::new("timer-add", Lucide::Plus, "1 Min")
                        .tooltip("Add a minute")
                        .on_click(act("board.timer", json!({"action": "add", "minutes": 1}))),
                )
                .button(
                    BarButton::new("timer-stop", menu_icons::STOP, "Stop")
                        .on_click(act("board.timer", json!({"action": "stop"}))),
                );
            if running {
                view.tick_timer(cx);
            }
        }
    }
    pill = pill.divider();
    pill = match voting {
        Some(true) => {
            let cast = {
                let this = this.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |view, cx| {
                        let ids = view
                            .board()
                            .map(|b| b.session.selection().to_vec())
                            .unwrap_or_default();
                        view.facilitate("board.vote", json!({"action": "cast", "ids": ids}), cx)
                    });
                }
            };
            let end = {
                let this = this.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |view, cx| {
                        view.facilitate("board.vote", json!({"action": "end"}), cx);
                        view.show_vote_results(cx);
                    });
                }
            };
            pill.button(
                BarButton::new("vote-cast", Lucide::Vote, "Vote on Selection")
                    .disabled(!has_selection)
                    .on_click(cast),
            )
            .button(BarButton::new("vote-end", Lucide::SquareCheck, "End Vote").on_click(end))
        }
        _ => {
            let results = {
                let this = this.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |view, cx| view.show_vote_results(cx));
                }
            };
            pill.button(
                BarButton::new("vote-start", Lucide::Vote, "Start Vote")
                    .tooltip("Start a vote, 3 votes each")
                    .on_click(act("board.vote", json!({"action": "start", "votes": 3}))),
            )
            .button(
                BarButton::new("vote-results", Lucide::ChartColumn, "Results").on_click(results),
            )
        }
    };
    Some(
        div()
            .absolute()
            // Above the toolbar's band at the bottom right, clear of the screen and context bars
            // at the canvas's top (B15).
            .bottom(px(super::toolbar::BOTTOM + super::toolbar::HEIGHT + 8.))
            .right(px(8.))
            .child(pill.render_in_place(room, window, cx))
            .into_any_element(),
    )
}
