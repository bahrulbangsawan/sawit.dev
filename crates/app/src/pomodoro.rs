//! The Pomodoro timer: 25 minutes of focus, then a 5-minute break, and a 15-minute break after
//! every fourth focus. One for the whole app, so it keeps time while the user moves between
//! canvases; its pill is on each canvas ([`crate::board`]'s `render_pomodoro`) unless the View
//! menu's Pomodoro Timer hides it. A focus that ends starts its break; a break that ends waits for
//! the user to start the next focus. Each end says so in a macOS notification, as the pill may be
//! off screen.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::*;
use serde_json::{Value, json};

/// How long each part lasts.
pub const FOCUS: Duration = Duration::from_secs(25 * 60);
pub const SHORT_BREAK: Duration = Duration::from_secs(5 * 60);
pub const LONG_BREAK: Duration = Duration::from_secs(15 * 60);
/// Focuses before the long break.
pub const ROUNDS: u32 = 4;

/// What the timer counts down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn length(self) -> Duration {
        match self {
            Phase::Focus => FOCUS,
            Phase::ShortBreak => SHORT_BREAK,
            Phase::LongBreak => LONG_BREAK,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Focus",
            Phase::ShortBreak => "Break",
            Phase::LongBreak => "Long Break",
        }
    }
}

/// Whether it counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    /// Not started: the phase's whole length waits.
    Idle,
    Running {
        ends: Instant,
    },
    Paused {
        left: Duration,
    },
}

/// The timer.
#[derive(Clone, Debug)]
pub struct Pomodoro {
    /// The pill shows on the canvases.
    pub shown: bool,
    pub phase: Phase,
    pub run: Run,
    /// Focuses finished in this cycle of [`ROUNDS`].
    pub done: u32,
    /// Bumped on every change, so a wait for an end that no longer holds does nothing.
    generation: u64,
    file: Option<PathBuf>,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Pomodoro {
            shown: true,
            phase: Phase::Focus,
            run: Run::Idle,
            done: 0,
            generation: 0,
            file: None,
        }
    }
}

impl Global for Pomodoro {}

impl Pomodoro {
    /// The time left at `now`.
    pub fn left(&self, now: Instant) -> Duration {
        match self.run {
            Run::Idle => self.phase.length(),
            Run::Running { ends } => ends.saturating_duration_since(now),
            Run::Paused { left } => left,
        }
    }

    pub fn running(&self) -> bool {
        matches!(self.run, Run::Running { .. })
    }

    /// Starts the phase waiting (idle) or goes on (paused).
    pub fn start(&mut self, now: Instant) {
        let left = self.left(now);
        self.run = Run::Running { ends: now + left };
        self.generation += 1;
    }

    pub fn pause(&mut self, now: Instant) {
        if let Run::Running { ends } = self.run {
            self.run = Run::Paused {
                left: ends.saturating_duration_since(now),
            };
            self.generation += 1;
        }
    }

    /// Back to a focus not started, the cycle's rounds kept.
    pub fn stop(&mut self) {
        self.phase = Phase::Focus;
        self.run = Run::Idle;
        self.generation += 1;
    }

    /// Ends the phase now, as if its time were up; what comes next ([`Self::finish`]).
    pub fn skip(&mut self, now: Instant) -> Phase {
        self.finish(now)
    }

    /// The phase is over: a focus counts and its break starts; a break leads to a focus that waits
    /// for the user. The phase now.
    pub fn finish(&mut self, now: Instant) -> Phase {
        match self.phase {
            Phase::Focus => {
                self.done += 1;
                self.phase = if self.done.is_multiple_of(ROUNDS) {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                };
                self.run = Run::Running {
                    ends: now + self.phase.length(),
                };
            }
            Phase::ShortBreak | Phase::LongBreak => {
                if self.phase == Phase::LongBreak {
                    self.done = 0;
                }
                self.phase = Phase::Focus;
                self.run = Run::Idle;
            }
        }
        self.generation += 1;
        self.phase
    }

    /// The round the focus is in, 1 to [`ROUNDS`].
    pub fn round(&self) -> u32 {
        (self.done % ROUNDS) + 1
    }
}

