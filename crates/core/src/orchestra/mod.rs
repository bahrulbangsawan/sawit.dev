//! The agent runtime: agents with persistent identities that run on provider adapters (ACP
//! coding agents first), a durable task queue that a drain scheduler runs within each agent's
//! and each provider's concurrency, and messages and delegated tasks between agents, routed back
//! to the thread that asked. Nothing here knows GPUI: the app holds an [`Orchestra`] and
//! listens to its [`RuntimeEvent`]s; `sawit-mcp` gives agents the communication tools by
//! working on the same store ([`comms`]).
//!
//! - [`store`]: SQLite (`<Sawit data>/orchestra.sqlite`, WAL), versioned migrations; the only
//!   source of truth. The app's scheduler and every `sawit-mcp` share it.
//! - [`provider`]: the provider-independent interface; [`acp`] implements it for Agent Client
//!   Protocol agents over stdio.
//! - [`scheduler`] and [`exec`]: claims tasks atomically and runs each turn with a cancellation
//!   token, a timeout and the provider's concurrency.
//! - [`comms`]: `list_agents`, `send_message`, `delegate_task`, `get_task_status`,
//!   `list_agent_tasks` and `cancel_task`, with authorization and delegation limits enforced
//!   here ([`policy`]), never left to prompts.
//! - [`runtime`]: the API the app uses, lifecycle and recovery.

pub mod acp;
pub mod comms;
pub mod events;
pub mod exec;
pub mod flow;
pub mod ids;
pub mod lifecycle;
pub mod model;
pub mod policy;
pub mod prompt;
pub mod provider;
pub mod registry;
pub mod runtime;
pub mod scheduler;
pub mod store;
#[cfg(test)]
mod tests;
pub mod workspace;

use std::time::Duration;

pub use comms::SubmitTask;
pub use comms::{Caller, Comms};
pub use events::RuntimeEvent;
pub use ids::{AgentId, ConversationId, DeliveryId, MessageId, ProviderId, TaskId, ThreadId};
pub use model::{
    AgentMessage, AgentProfile, AgentState, CommsPolicy, Delivery, DeliveryStatus, Isolation,
    Launch, MessagePayload, PermissionMode, Task, TaskPayload, TaskStatus,
};
pub use registry::NewAgent;
pub use runtime::{AgentStatus, Orchestra, OrchestraConfig};
pub use store::Store;

/// The runtime's resource limits. Every one is configurable; the defaults suit one person's Mac.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Turns running at once across every agent.
    pub max_running: usize,
    /// Provider processes alive at once.
    pub max_live_agents: usize,
    /// How deep a chain of delegations may go (a root task is depth 0).
    pub max_delegation_depth: u32,
    /// Unfinished delegated tasks one root task may have at once.
    pub max_outstanding_per_root: u32,
    /// Queued tasks one agent may have before new work for it is refused (backpressure).
    pub max_queued_per_agent: u32,
    /// Pending messages one agent's mailbox may hold.
    pub max_pending_messages: u32,
    /// The largest message or instruction text, in bytes.
    pub max_message_bytes: usize,
    /// A turn's time limit when the task gives none.
    pub default_timeout: Duration,
    /// Attempts a task gets when its submitter gives none.
    pub default_max_attempts: u32,
    /// How long an idle provider process is kept before it is stopped.
    pub idle_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_running: 8,
            max_live_agents: 8,
            max_delegation_depth: 3,
            max_outstanding_per_root: 16,
            max_queued_per_agent: 64,
            max_pending_messages: 256,
            max_message_bytes: 64 * 1024,
            default_timeout: Duration::from_secs(30 * 60),
            default_max_attempts: 3,
            idle_timeout: Duration::from_secs(10 * 60),
        }
    }
}

/// Why a runtime operation was refused or failed. Every message is meant to be shown as is, to
/// a person or to the agent that called a tool.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum OrchestraError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    /// Authorization refused it.
    #[error("not allowed: {0}")]
    Denied(String),
    /// A resource limit refused it.
    #[error("limit reached: {0}")]
    Limit(String),
    /// The record is not in a state that allows it.
    #[error("{0}")]
    Conflict(String),
    #[error("store: {0}")]
    Store(String),
    #[error("provider: {0}")]
    Provider(String),
}

pub type Result<T> = std::result::Result<T, OrchestraError>;

impl From<rusqlite::Error> for OrchestraError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.to_string())
    }
}
