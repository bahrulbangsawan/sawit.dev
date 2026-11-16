//! Test support: a scripted in-process provider (each turn runs a closure the test gives, with
//! the turn's text and context, so it can call the communication tools, block, fail or wait for
//! cancellation), and a runtime over a store in a temporary folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::future::BoxFuture;

use crate::orchestra::provider::*;
use crate::orchestra::store::Store;
use crate::orchestra::{
    AgentId, Limits, NewAgent, Orchestra, OrchestraConfig, PermissionMode, TaskId,
};

/// One turn, as a script sees it.
pub struct Turn {
    pub agent: AgentId,
    pub text: String,
    pub ctx: TurnContext,
}

pub type Script =
    Arc<dyn Fn(Turn) -> BoxFuture<'static, Result<TurnOutcome, ProviderError>> + Send + Sync>;

/// A script that answers every turn with `done: <task title line>` at once.
pub fn echo() -> Script {
    Arc::new(|t: Turn| {
        Box::pin(async move {
            Ok(done(format!(
                "done: {}",
                t.text
                    .lines()
                    .find(|l| l.starts_with("Task "))
                    .unwrap_or("")
            )))
        })
    })
}

pub fn done(text: impl Into<String>) -> TurnOutcome {
    TurnOutcome {
        stop: StopReason::EndTurn,
        text: text.into(),
    }
}

#[derive(Default)]
pub struct Counters {
    pub starts: AtomicUsize,
    pub turns: AtomicUsize,
    pub active: AtomicUsize,
    pub max_active: AtomicUsize,
    /// Starts to fail before one succeeds.
    pub fail_starts: AtomicUsize,
    /// The order turns began in: (agent, task).
    pub order: Mutex<Vec<(AgentId, TaskId)>>,
    pub per_task: Mutex<HashMap<TaskId, usize>>,
}

pub struct Scripted {
    pub caps: ProviderCapabilities,
    pub script: Script,
    pub counters: Arc<Counters>,
}

impl Scripted {
    pub fn new(concurrent: bool, script: Script) -> Arc<Self> {
        Arc::new(Self {
            caps: ProviderCapabilities {
                concurrent_turns: concurrent,
                streaming: true,
                cancel: true,
                tool_calls: true,
                ..Default::default()
            },
            script,
            counters: Arc::default(),
        })
    }
}

#[async_trait]
impl AgentProvider for Scripted {
    fn id(&self) -> &str {
        "mock"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.caps.clone()
    }

    async fn start(
        &self,
        _request: StartRequest,
    ) -> Result<Arc<dyn ProviderRuntime>, ProviderError> {
        let c = &self.counters;
        if c.fail_starts.load(Ordering::SeqCst) > 0 {
            c.fail_starts.fetch_sub(1, Ordering::SeqCst);
            return Err(ProviderError::Start("scripted start failure".into()));
        }
        c.starts.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(ScriptedRuntime {
            caps: self.caps.clone(),
            script: self.script.clone(),
            counters: self.counters.clone(),
            alive: AtomicBool::new(true),
            next: AtomicUsize::new(1),
        }))
    }
}

struct ScriptedRuntime {
    caps: ProviderCapabilities,
    script: Script,
    counters: Arc<Counters>,
    alive: AtomicBool,
    next: AtomicUsize,
}

#[async_trait]
impl ProviderRuntime for ScriptedRuntime {
    fn capabilities(&self) -> ProviderCapabilities {
        self.caps.clone()
    }

    async fn open_session(
        &self,
        request: SessionRequest,
    ) -> Result<ProviderSession, ProviderError> {
        Ok(ProviderSession {
            id: format!(
                "s-{}-{}",
                request.thread,
                self.next.fetch_add(1, Ordering::SeqCst)
            ),
            restored: false,
        })
    }

    async fn prompt(
        &self,
        _session: &ProviderSession,
        text: String,
        ctx: TurnContext,
    ) -> Result<TurnOutcome, ProviderError> {
        let c = &self.counters;
        let now = c.active.fetch_add(1, Ordering::SeqCst) + 1;
        c.max_active.fetch_max(now, Ordering::SeqCst);
        c.turns.fetch_add(1, Ordering::SeqCst);
        c.order
            .lock()
            .unwrap()
            .push((ctx.agent.clone(), ctx.task.clone()));
        *c.per_task
            .lock()
            .unwrap()
            .entry(ctx.task.clone())
            .or_default() += 1;
        let out = (self.script)(Turn {
            agent: ctx.agent.clone(),
            text,
            ctx,
        })
        .await;
        c.active.fetch_sub(1, Ordering::SeqCst);
        out
    }

    async fn cancel(&self, _session: &ProviderSession) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn close_session(&self, _session: ProviderSession) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn shutdown(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}

/// A runtime over the store at `dir/orchestra.sqlite` with the scripted provider, started.
pub fn runtime(dir: &Path, provider: Arc<Scripted>, limits: Limits) -> Orchestra {
    let store = Store::open(&dir.join("orchestra.sqlite")).expect("store opens");
    let mut config = OrchestraConfig::for_data_dir(dir);
    config.limits = limits;
    config.poll_interval = Duration::from_millis(25);
    let o = Orchestra::new(store, ProviderRegistry::default().with(provider), config);
    o.start(&tokio::runtime::Handle::current());
    o
}

/// A new agent on the scripted provider, working in its own folder under `dir` (the workspace).
pub fn agent(name: &str, dir: &Path) -> NewAgent {
    let workdir = dir.join(name);
    std::fs::create_dir_all(&workdir).expect("agent folder");
    NewAgent {
        name: name.into(),
        role: format!("{name} role"),
        provider: Some("mock".into()),
        workdir,
        workspace: Some(dir.to_path_buf()),
        permission: Some(PermissionMode::Allow),
        ..Default::default()
    }
}

/// A temporary folder that stays for the test.
pub fn temp() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = std::fs::canonicalize(dir.path()).expect("canonical");
    (dir, path)
}

/// Waits until `f` holds, or panics after 10 s.
pub async fn until(what: &str, mut f: impl AsyncFnMut() -> bool) {
    for _ in 0..1000 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}
