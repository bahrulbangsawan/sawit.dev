//! [`Orchestra`]: the runtime the app holds. It owns the scheduler, the agents' provider
//! runtimes (started lazily, when an agent first gets work, and stopped when idle) and the
//! event stream; everything durable goes through the store. It runs on any Tokio runtime and
//! never touches GPUI, so tests drive it directly.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{Notify, Semaphore, broadcast};
use tokio_util::sync::CancellationToken;

use super::comms::Comms;
use super::events::RuntimeEvent;
use super::exec;
use super::ids::{AgentId, TaskId, ThreadId, random_hex};
use super::lifecycle::Lifecycle;
use super::model::{AgentProfile, AgentState, Launch, Task};
use super::provider::{
    McpServer, PermissionGate, ProviderRegistry, ProviderRuntime, RefuseAll, StartRequest,
};
use super::store::{Store, agents};
use super::workspace::FolderLocks;
use super::{Limits, OrchestraError, Result};

mod api;

pub use api::AgentStatus;

/// How long a failed provider start waits before it is tried again.
const START_COOLDOWN: Duration = Duration::from_secs(10);

/// The runtime's settings.
#[derive(Clone)]
pub struct OrchestraConfig {
    pub limits: Limits,
    /// How often the scheduler looks for work other processes stored.
    pub poll_interval: Duration,
    /// How to start `sawit-mcp`, which gives ACP agents the communication tools. `None` leaves
    /// them without.
    pub mcp_bridge: Option<Launch>,
    /// Where isolated threads' worktrees go.
    pub worktrees_dir: PathBuf,
    /// Who answers permission requests in `ask` mode.
    pub gate: Arc<dyn PermissionGate>,
    /// This process's name on the scheduler lease and on claims.
    pub owner: String,
}

impl OrchestraConfig {
    /// The settings for Sawit's data folder `root`.
    pub fn for_data_dir(root: &std::path::Path) -> Self {
        Self {
            limits: Limits::default(),
            poll_interval: Duration::from_secs(1),
            mcp_bridge: None,
            worktrees_dir: root.join("orchestra").join("worktrees"),
            gate: Arc::new(RefuseAll),
            owner: format!("sawit-{}-{}", std::process::id(), random_hex(4)),
        }
    }
}

pub(crate) struct Running {
    agent: AgentId,
    cancel: CancellationToken,
}

/// What the runtime's tasks share.
pub(crate) struct Inner {
    pub store: Store,
    pub providers: ProviderRegistry,
    pub config: OrchestraConfig,
    pub lifecycle: Lifecycle,
    pub events: broadcast::Sender<RuntimeEvent>,
    pub wake: Arc<Notify>,
    pub running: Mutex<HashMap<TaskId, Running>>,
    pub permits: Arc<Semaphore>,
    pub folders: FolderLocks,
    pub shutdown: CancellationToken,
    /// The agent the scheduler gave work to last, for round-robin fairness.
    pub last_served: Mutex<Option<AgentId>>,
}

impl Inner {
    pub fn publish(&self, events: Vec<RuntimeEvent>) {
        for e in events {
            // No listener is fine: the store has it all.
            let _ = self.events.send(e);
        }
    }

    pub fn comms(&self) -> Comms {
        let wake = self.wake.clone();
        let events = self.events.clone();
        Comms::new(self.store.clone(), self.config.limits.clone()).with_hooks(
            Arc::new(move || wake.notify_one()),
            Arc::new(move |list| {
                for e in list {
                    let _ = events.send(e);
                }
            }),
        )
    }

    fn running(&self) -> std::sync::MutexGuard<'_, HashMap<TaskId, Running>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Registers a claimed task as running; its token cancels it.
    pub fn track(&self, task: &Task) -> CancellationToken {
        let cancel = CancellationToken::new();
        self.running().insert(
            task.id.clone(),
            Running {
                agent: task.agent_id.clone(),
                cancel: cancel.clone(),
            },
        );
        cancel
    }

    pub fn running_for(&self, agent: &AgentId) -> usize {
        self.running()
            .values()
            .filter(|r| &r.agent == agent)
            .count()
    }

    /// Asks a running task to stop.
    pub fn signal(&self, task: &TaskId) -> bool {
        self.running()
            .get(task)
            .map(|r| r.cancel.cancel())
            .is_some()
    }

    pub fn finished(&self, task: &TaskId) {
        self.running().remove(task);
        self.wake.notify_one();
    }

    pub fn bridge(&self, agent: &AgentId, thread: &ThreadId, token: &str) -> Vec<McpServer> {
        self.config
            .mcp_bridge
            .iter()
            .map(|l| exec::bridge_server(l, agent, thread, token))
            .collect()
    }

    fn set_state(&self, id: &AgentId, state: AgentState, detail: Option<String>) {
        let (store, id2, d) = (self.store.clone(), id.clone(), detail.clone());
        // Recorded for other processes, off the async workers; a failure to record never stops
        // the runtime.
        tokio::task::spawn_blocking(move || {
            store.with(|c| agents::record_state(c, &id2, state, d.as_deref()))
        });
        self.publish(vec![RuntimeEvent::AgentStatusChanged {
            agent: id.clone(),
            state,
            detail,
        }]);
    }

    fn transition(&self, id: &AgentId, to: AgentState, detail: Option<String>) -> Result<()> {
        self.lifecycle.transition(id, to, detail.clone())?;
        self.set_state(id, to, detail);
        Ok(())
    }

