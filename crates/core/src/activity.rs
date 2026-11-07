//! One set of activity states for every tab that does work: terminals, browser tabs and canvases.
//! The tab strips draw them as icons (animated only while busy) and `app_state` reports them, so
//! the user and agents read the same thing.
//!
//! The state is worked out here from plain signals, so the rules are tested without a window.

use serde::Serialize;

/// What a tab is doing, as its icon shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Nothing running: a shell at its prompt, a loaded page, a quiet canvas.
    Idle,
    /// Work in progress: a command running, a page loading, an agent working or drawing.
    Busy,
    /// The program exited with code 0.
    Succeeded,
    /// The program exited with another code (`code`), or was killed by a signal (`None`).
    Failed { code: Option<i32> },
    /// Stopped without a result: a script whose command ended while its shell lives on, or an
    /// SSH session that disconnected.
    Stopped,
    /// Something asked for the user while they looked elsewhere (the terminal's bell).
    Attention,
}

impl Activity {
    /// The id `app_state` reports (`busy`, `failed`, …).
    pub fn id(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Succeeded => "succeeded",
            Self::Failed { .. } => "failed",
            Self::Stopped => "stopped",
            Self::Attention => "attention",
        }
    }

    /// What the tab's tooltip and VoiceOver say, in sentence case.
    pub fn label(self) -> String {
        match self {
            Self::Idle => "Idle".into(),
            Self::Busy => "Running".into(),
            Self::Succeeded => "Exited with code 0".into(),
            Self::Failed { code: Some(code) } => format!("Exited with code {code}"),
            Self::Failed { code: None } => "Ended by a signal".into(),
            Self::Stopped => "Stopped".into(),
            Self::Attention => "Needs attention".into(),
        }
    }

    /// Only busy work animates; every other state is still.
    pub fn animates(self) -> bool {
        self == Self::Busy
    }

    /// The exit code, for the states that have one.
    pub fn exit_code(self) -> Option<i32> {
        match self {
            Self::Succeeded => Some(0),
            Self::Failed { code } => code,
            _ => None,
        }
    }

    /// The state and exit code as `app_state` shows them: `{"state": "failed", "label": …,
    /// "exit_code": 1}`.
    pub fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "state": self.id(),
            "label": self.label(),
            "exit_code": self.exit_code(),
        })
    }

    /// The more pressing of two states, for a tab with several panes: attention first, then a
    /// failure, busy work, a stop, a success, and idle last.
    pub fn most_pressing(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Succeeded => 1,
            Self::Stopped => 2,
            Self::Busy => 3,
            Self::Failed { .. } => 4,
            Self::Attention => 5,
        }
    }
}

/// How long after its last output an agent still counts as working: agents redraw a spinner
/// while they think, and fall quiet while they wait for the user.
pub const AGENT_QUIET_SECS: f32 = 3.0;

/// What a terminal pane shows, read from its view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalSignals {
    /// The pane's program exited, with its exit code when it had one.
    pub exited: bool,
    pub exit_code: Option<i32>,
    /// The program ended by a signal rather than an exit code.
    pub signaled: bool,
    /// A program runs in the foreground (not the shell at its prompt).
    pub foreground_job: bool,
    /// That program is a coding agent.
    pub agent: bool,
    /// The pane printed something in the last [`AGENT_QUIET_SECS`].
    pub recent_output: bool,
    /// A script's tab, or an SSH session's: when its command ends the tab says stopped.
    pub script: bool,
    pub ssh: bool,
    /// The bell rang since the user last looked at the pane.
    pub bell: bool,
}

impl TerminalSignals {
    /// The pane's state.
    pub fn activity(self) -> Activity {
        if self.exited {
            return match (self.exit_code, self.signaled, self.ssh) {
                (_, _, true) => Activity::Stopped,
                (Some(0), _, _) => Activity::Succeeded,
                (Some(code), _, _) => Activity::Failed { code: Some(code) },
                (None, true, _) => Activity::Failed { code: None },
                (None, false, _) => Activity::Stopped,
            };
        }
        if self.bell {
            return Activity::Attention;
        }
        if self.foreground_job {
            // An agent works while it prints; quiet, it waits for the user.
            return if !self.agent || self.recent_output {
                Activity::Busy
            } else {
                Activity::Idle
            };
        }
        if self.script {
            // A script's command ended and its shell lives on.
            return Activity::Stopped;
        }
        Activity::Idle
    }
}

