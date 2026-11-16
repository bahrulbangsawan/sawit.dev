//! The provider-independent interface the runtime drives agents through. A [`AgentProvider`] is
//! an adapter kind (ACP, or a native one); [`AgentProvider::start`] makes one agent's
//! [`ProviderRuntime`]: its process and connection, which holds the provider sessions of the
//! agent's threads. The orchestrator knows no provider by name: it asks each runtime what it can
//! do ([`ProviderCapabilities`]) and schedules within that.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::ids::{AgentId, TaskId, ThreadId};
use super::model::{AgentProfile, PermissionMode};

/// What a provider runtime can do, as negotiated with it. Only what an adapter really supports
/// is set; the scheduler and executor never use what isn't.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// A session can be resumed from its id after the process restarts.
    pub load_session: bool,
    /// Turns of different sessions may run at the same time on one runtime. When false the
    /// scheduler never sends overlapping turns to it.
    pub concurrent_turns: bool,
    /// The runtime streams its output while a turn runs.
    pub streaming: bool,
    /// A running turn can be cancelled.
    pub cancel: bool,
    /// It calls tools and reports them.
    pub tool_calls: bool,
    /// It asks before tool use, and takes the answer.
    pub permission_requests: bool,
    /// It connects MCP servers given at session start (stdio).
    pub mcp_stdio: bool,
    /// The agent's model can be chosen.
    pub model_selection: bool,
    /// Images may be sent in prompts.
    pub image_prompts: bool,
}

/// An MCP server a session should connect (the bridge that gives an agent its communication
/// tools).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// A session to open on a runtime for a thread.
#[derive(Clone, Debug)]
pub struct SessionRequest {
    pub thread: ThreadId,
    pub cwd: PathBuf,
    /// The provider session the thread had, to restore when the runtime can.
    pub restore: Option<String>,
    pub mcp_servers: Vec<McpServer>,
}

/// An open provider session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSession {
    pub id: String,
    /// It was restored rather than made new: the instructions are already in it.
    pub restored: bool,
}

/// What a runtime streams while a turn runs.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderEvent {
    /// A piece of the agent's answer.
    Message(String),
    /// A piece of its reasoning.
    Thought(String),
    ToolCall {
        id: String,
        title: String,
        kind: String,
        status: String,
    },
    ToolCallUpdate {
        id: String,
        status: Option<String>,
    },
    Plan(serde_json::Value),
    /// Anything else it said, as sent.
    Other(serde_json::Value),
}

/// Why a turn stopped, as the provider said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    Other(String),
}

/// A turn that ran to its end.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnOutcome {
    pub stop: StopReason,
    /// The agent's answer, every message piece joined.
    pub text: String,
}

/// A request the provider makes for permission to use a tool.
#[derive(Clone, Debug, PartialEq)]
pub struct PermissionRequest {
    pub agent: AgentId,
    pub task: TaskId,
    pub tool_title: String,
    /// The ACP tool kind (`read`, `edit`, `execute`, …), `other` when not given.
    pub tool_kind: String,
    /// The options offered: `(option id, kind)`, kinds as ACP names them (`allow_once`, …).
    pub options: Vec<(String, String)>,
    pub raw: serde_json::Value,
}

/// The answer to a [`PermissionRequest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Reject,
    /// The turn is being cancelled.
    Cancelled,
}

/// Who answers permission requests in [`PermissionMode::Ask`]: the IDE, which can show them.
pub trait PermissionGate: Send + Sync {
    fn ask(&self, request: PermissionRequest) -> BoxFuture<'static, PermissionDecision>;
}

/// The gate when nobody can answer: refuses.
pub struct RefuseAll;

impl PermissionGate for RefuseAll {
    fn ask(&self, _request: PermissionRequest) -> BoxFuture<'static, PermissionDecision> {
        Box::pin(async { PermissionDecision::Reject })
    }
}

/// The runtime's tools an in-process provider may call directly, bound to the calling agent
/// and task ([`super::comms`]). Providers that run a process reach the same tools through the
/// MCP bridge instead.
pub trait ToolHost: Send + Sync {
    fn call(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> BoxFuture<'static, Result<serde_json::Value, String>>;
}

/// Everything a turn needs besides its prompt.
#[derive(Clone)]
pub struct TurnContext {
    pub agent: AgentId,
    pub task: TaskId,
    pub thread: ThreadId,
    /// The folder the turn may read and write in.
    pub root: PathBuf,
    pub permission: PermissionMode,
    pub gate: Arc<dyn PermissionGate>,
    pub tools: Arc<dyn ToolHost>,
    pub events: mpsc::UnboundedSender<ProviderEvent>,
    pub cancel: CancellationToken,
}

/// A provider failure, sorted by what the scheduler may do about it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// It never started: nothing ran, so the task can be retried as is.
    #[error("could not start: {0}")]
    Start(String),
    /// The process went away or the connection broke.
    #[error("disconnected: {0}")]
    Disconnected(String),
    /// It answered with an error.
    #[error("{0}")]
    Protocol(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// One agent's live provider runtime.
#[async_trait]
pub trait ProviderRuntime: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities;
    /// Opens a session for a thread, restoring `request.restore` when the runtime can.
    async fn open_session(&self, request: SessionRequest)
    -> Result<ProviderSession, ProviderError>;
    /// Runs one turn to its end, streaming to `ctx.events`. Cancelling `ctx.cancel` asks the
    /// provider to stop; the turn then ends with [`StopReason::Cancelled`] as the provider
    /// acknowledges it.
    async fn prompt(
        &self,
        session: &ProviderSession,
        text: String,
        ctx: TurnContext,
    ) -> Result<TurnOutcome, ProviderError>;
    async fn cancel(&self, session: &ProviderSession) -> Result<(), ProviderError>;
    async fn close_session(&self, session: ProviderSession) -> Result<(), ProviderError>;
    /// Ends the runtime and its process. Never leaves a child behind.
    async fn shutdown(&self);
    /// Whether it can still take turns (its process lives).
    fn is_alive(&self) -> bool;
    /// Recent diagnostics (the process's last stderr lines), for failure reports.
    fn diagnostics(&self) -> String {
        String::new()
    }
    /// The provider's process, when it runs one.
    fn process_id(&self) -> Option<u32> {
        None
    }
}

/// What starting an agent's runtime gets.
#[derive(Clone, Debug)]
pub struct StartRequest {
    pub profile: AgentProfile,
}

/// An adapter kind: starts runtimes for agents configured with it.
#[async_trait]
pub trait AgentProvider: Send + Sync {
    fn id(&self) -> &str;
    /// What its runtimes can do at most, before one is started.
    fn capabilities(&self) -> ProviderCapabilities;
    async fn start(&self, request: StartRequest)
    -> Result<Arc<dyn ProviderRuntime>, ProviderError>;
}

/// The provider adapters the runtime knows, by id.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn AgentProvider>>,
}

impl ProviderRegistry {
    pub fn with(mut self, provider: Arc<dyn AgentProvider>) -> Self {
        self.providers.insert(provider.id().to_string(), provider);
        self
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.providers.get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.providers.keys().cloned().collect();
        ids.sort();
        ids
    }
}