    /// The agent's provider runtime, started if it isn't running.
    pub async fn ensure_started(&self, profile: &AgentProfile) -> Result<Arc<dyn ProviderRuntime>> {
        let id = &profile.id;
        if let Some(rt) = self.lifecycle.live_runtime(id) {
            return Ok(rt);
        }
        let lock = self.lifecycle.start_lock(id);
        let _one_start = lock.lock().await;
        if let Some(rt) = self.lifecycle.live_runtime(id) {
            return Ok(rt);
        }
        if let Some(dead) = self.lifecycle.runtime(id) {
            // It died since; make sure nothing of it is left.
            dead.shutdown().await;
            self.lifecycle.set_runtime(id, None);
            if self.lifecycle.state(id).is_running() {
                let _ = self.transition(id, AgentState::Failed, Some("its provider exited".into()));
            }
        }
        let provider = self.providers.get(&profile.provider).ok_or_else(|| {
            OrchestraError::Provider(format!("no provider `{}`", profile.provider))
        })?;
        self.transition(id, AgentState::Starting, None)?;
        match provider
            .start(StartRequest {
                profile: profile.clone(),
            })
            .await
        {
            Ok(rt) => {
                if self.lifecycle.state(id) != AgentState::Starting {
                    // Stopped while it started.
                    rt.shutdown().await;
                    return Err(OrchestraError::Conflict(format!(
                        "agent {id} was stopped while it started"
                    )));
                }
                self.lifecycle.set_runtime(id, Some(rt.clone()));
                self.lifecycle.set_retry_after(id, None);
                self.transition(id, AgentState::Ready, None)?;
                self.publish(vec![RuntimeEvent::ProviderConnected {
                    agent: id.clone(),
                    provider: profile.provider.clone(),
                }]);
                Ok(rt)
            }
            Err(e) => {
                self.lifecycle.set_retry_after(id, Some(START_COOLDOWN));
                let _ = self.transition(id, AgentState::Failed, Some(e.to_string()));
                Err(OrchestraError::Provider(e.to_string()))
            }
        }
    }

    /// Stops the agent's provider runtime; the agent stays registered.
    pub async fn stop_runtime(&self, id: &AgentId, reason: &str) {
        let lock = self.lifecycle.start_lock(id);
        let _one = lock.lock().await;
        let Some(rt) = self.lifecycle.runtime(id) else {
            return;
        };
        let _ = self.transition(id, AgentState::Stopping, Some(reason.into()));
        rt.shutdown().await;
        self.lifecycle.set_runtime(id, None);
        let _ = self.transition(id, AgentState::Stopped, Some(reason.into()));
        self.publish(vec![RuntimeEvent::ProviderDisconnected {
            agent: id.clone(),
            provider: String::new(),
            reason: reason.into(),
        }]);
    }

    /// The agent's provider went away mid-work: its sessions are gone with it.
    pub fn runtime_died(&self, id: &AgentId, provider: &str, why: &str) {
        self.lifecycle.set_runtime(id, None);
        if self.lifecycle.state(id).is_running() {
            let _ = self.transition(id, AgentState::Failed, Some(why.into()));
        }
        self.publish(vec![RuntimeEvent::ProviderDisconnected {
            agent: id.clone(),
            provider: provider.into(),
            reason: why.into(),
        }]);
    }
}

/// The agent runtime.
pub struct Orchestra {
    pub(crate) inner: Arc<Inner>,
    scheduler: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Orchestra {
    /// A runtime over `store`, with the providers in `providers`. Nothing runs until
    /// [`Orchestra::start`].
    pub fn new(store: Store, providers: ProviderRegistry, config: OrchestraConfig) -> Self {
        let (events, _) = broadcast::channel(1024);
        let max_running = config.limits.max_running.max(1);
        Self {
            inner: Arc::new(Inner {
                store,
                providers,
                config,
                lifecycle: Lifecycle::default(),
                events,
                wake: Arc::new(Notify::new()),
                running: Mutex::new(HashMap::new()),
                permits: Arc::new(Semaphore::new(max_running)),
                folders: FolderLocks::default(),
                shutdown: CancellationToken::new(),
                last_served: Mutex::new(None),
            }),
            scheduler: Mutex::new(None),
        }
    }

    /// Starts the scheduler on `handle`'s runtime.
    pub fn start(&self, handle: &tokio::runtime::Handle) {
        let mut slot = self.scheduler.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            *slot = Some(handle.spawn(super::scheduler::run(self.inner.clone())));
        }
    }

    /// The runtime's events from now on. A receiver that lags loses the oldest and should reload
    /// from the store.
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.inner.events.subscribe()
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// The communication tools, as their events and wake-ups reach this runtime.
    pub fn comms(&self) -> Comms {
        self.inner.comms()
    }

    /// Wakes the scheduler.
    pub fn wake(&self) {
        self.inner.wake.notify_one();
    }

    /// Stops scheduling, ends every running turn (each is held for review or requeued, as its
    /// side effects allow), stops every provider process and gives up the lease.
    pub async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        let handle = self
            .scheduler
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(h) = handle {
            let _ = h.await;
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !self.inner.running().is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        for id in self.inner.lifecycle.with_runtime() {
            self.inner.stop_runtime(&id, "Sawit is quitting").await;
        }
    }
}
