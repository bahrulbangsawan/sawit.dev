//! The polite status line's sentences: one per change ("Codex finished Sign-in form"), queued per
//! agent and said in turn, each held for [`HOLD`] before the next replaces it, so that agents at
//! work at once don't talk over each other. An agent's ops outside a component are said at most
//! once per [`ANNOUNCE_GAP`]: a screen reader would otherwise read every op of a busy agent.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui_kit::SharedString;

use super::ANNOUNCE_GAP;

/// How long a sentence stays in the status line before the next one takes its place.
pub(super) const HOLD: Duration = Duration::from_millis(1500);
/// How many sentences an agent may have waiting: past it, its oldest is dropped.
const MAX_WAITING: usize = 4;

#[derive(Default)]
pub(in crate::board) struct Announcer {
    /// Each agent's sentences waiting, by the agent's id, in the order the agents first spoke.
    waiting: Vec<(SharedString, VecDeque<SharedString>)>,
    /// What the status line says, and since when.
    pub current: SharedString,
    since: Option<Instant>,
    /// The agent said last: the agents after it have their turn first.
    last: Option<SharedString>,
    /// When each agent's last op outside a component was said.
    ops: Vec<(SharedString, Instant)>,
}

impl Announcer {
    /// Queues `sentence` about agent `agent`, and says it at once when nothing is being said.
    pub fn say(&mut self, agent: &SharedString, sentence: SharedString, now: Instant) {
        let ix = match self.waiting.iter().position(|(id, _)| id == agent) {
            Some(ix) => ix,
            None => {
                self.waiting.push((agent.clone(), VecDeque::new()));
                self.waiting.len() - 1
            }
        };
        let queue = &mut self.waiting[ix].1;
        if queue.len() >= MAX_WAITING {
            queue.pop_front();
        }
        queue.push_back(sentence);
        self.tick(now);
    }

    /// Queues what agent `agent` did outside a component, unless it was said less than
    /// [`ANNOUNCE_GAP`] ago.
    pub fn op(&mut self, agent: &SharedString, sentence: SharedString, now: Instant) {
        self.ops
            .retain(|(_, at)| now.saturating_duration_since(*at) < ANNOUNCE_GAP);
        if self.ops.iter().any(|(id, _)| id == agent) {
            return;
        }
        self.ops.push((agent.clone(), now));
        self.say(agent, sentence, now);
    }

    /// Moves on to the next agent's sentence once the one said has been held for [`HOLD`].
    /// Whether the status line changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let held = self
            .since
            .is_none_or(|since| now.saturating_duration_since(since) >= HOLD);
        if !held || self.waiting.iter().all(|(_, q)| q.is_empty()) {
            return false;
        }
        let n = self.waiting.len();
        let after = (self.last.as_ref())
            .and_then(|last| self.waiting.iter().position(|(id, _)| id == last))
            .map_or(0, |ix| ix + 1);
        for step in 0..n {
            let ix = (after + step) % n;
            if let Some(sentence) = self.waiting[ix].1.pop_front() {
                self.current = sentence;
                self.since = Some(now);
                self.last = Some(self.waiting[ix].0.clone());
                return true;
            }
        }
        false
    }

    /// Whether sentences wait to be said.
    pub fn pending(&self) -> bool {
        self.waiting.iter().any(|(_, q)| !q.is_empty())
    }
}
