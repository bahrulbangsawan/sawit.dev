//! The pane's activity (busy, exited with a code, needs attention, …), worked out by
//! [`sawit_core::activity`] from what the view knows, and told to its tab when it changes.

use std::os::unix::process::ExitStatusExt as _;
use std::time::{Duration, Instant};

use gpui_kit::*;
use sawit_core::activity::{AGENT_QUIET_SECS, Activity, TerminalSignals};

use super::{TerminalEvent, TerminalView};

/// Output this soon after a keystroke is the keystroke's echo, not the program at work.
const ECHO: Duration = Duration::from_millis(250);

impl TerminalView {
    /// The pane's state as last worked out: soon after output, on the bell and on exit.
    pub fn activity(&self) -> Activity {
        self.activity
    }

    /// The program's exit code, once it exited with one.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_status.and_then(|s| s.code())
    }

    /// The program printed: the agent's quiet clock starts again, unless it is the echo of
    /// what the user typed.
    pub(super) fn saw_output(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let echo = self
            .typed_at
            .get()
            .is_some_and(|at| now.saturating_duration_since(at) < ECHO);
        if echo {
            return;
        }
        self.last_output = Some(now);
        if self.agent.is_some() {
            if self.activity != Activity::Busy {
                self.refresh_activity(cx);
            }
            self.check_quiet_soon(cx);
        }
    }

    /// Looks again once the output has been quiet for [`AGENT_QUIET_SECS`].
    fn check_quiet_soon(&mut self, cx: &mut Context<Self>) {
        if self.quiet_check.is_some() {
            return;
        }
        self.quiet_check = Some(cx.spawn(async move |this, cx| {
            loop {
                let wait = this.update(cx, |this, _| this.quiet_left()).ok().flatten();
                let Some(wait) = wait else { break };
                cx.background_executor().timer(wait).await;
            }
            let _ = this.update(cx, |this, cx| {
                this.quiet_check = None;
                this.refresh_activity(cx);
            });
        }));
    }

    /// How long until the output has been quiet long enough; `None` once it has.
    fn quiet_left(&self) -> Option<Duration> {
        let quiet = Duration::from_secs_f32(AGENT_QUIET_SECS);
        let since = self.last_output?.elapsed();
        (since < quiet).then(|| quiet - since + Duration::from_millis(20))
    }

    /// What the view knows, for [`TerminalSignals::activity`].
    fn signals(&self) -> TerminalSignals {
        let ssh = self.remote.is_some();
        TerminalSignals {
            exited: self.exited,
            exit_code: self.exit_code(),
            signaled: self.exit_status.is_some_and(|s| s.signal().is_some()),
            // An SSH session's `ssh` runs for as long as it is connected: that is not work.
            foreground_job: !ssh && self.foreground_job().is_some(),
            agent: self.agent.is_some(),
            recent_output: self.quiet_left().is_some(),
            script: self.job.is_some(),
            ssh,
            bell: self.bell,
        }
    }

    /// Works the state out again, and tells the tab when it changed.
    pub(super) fn refresh_activity(&mut self, cx: &mut Context<Self>) {
        let activity = self.signals().activity();
        if activity != self.activity {
            self.activity = activity;
            cx.emit(TerminalEvent::ActivityChanged);
            cx.notify();
        }
    }
}
