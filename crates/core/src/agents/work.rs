//! A component an agent builds on the canvas, as it says it does: `canvas_begin` makes the
//! component's frame and names the work, the agent's ops that pass the work go into that frame,
//! and `canvas_progress` says which step is running, or that it is done or failed.
//!
//! The frame is a real FigCraft frame in the document, marked as the work's with its plugin data
//! ([`WORK_KEY`]), so the work is found again on the saved canvas with the app closed, and after
//! the app opens it again. What the app draws for a work (its row, its badge, its skeleton) is
//! presence, never part of the document.

use std::time::Duration;

/// The plugin data key on a work's frame that holds the work's id (`w_7f3a91c2`).
pub const WORK_KEY: &str = "sawit.work";
/// The plugin data key on a work's frame that holds its steps, as a JSON array of strings.
pub const WORK_STEPS_KEY: &str = "sawit.work.steps";
/// The plugin data key on a work's frame that holds its kind (`component`).
pub const WORK_KIND_KEY: &str = "sawit.work.kind";

/// The kinds of work an agent can begin: a component it draws with the canvas tools, a UI screen
/// it writes as HTML (`canvas_screen_write`), or a dev server's routes laid out as live frames in a
/// section (`canvas_add_routes` begins those itself).
pub const WORK_KINDS: [&str; 3] = ["component", "screen", "routes"];

/// How long a work that is still running may go without an op or a progress of its agent before
/// it shows as stalled: its agent may have crashed, or forgotten to say it is done.
pub const STALL_AFTER: Duration = Duration::from_secs(60);
/// How long an agent's name stays on show after it last did something: on its work's row, or
/// on its pointer's pill for an op outside a work.
pub const NAME_AFTER: Duration = Duration::from_secs(2);
/// How long a done work's check fades for, once its agent's name has gone.
pub const CHECK_FADE: Duration = Duration::from_millis(1500);

/// Where a work stands, as its agent last said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkStatus {
    Working,
    Done,
    /// It failed, for the reason its agent gave.
    Failed(String),
}

impl WorkStatus {
    /// The status `canvas_progress` names (`working`, `done` or `failed`), with the `message` a
    /// failure gives as its reason. A failure with no message is one "for no reason given".
    pub fn parse(status: &str, message: Option<&str>) -> Result<Self, String> {
        match status.trim().to_ascii_lowercase().as_str() {
            "working" => Ok(Self::Working),
            "done" => Ok(Self::Done),
            "failed" => {
                let reason = message.map(str::trim).filter(|m| !m.is_empty());
                Ok(Self::Failed(
                    reason.unwrap_or("no reason given").to_string(),
                ))
            }
            other => Err(format!(
                "status “{other}” isn't one: use working, done or failed"
            )),
        }
    }

    /// Its name on the wire: `working`, `done` or `failed`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Done => "done",
            Self::Failed(_) => "failed",
        }
    }
}

/// Whether `kind` (`canvas_begin`'s, `component` when none is given) is a kind of work the canvas
/// builds; the error says which there are.
pub fn check_kind(kind: Option<&str>) -> Result<&'static str, String> {
    match kind.map(str::trim).filter(|k| !k.is_empty()) {
        None => Ok(WORK_KINDS[0]),
        Some(kind) => WORK_KINDS
            .into_iter()
            .find(|known| kind.eq_ignore_ascii_case(known))
            .ok_or_else(|| {
                format!(
                    "kind “{kind}” isn't one: the canvas builds \"component\" work (drawn with \
                     the canvas tools) and \"screen\" work (an HTML screen)"
                )
            }),
    }
}

/// A new work's id: `w_` and eight hex digits from `seed` (the time, the process, a count), so
/// that two agents' works never share one in practice.
pub fn new_work_id(seed: u128) -> String {
    // FNV-1a over the seed's bytes: spread, so that close seeds make far ids.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("w_{:08x}", (hash ^ (hash >> 32)) as u32)
}

/// How a work's row looks `since` its status last changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowLook {
    /// Running: a spinner, its step; `stalled` once [`STALL_AFTER`] has gone by with nothing from
    /// its agent.
    Working { stalled: bool },
    /// Done: a check in the agent's colour; the agent's name for [`NAME_AFTER`], then the check
    /// alone, fading (`check` from 1 to 0) over [`CHECK_FADE`].
    Done { name: bool, check: f32 },
    /// Failed: "!", and the reason, until its agent retries or the user dismisses it.
    Failed,
    /// Nothing shows any more.
    Gone,
}

/// How the row of a work in `status` looks `since` that status was given, and `quiet` since its
/// agent last did anything for it.
pub fn row_look(status: &WorkStatus, since: Duration, quiet: Duration) -> RowLook {
    match status {
        WorkStatus::Working => RowLook::Working {
            stalled: quiet >= STALL_AFTER,
        },
        WorkStatus::Failed(_) => RowLook::Failed,
        WorkStatus::Done if since < NAME_AFTER => RowLook::Done {
            name: true,
            check: 1.0,
        },
        WorkStatus::Done if since < NAME_AFTER + CHECK_FADE => {
            let into = (since - NAME_AFTER).as_secs_f32() / CHECK_FADE.as_secs_f32();
            RowLook::Done {
                name: false,
                check: (1.0 - into).clamp(0.0, 1.0),
            }
        }
        WorkStatus::Done => RowLook::Gone,
    }
}

/// What the app announces, politely, when a work changes state.
pub fn started(subject: &str, work: &str) -> String {
    format!("{subject} started {work}")
}

pub fn finished(subject: &str, work: &str) -> String {
    format!("{subject} finished {work}")
}

/// "Gemini CLI couldn't finish Testimonial: couldn't load the avatar image", the reason as the
/// end of the sentence.
pub fn failed(subject: &str, work: &str, reason: &str) -> String {
    let mut letters = reason.trim().trim_end_matches('.').chars();
    let reason: String = match letters.next() {
        // "Couldn't …" reads as "couldn't …" after the colon; an "API …" stays as it is.
        Some(first) if letters.clone().next().is_some_and(char::is_lowercase) => {
            first.to_lowercase().chain(letters).collect()
        }
        Some(first) => std::iter::once(first).chain(letters).collect(),
        None => String::new(),
    };
    match reason.is_empty() {
        true => format!("{subject} couldn't finish {work}"),
        false => format!("{subject} couldn't finish {work}: {reason}"),
    }
}

pub fn stalled(subject: &str, work: &str) -> String {
    format!("{subject} stalled on {work}")
}

/// How many of the canvas's works and agents are in each state, for the roster.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub building: usize,
    pub stalled: usize,
    pub done: usize,
    pub failed: usize,
    /// Agents at work outside any work item: their ops land as they come.
    pub active: usize,
    /// Agents that have done nothing for a while: collapsed into "+N".
    pub idle: usize,
}

impl Tally {
    /// The roster's words: "3 building · 1 done · 1 failed", what is zero left out; "2 agents
    /// at work" when none of them builds a component; empty when nothing is on.
    pub fn words(&self) -> String {
        let parts = [
            (self.building, "building"),
            (self.stalled, "stalled"),
            (self.done, "done"),
            (self.failed, "failed"),
        ];
        let said: Vec<String> = (parts.iter())
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| format!("{n} {what}"))
            .collect();
        match (said.is_empty(), self.active) {
            (false, _) => said.join(" · "),
            (true, 0) => String::new(),
            (true, 1) => "1 agent at work".into(),
            (true, n) => format!("{n} agents at work"),
        }
    }
}