/// `25:00`, `4:59`: minutes and seconds, the seconds rounded up.
pub fn clock(left: Duration) -> String {
    let s = left.as_millis().div_ceil(1000) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The timer, as it is now.
pub fn get(cx: &App) -> Pomodoro {
    cx.try_global::<Pomodoro>().cloned().unwrap_or_default()
}

/// Changes the timer with `change`, then waits for the end of what runs, and redraws.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Pomodoro, Instant)) {
    let now = Instant::now();
    let pomodoro = cx.default_global::<Pomodoro>();
    change(pomodoro, now);
    wait_for_end(cx);
    cx.refresh_windows();
}

/// While a phase runs, waits for its end; then moves on and says so.
fn wait_for_end(cx: &mut App) {
    let pomodoro = cx.default_global::<Pomodoro>();
    let Run::Running { ends } = pomodoro.run else {
        return;
    };
    let generation = pomodoro.generation;
    cx.spawn(async move |cx| {
        let wait = ends.saturating_duration_since(Instant::now());
        cx.background_executor().timer(wait).await;
        cx.update(|cx| {
            let pomodoro = cx.default_global::<Pomodoro>();
            if pomodoro.generation != generation {
                return;
            }
            let ended = pomodoro.phase;
            let next = pomodoro.finish(Instant::now());
            notify(&ended_message(ended, next));
            wait_for_end(cx);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// What the notification says when `ended` is over and `next` is up.
pub fn ended_message(ended: Phase, next: Phase) -> String {
    match (ended, next) {
        (Phase::Focus, Phase::LongBreak) => {
            "Four focuses done. Take a 15-minute break.".to_string()
        }
        (Phase::Focus, _) => "Focus done. Take a 5-minute break.".to_string(),
        _ => "Break's over. Start the next focus when you're ready.".to_string(),
    }
}

/// A macOS notification, best effort (`osascript`; macOS may ask once).
fn notify(text: &str) {
    let script = format!(
        "display notification \"{}\" with title \"Pomodoro\" sound name \"Glass\"",
        text.replace(['"', '\\'], "")
    );
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// The timer as `app_state` and `app_pomodoro` report it.
pub fn describe(cx: &App) -> Value {
    let p = get(cx);
    let now = Instant::now();
    json!({
        "shown": p.shown,
        "phase": match p.phase {
            Phase::Focus => "focus",
            Phase::ShortBreak => "short_break",
            Phase::LongBreak => "long_break",
        },
        "state": match p.run {
            Run::Idle => "idle",
            Run::Running { .. } => "running",
            Run::Paused { .. } => "paused",
        },
        "left": clock(p.left(now)),
        "left_secs": p.left(now).as_secs(),
        "round": p.round(),
        "rounds": ROUNDS,
    })
}

/// An agent's `app_pomodoro`: `action` done as the pill's buttons and the View menu do it; the
/// timer as it is after.
pub fn run(action: &str, cx: &mut App) -> Result<Value, String> {
    match action.trim().to_lowercase().as_str() {
        "state" => {}
        "start" | "resume" => update(cx, |p, now| p.start(now)),
        "pause" => update(cx, |p, now| p.pause(now)),
        "skip" => update(cx, |p, now| {
            p.skip(now);
        }),
        "stop" => update(cx, |p, _| p.stop()),
        "show" | "hide" => {
            let want = action.trim().eq_ignore_ascii_case("show");
            if get(cx).shown != want {
                toggle_shown(cx);
            }
        }
        other => {
            return Err(format!(
                "Unknown action “{other}”: use state, start, pause, skip, stop, show or hide."
            ));
        }
    }
    Ok(describe(cx))
}

/// Whether the View menu ticks `id`: Pomodoro Timer while the pill shows.
pub fn checked(id: &str, cx: &App) -> Option<bool> {
    (id == sawit_board::policy::POMODORO).then(|| get(cx).shown)
}

/// The View menu's Pomodoro Timer: shows or hides the pill, and keeps the choice. A timer that
/// runs goes on hidden, and still says when it ends.
pub fn toggle_shown(cx: &mut App) {
    let pomodoro = cx.default_global::<Pomodoro>();
    pomodoro.shown = !pomodoro.shown;
    save(cx);
    crate::cli::set_menus(cx);
    cx.refresh_windows();
}

/// Reads whether the pill shows from `file` (`pomodoro.json` in the app's folder), and keeps the
/// choice there from now on. Without it, as in tests, the pill shows and a choice lasts the run.
pub fn keep_setting(file: PathBuf, cx: &mut App) {
    let saved = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_default();
    let pomodoro = cx.default_global::<Pomodoro>();
    pomodoro.shown = saved["shown"].as_bool().unwrap_or(true);
    pomodoro.file = Some(file);
}

fn save(cx: &mut App) {
    let pomodoro = cx.default_global::<Pomodoro>();
    let Some(file) = pomodoro.file.clone() else {
        return;
    };
    let text = json!({ "shown": pomodoro.shown }).to_string();
    cx.background_spawn(async move {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = sawit_core::files::write_atomic(&file, text.as_bytes());
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{FOCUS, Phase, Pomodoro, ROUNDS, Run, clock};

    /// A focus that ends starts its break; the fourth starts the long one; a break that ends
    /// leaves a focus waiting, and the long break starts the next cycle.
    #[test]
    fn focus_and_breaks_take_turns() {
        let t0 = Instant::now();
        let mut p = Pomodoro::default();
        assert_eq!(p.left(t0), FOCUS);
        p.start(t0);
        assert_eq!(
            p.left(t0 + Duration::from_secs(60)),
            FOCUS - Duration::from_secs(60)
        );
        for round in 1..=ROUNDS {
            assert_eq!(p.phase, Phase::Focus);
            assert_eq!(p.round(), round);
            p.start(t0);
            let next = p.finish(t0);
            let want = if round == ROUNDS {
                Phase::LongBreak
            } else {
                Phase::ShortBreak
            };
            assert_eq!(next, want);
            assert!(p.running(), "the break starts by itself");
            assert_eq!(p.finish(t0), Phase::Focus);
            assert_eq!(p.run, Run::Idle, "the focus waits for the user");
        }
        assert_eq!(p.done, 0, "the long break starts a new cycle");
    }

    /// Pausing keeps the time left; going on counts from it.
    #[test]
    fn a_pause_keeps_the_time_left() {
        let t0 = Instant::now();
        let mut p = Pomodoro::default();
        p.start(t0);
        p.pause(t0 + Duration::from_secs(90));
        assert_eq!(
            p.left(t0 + Duration::from_secs(600)),
            FOCUS - Duration::from_secs(90)
        );
        p.start(t0 + Duration::from_secs(600));
        assert_eq!(
            p.left(t0 + Duration::from_secs(610)),
            FOCUS - Duration::from_secs(100)
        );
        p.stop();
        assert_eq!((p.phase, p.run), (Phase::Focus, Run::Idle));
    }

    #[test]
    fn the_clock_rounds_seconds_up() {
        assert_eq!(clock(FOCUS), "25:00");
        assert_eq!(clock(Duration::from_millis(299_001)), "5:00");
        assert_eq!(clock(Duration::from_millis(1)), "0:01");
        assert_eq!(clock(Duration::ZERO), "0:00");
    }
}

#[cfg(test)]
mod control_tests {
    use super::{describe, run};

    /// An agent's actions work as the pill's buttons do, and an unknown one says which there are.
    #[gpui_kit::test]
    fn an_agent_drives_the_timer_as_the_pill_does(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            assert_eq!(describe(cx)["state"], "idle");
            assert_eq!(run("start", cx).unwrap()["state"], "running");
            assert_eq!(run("pause", cx).unwrap()["state"], "paused");
            let skipped = run("skip", cx).unwrap();
            assert_eq!(skipped["phase"], "short_break");
            assert_eq!(run("stop", cx).unwrap()["phase"], "focus");
            assert_eq!(run("hide", cx).unwrap()["shown"], false);
            assert_eq!(run("show", cx).unwrap()["shown"], true);
            let error = run("fly", cx).unwrap_err();
            assert!(error.contains("state, start, pause"), "{error}");
        });
    }
}
