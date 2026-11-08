//! The Pomodoro pill at the canvas's bottom right, above the toolbar ([`crate::pomodoro`]): Focus
//! 25:00 to start; while it runs or waits paused, the phase, the time left and the round, with
//! Pause or Resume, Skip and Stop. It is on every canvas unless the View menu's Pomodoro Timer hides
//! it, never over Preview or Present, and draws itself again each second while it runs.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::*;

use super::BoardView;
use super::float_bar::{BarButton, FloatBar};
use crate::menu_icons;
use crate::pomodoro::{self, Phase, ROUNDS, Run};

impl BoardView {
    /// Draws the pill again in a second while the timer runs.
    fn tick_pomodoro(&mut self, cx: &mut Context<Self>) {
        if self.timer_ticking || !pomodoro::get(cx).running() {
            return;
        }
        self.timer_ticking = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |view, cx| {
                view.timer_ticking = false;
                cx.notify();
                view.tick_pomodoro(cx);
            });
        })
        .detach();
    }
}

/// The pill: the canvas's one floating bar ([`FloatBar`]), at its bottom right above the toolbar,
/// its buttons collapsing to icons when the canvas is narrow.
pub(super) fn render_facilitation(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> Option<AnyElement> {
    // Never over Preview or Present.
    if view.preview_open() || view.player.is_some() {
        return None;
    }
    let timer = pomodoro::get(cx);
    if !timer.shown {
        return None;
    }
    let ds = view.board()?.session.ds().ok()?;
    // Half the canvas at most.
    let room = (ds.view.vw / 2.0) as f32;
    let now = Instant::now();
    let act = |change: fn(&mut pomodoro::Pomodoro, Instant)| {
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| pomodoro::update(cx, change)
    };
    let mut pill = FloatBar::new("board-pomodoro");
    match timer.run {
        Run::Idle => {
            pill = pill.button(
                BarButton::new(
                    "pomodoro-start",
                    Lucide::Timer,
                    format!(
                        "{} {}",
                        timer.phase.label(),
                        pomodoro::clock(timer.left(now))
                    ),
                )
                .tooltip("Start a 25-minute focus")
                .on_click(act(|p, now| p.start(now))),
            );
            if timer.done > 0 {
                pill = pill.text(
                    "pomodoro-round",
                    format!("Round {} of {ROUNDS}", timer.round()),
                    false,
                );
            }
        }
        Run::Running { .. } | Run::Paused { .. } => {
            let running = timer.running();
            let label = match timer.phase {
                Phase::Focus => format!("Focus {} of {ROUNDS}", timer.round()),
                phase => phase.label().to_string(),
            };
            let (icon, verb) = match running {
                true => (Lucide::Pause, "Pause"),
                false => (Lucide::Play, "Resume"),
            };
            let skip_to = match timer.phase {
                Phase::Focus => "Skip to the Break",
                _ => "Skip to the Next Focus",
            };
            pill = pill
                .text("pomodoro-phase", label, false)
                .text("pomodoro-left", pomodoro::clock(timer.left(now)), false)
                .divider()
                .button(
                    BarButton::new("pomodoro-pause", icon, verb).on_click(act(|p, now| {
                        match p.running() {
                            true => p.pause(now),
                            false => p.start(now),
                        }
                    })),
                )
                .button(
                    BarButton::new("pomodoro-skip", Lucide::SkipForward, "Skip")
                        .tooltip(skip_to)
                        .on_click(act(|p, now| {
                            p.skip(now);
                        })),
                )
                .button(
                    BarButton::new("pomodoro-stop", menu_icons::STOP, "Stop")
                        .tooltip("Stop, Back to a Focus Not Started")
                        .on_click(act(|p, _| p.stop())),
                );
            if running {
                view.tick_pomodoro(cx);
            }
        }
    }
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