/// A browser tab's state: busy while its page loads or an agent acts on it.
pub fn browser(loading: bool, agent_at_work: bool) -> Activity {
    if loading || agent_at_work {
        Activity::Busy
    } else {
        Activity::Idle
    }
}

/// A canvas's state: busy while an agent draws on it.
pub fn canvas(agent_at_work: bool) -> Activity {
    if agent_at_work {
        Activity::Busy
    } else {
        Activity::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term() -> TerminalSignals {
        TerminalSignals::default()
    }

    #[test]
    fn a_shell_at_its_prompt_is_idle_and_a_command_is_busy() {
        assert_eq!(term().activity(), Activity::Idle);
        let running = TerminalSignals {
            foreground_job: true,
            ..term()
        };
        assert_eq!(running.activity(), Activity::Busy);
        assert!(running.activity().animates());
    }

    #[test]
    fn exit_codes_give_success_or_failure() {
        let exited = |code, signaled| TerminalSignals {
            exited: true,
            exit_code: code,
            signaled,
            ..term()
        };
        assert_eq!(exited(Some(0), false).activity(), Activity::Succeeded);
        assert_eq!(
            exited(Some(2), false).activity(),
            Activity::Failed { code: Some(2) }
        );
        assert_eq!(
            exited(None, true).activity(),
            Activity::Failed { code: None }
        );
        assert_eq!(exited(None, false).activity(), Activity::Stopped);
        assert_eq!(exited(Some(2), false).activity().exit_code(), Some(2));
        assert_eq!(
            exited(Some(2), false).activity().label(),
            "Exited with code 2"
        );
    }

    #[test]
    fn an_ssh_session_that_ends_is_stopped_whatever_its_code() {
        let gone = TerminalSignals {
            exited: true,
            exit_code: Some(255),
            ssh: true,
            ..term()
        };
        assert_eq!(gone.activity(), Activity::Stopped);
    }

    #[test]
    fn an_agent_is_busy_only_while_it_prints() {
        let agent = TerminalSignals {
            foreground_job: true,
            agent: true,
            ..term()
        };
        assert_eq!(agent.activity(), Activity::Idle);
        let working = TerminalSignals {
            recent_output: true,
            ..agent
        };
        assert_eq!(working.activity(), Activity::Busy);
    }

    #[test]
    fn the_bell_asks_for_attention_until_the_program_exits() {
        let bell = TerminalSignals {
            bell: true,
            foreground_job: true,
            ..term()
        };
        assert_eq!(bell.activity(), Activity::Attention);
        assert!(!bell.activity().animates());
        let exited = TerminalSignals {
            exited: true,
            exit_code: Some(1),
            ..bell
        };
        assert_eq!(exited.activity(), Activity::Failed { code: Some(1) });
    }

    #[test]
    fn a_script_whose_command_ended_is_stopped() {
        let script = TerminalSignals {
            script: true,
            ..term()
        };
        assert_eq!(script.activity(), Activity::Stopped);
        let running = TerminalSignals {
            foreground_job: true,
            ..script
        };
        assert_eq!(running.activity(), Activity::Busy);
    }

    #[test]
    fn browser_and_canvas_are_busy_while_working() {
        assert_eq!(browser(true, false), Activity::Busy);
        assert_eq!(browser(false, true), Activity::Busy);
        assert_eq!(browser(false, false), Activity::Idle);
        assert_eq!(canvas(true), Activity::Busy);
        assert_eq!(canvas(false), Activity::Idle);
    }

    #[test]
    fn the_most_pressing_state_wins_across_panes() {
        let states = [
            Activity::Idle,
            Activity::Succeeded,
            Activity::Busy,
            Activity::Failed { code: Some(1) },
        ];
        let tab = states
            .into_iter()
            .fold(Activity::Idle, Activity::most_pressing);
        assert_eq!(tab, Activity::Failed { code: Some(1) });
        assert_eq!(
            Activity::Busy.most_pressing(Activity::Attention),
            Activity::Attention
        );
    }

    #[test]
    fn json_has_the_state_label_and_code() {
        let json = Activity::Failed { code: Some(3) }.to_json();
        assert_eq!(json["state"], "failed");
        assert_eq!(json["exit_code"], 3);
        assert_eq!(
            Activity::Busy.to_json()["exit_code"],
            serde_json::Value::Null
        );
    }
}
