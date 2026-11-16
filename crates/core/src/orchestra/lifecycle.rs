//! Each agent's runtime as it is now: its lifecycle state, its live provider runtime and the
//! provider sessions open on it, its running turns and when it was last used. State moves only
//! along [`AgentState::can_go`], checked and made under one lock, so two requests racing (a
//! start and a stop) can't leave an agent in a state neither meant. The lock is a plain mutex,
//! never held across an `.await`; starting and stopping one agent are serialized by its own
//! async lock instead ([`Lifecycle::start_lock`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::ids::{AgentId, ThreadId};
use super::model::AgentState;
use super::provider::{ProviderRuntime, ProviderSession};
use super::{OrchestraError, Result};

struct Slot {
    state: AgentState,
    detail: Option<String>,
    runtime: Option<Arc<dyn ProviderRuntime>>,
    sessions: HashMap<ThreadId, ProviderSession>,
    turns: usize,
    last_used: Instant,
    /// After a failed start, no new start before this.
    retry_after: Option<Instant>,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            state: AgentState::Registered,
            detail: None,
            runtime: None,
            sessions: HashMap::new(),
            turns: 0,
            last_used: Instant::now(),
            retry_after: None,
        }
    }
}

#[derive(Default)]
pub struct Lifecycle {
    slots: Mutex<HashMap<AgentId, Slot>>,
    starts: Mutex<HashMap<AgentId, Arc<tokio::sync::Mutex<()>>>>,
}

impl Lifecycle {
    fn slots(&self) -> MutexGuard<'_, HashMap<AgentId, Slot>> {
        self.slots.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn state(&self, id: &AgentId) -> AgentState {
        self.slots()
            .get(id)
            .map_or(AgentState::Registered, |s| s.state)
    }

    pub fn detail(&self, id: &AgentId) -> Option<String> {
        self.slots().get(id).and_then(|s| s.detail.clone())
    }

    /// Moves the agent to `to` if the lifecycle allows it from where it is; returns the state it
    /// left.
    pub fn transition(
        &self,
        id: &AgentId,
        to: AgentState,
        detail: Option<String>,
    ) -> Result<AgentState> {
        let mut slots = self.slots();
        let slot = slots.entry(id.clone()).or_default();
        let from = slot.state;
        if from == to {
            return Ok(from);
        }
        if !from.can_go(to) {
            return Err(OrchestraError::Conflict(format!(
                "agent {id} is {}; it can't become {}",
                from.name(),
                to.name()
            )));
        }
        slot.state = to;
        slot.detail = detail;
        Ok(from)
    }

    /// The lock that serializes starting and stopping this agent. Held across the provider's
    /// start, which is the point: a second start waits for the first and then finds it running.
    pub fn start_lock(&self, id: &AgentId) -> Arc<tokio::sync::Mutex<()>> {
        self.starts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(id.clone())
            .or_default()
            .clone()
    }

    /// The agent's runtime, if it has one that still lives.
    pub fn live_runtime(&self, id: &AgentId) -> Option<Arc<dyn ProviderRuntime>> {
        self.slots()
            .get(id)
            .and_then(|s| s.runtime.clone())
            .filter(|r| r.is_alive())
    }

    /// The agent's runtime, alive or not (to shut down).
    pub fn runtime(&self, id: &AgentId) -> Option<Arc<dyn ProviderRuntime>> {
        self.slots().get(id).and_then(|s| s.runtime.clone())
    }

    /// Installs a started runtime (its sessions start empty).
    pub fn set_runtime(&self, id: &AgentId, runtime: Option<Arc<dyn ProviderRuntime>>) {
        let mut slots = self.slots();
        let slot = slots.entry(id.clone()).or_default();
        slot.runtime = runtime;
        slot.sessions.clear();
        slot.last_used = Instant::now();
    }

    pub fn session(&self, id: &AgentId, thread: &ThreadId) -> Option<ProviderSession> {
        self.slots()
            .get(id)
            .and_then(|s| s.sessions.get(thread).cloned())
    }

    pub fn set_session(&self, id: &AgentId, thread: &ThreadId, session: Option<ProviderSession>) {
        let mut slots = self.slots();
        let slot = slots.entry(id.clone()).or_default();
        match session {
            Some(s) => slot.sessions.insert(thread.clone(), s),
            None => slot.sessions.remove(thread),
        };
    }

    /// A turn begins: the agent is busy.
    pub fn begin_turn(&self, id: &AgentId) {
        let mut slots = self.slots();
        let slot = slots.entry(id.clone()).or_default();
        slot.turns += 1;
        if slot.state == AgentState::Ready {
            slot.state = AgentState::Busy;
        }
    }

    /// A turn ended: the agent is ready when it was its last.
    pub fn end_turn(&self, id: &AgentId) {
        let mut slots = self.slots();
        let slot = slots.entry(id.clone()).or_default();
        slot.turns = slot.turns.saturating_sub(1);
        slot.last_used = Instant::now();
        if slot.turns == 0 && slot.state == AgentState::Busy {
            slot.state = AgentState::Ready;
        }
    }

    pub fn turns(&self, id: &AgentId) -> usize {
        self.slots().get(id).map_or(0, |s| s.turns)
    }

    pub fn set_retry_after(&self, id: &AgentId, wait: Option<Duration>) {
        let mut slots = self.slots();
        slots.entry(id.clone()).or_default().retry_after = wait.map(|w| Instant::now() + w);
    }

    /// Whether a start may be tried now (no failed start is cooling down).
    pub fn may_start(&self, id: &AgentId) -> bool {
        self.slots()
            .get(id)
            .and_then(|s| s.retry_after)
            .is_none_or(|t| Instant::now() >= t)
    }

    /// Agents with a runtime, idle (no turn) for longer than `idle`, least recently used first.
    pub fn idle(&self, idle: Duration) -> Vec<AgentId> {
        let slots = self.slots();
        let mut out: Vec<(&AgentId, Instant)> = slots
            .iter()
            .filter(|(_, s)| s.runtime.is_some() && s.turns == 0 && s.last_used.elapsed() >= idle)
            .map(|(id, s)| (id, s.last_used))
            .collect();
        out.sort_by_key(|(_, t)| *t);
        out.into_iter().map(|(id, _)| id.clone()).collect()
    }

    /// Agents whose runtime is alive.
    pub fn live_count(&self) -> usize {
        self.slots()
            .values()
            .filter(|s| s.runtime.as_ref().is_some_and(|r| r.is_alive()))
            .count()
    }

    /// Every agent with a runtime.
    pub fn with_runtime(&self) -> Vec<AgentId> {
        self.slots()
            .iter()
            .filter(|(_, s)| s.runtime.is_some())
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn forget(&self, id: &AgentId) {
        self.slots().remove(id);
    }
}
