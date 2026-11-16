//! The agent runtime (`sawit_core::orchestra`) in the app. It starts once at launch on the app's
//! Tokio runtime: its scheduler takes the store's lease, recovers what the last run left, and runs
//! what agents and `sawit-mcp` queue. ACP agents get the communication tools through this binary
//! run as `sawit-mcp`. Nothing here draws: the runtime's events land in [`Feed`], a GPUI model
//! any view can observe (`cx.observe(&feed, …)`), on the main thread; the runtime itself never
//! touches GPUI state and does its I/O off the main thread. A feed that fell behind the event
//! stream is told to reload from the store ([`Feed::stale`]), which stays the source of truth.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use gpui_kit::*;
use sawit_core::orchestra::acp::AcpProvider;
use sawit_core::orchestra::provider::ProviderRegistry;
use sawit_core::orchestra::{
    AgentId, AgentState, Launch, Orchestra, OrchestraConfig, RuntimeEvent, store,
};
use tokio::sync::broadcast::error::RecvError;

/// Events the feed keeps, newest last.
const RECENT: usize = 200;

/// The runtime, for code that acts on it (submitting, cancelling, starting agents). No view reads
/// it yet: the runtime is driven through `sawit-mcp` for now.
#[allow(dead_code)]
pub struct AgentRuntime {
    pub orchestra: Arc<Orchestra>,
    pub feed: Entity<Feed>,
}

impl Global for AgentRuntime {}

/// What the runtime did lately, for views.
#[derive(Default)]
pub struct Feed {
    pub recent: VecDeque<RuntimeEvent>,
    /// Each agent's runtime state as last reported.
    pub states: HashMap<AgentId, AgentState>,
    /// Events were dropped: reload what is shown from the store.
    pub stale: bool,
}

impl Feed {
    fn apply(&mut self, event: Option<RuntimeEvent>) {
        let Some(event) = event else {
            self.stale = true;
            return;
        };
        if let RuntimeEvent::AgentStatusChanged { agent, state, .. } = &event {
            self.states.insert(agent.clone(), *state);
        }
        if let RuntimeEvent::AgentDeleted { agent } = &event {
            self.states.remove(agent);
        }
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(event);
    }
}

/// `sawit-mcp` beside this binary (the bundle links it there; a dev build builds it there).
fn mcp_bridge() -> Option<Launch> {
    let exe = std::env::current_exe().ok()?;
    let mcp: PathBuf = exe.with_file_name("sawit-mcp");
    if !mcp.is_file() {
        log::warn!(
            "agent runtime: no sawit-mcp beside {}; agents run without the communication tools",
            exe.display()
        );
        return None;
    }
    let mut env = std::collections::BTreeMap::new();
    if let Some(home) = std::env::var_os("SAWIT_HOME") {
        env.insert("SAWIT_HOME".into(), home.to_string_lossy().into_owned());
    }
    Some(Launch {
        command: mcp.to_string_lossy().into_owned(),
        args: Vec::new(),
        env,
    })
}

/// Starts the runtime. Call after the Tokio runtime global is set.
pub fn init(cx: &mut App) {
    let root = sawit_core::instances::AppPaths::default_macos().root;
    let store = match store::Store::open(&store::default_path(&root)) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("agent runtime: the store can't be opened, so the runtime is off: {e}");
            return;
        }
    };
    let mut config = OrchestraConfig::for_data_dir(&root);
    config.mcp_bridge = mcp_bridge();
    let providers = ProviderRegistry::default().with(Arc::new(AcpProvider));
    let orchestra = Arc::new(Orchestra::new(store, providers, config));
    let runtime = cx.global::<crate::rt::Rt>().0.clone();
    orchestra.start(runtime.handle());
    let mut events = orchestra.subscribe();
    let mut stream = runtime.spawn_stream(move |tx| {
        Box::pin(async move {
            loop {
                let item = match events.recv().await {
                    Ok(e) => Some(e),
                    Err(RecvError::Lagged(_)) => None,
                    Err(RecvError::Closed) => break,
                };
                if tx.unbounded_send(item).is_err() {
                    break;
                }
            }
        })
    });
    let quitting = orchestra.clone();
    let feed = cx.new(|cx| {
        cx.on_app_quit(move |_: &mut Feed, _| {
            // Running turns are recorded as interrupted (or requeued when that is safe) and the
            // agents' processes stopped; whatever the quit doesn't wait for, recovery handles.
            let done = runtime.spawn({
                let o = quitting.clone();
                async move { o.shutdown().await }
            });
            async move {
                let _ = done.await;
            }
        })
        .detach();
        Feed::default()
    });
    let weak = feed.downgrade();
    cx.spawn(async move |cx| {
        while let Some(event) = stream.next().await {
            if weak
                .update(cx, |f, cx| {
                    f.apply(event);
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
    cx.set_global(AgentRuntime { orchestra, feed });
}
