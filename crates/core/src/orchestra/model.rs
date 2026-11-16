//! What the runtime keeps: an agent's profile (who it is, kept across restarts), the lifecycle
//! state of its runtime (what runs now, never conflated with the profile), and the records of
//! work: tasks, messages and the deliveries of messages.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::ids::{
    AgentId, ConversationId, DeliveryId, MessageId, ProviderId, TaskId, ThreadId, Timestamp,
};

/// How a program is started: a program and its arguments, never a shell line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Added to the inherited environment. Never logged: it may hold keys.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// What an agent may do without asking, from least to most. A delegated task runs with the lower
/// of its parent's mode and its agent's own ([`PermissionMode::min`]), so delegation never
/// widens what a request was allowed.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Every tool use the provider asks about is refused; files are neither read nor written.
    Deny,
    /// Reads, searches and fetches are allowed; edits and commands are refused.
    ReadOnly,
    /// Reads are allowed; everything else is asked of the IDE's permission gate (refused when
    /// nobody answers).
    #[default]
    Ask,
    /// Everything is allowed.
    Allow,
}

impl PermissionMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::ReadOnly => "read_only",
            Self::Ask => "ask",
            Self::Allow => "allow",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Deny, Self::ReadOnly, Self::Ask, Self::Allow]
            .into_iter()
            .find(|m| m.name() == s)
    }

    /// Whether replaying work done in this mode cannot change anything outside the agent.
    pub fn side_effect_free(self) -> bool {
        self <= Self::ReadOnly
    }
}

/// Where an agent's work happens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// In the agent's working folder. Agents sharing a folder take turns there (one task at a
    /// time per folder), so none overwrites another's edits mid-turn.
    #[default]
    Shared,
    /// In a git worktree of the working folder made for each thread, on its own branch.
    Worktree,
    /// In the agent's folder without taking turns: for agents that only read, or whose tasks
    /// touch separate files.
    Free,
}

/// Who an agent may reach and who may reach it. `"*"` stands for every agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommsPolicy {
    /// Whether other agents see this one in `list_agents`.
    pub discoverable: bool,
    /// Agents this one may message or delegate to.
    pub may_contact: Vec<String>,
    /// Whether this agent may delegate tasks at all.
    pub may_delegate: bool,
    /// Agents this one takes messages and tasks from.
    pub accepts_from: Vec<String>,
}

impl Default for CommsPolicy {
    fn default() -> Self {
        Self {
            discoverable: true,
            may_contact: vec!["*".into()],
            may_delegate: true,
            accepts_from: vec!["*".into()],
        }
    }
}

impl CommsPolicy {
    fn lists(list: &[String], id: &AgentId) -> bool {
        list.iter().any(|e| e == "*" || e == id.as_str())
    }

    pub fn may_contact(&self, id: &AgentId) -> bool {
        Self::lists(&self.may_contact, id)
    }

    pub fn accepts(&self, id: &AgentId) -> bool {
        Self::lists(&self.accepts_from, id)
    }

    /// Whether `id` is named on the accept list itself, not through `"*"`.
    pub fn names(&self, id: &AgentId) -> bool {
        self.accepts_from.iter().any(|e| e == id.as_str())
    }
}

/// An agent's persistent identity and configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: AgentId,
    pub name: String,
    /// What it is for, in a word or two (`backend`, `reviewer`).
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub description: String,
    /// The adapter it runs on (`acp`).
    pub provider: ProviderId,
    /// The provider's program, for adapters that start one.
    #[serde(default)]
    pub launch: Option<Launch>,
    #[serde(default)]
    pub model: Option<String>,
    /// Sent at the start of every new provider session.
    #[serde(default)]
    pub instructions: String,
    /// The project it belongs to: agents reach only agents of the same workspace, unless the
    /// recipient names them on its accept list.
    pub workspace: PathBuf,
    /// The folder it works in.
    pub workdir: PathBuf,
    #[serde(default)]
    pub isolation: Isolation,
    #[serde(default)]
    pub permission: PermissionMode,
    /// What it says it can do, for discovery (`rust`, `review`, `tests`).
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub comms: CommsPolicy,
    /// Tasks it may run at once; the provider's own limit can lower it.
    pub max_concurrent_tasks: u32,
    pub enabled: bool,
    /// Adapter-specific settings.
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub created_at: Timestamp,
    #[serde(default)]
    pub updated_at: Timestamp,
}

/// The lifecycle state of an agent's runtime: its process and connection, not its profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Known, with nothing running.
    Registered,
    Starting,
    /// Running and idle.
    Ready,
    /// Running a task.
    Busy,
    /// Running but taking no new work.
    Paused,
    Stopping,
    Stopped,
    /// Its provider failed to start or died; a new start may be tried.
    Failed,
}

impl AgentState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Busy => "busy",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }

    /// Whether the lifecycle may go from `self` to `to`.
    pub fn can_go(self, to: AgentState) -> bool {
        use AgentState::*;
        matches!(
            (self, to),
            (Registered | Stopped | Failed, Starting)
                | (Starting, Ready | Failed | Stopping)
                | (Ready, Busy | Paused | Stopping | Failed)
                | (Busy, Ready | Paused | Stopping | Failed)
                | (Paused, Ready | Busy | Stopping | Failed)
                | (Stopping, Stopped | Failed)
        )
    }

    /// Whether a provider runtime exists in this state.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Ready | Self::Busy | Self::Paused)
    }
}

/// Where a task stands in the queue: not the provider session's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Waiting for capacity (maybe until `not_before`, for a retry).
    Queued,
    /// Held by the user: not scheduled until released.
    Held,
    /// Claimed by a scheduler; the provider has not been sent the prompt yet.
    Starting,
    /// The prompt was sent.
    Running,
    /// Its turn ended with delegated tasks still out: it resumes when they finish.
    Waiting,
    Completed,
    Failed,
    Cancelled,
    /// It was running when the runtime stopped and may have changed things: a person has to
    /// look before it runs again ([`super::Orchestra::requeue`]).
    Interrupted,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 9] = [
        Self::Queued,
        Self::Held,
        Self::Starting,
        Self::Running,
        Self::Waiting,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
        Self::Interrupted,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Held => "held",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == s)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// What a task asks of its agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskPayload {
    /// A request from the user (or from an agent acting without a task).
    Prompt { text: String },
    /// Work delegated by another agent's task.
    Delegated { title: String, instructions: String },
    /// A message that arrived in the agent's mailbox.
    Message { delivery: DeliveryId },
}

/// One unit of scheduled work, as stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub agent_id: AgentId,
    pub thread_id: ThreadId,
    pub parent_task_id: Option<TaskId>,
    pub root_task_id: TaskId,
    /// 0 for a root task, the parent's depth + 1 for a delegated one.
    pub depth: u32,
    /// The agent that asked for it; `None` for the user.
    pub requested_by: Option<AgentId>,
    pub title: String,
    pub payload: TaskPayload,
    pub status: TaskStatus,
    pub permission: PermissionMode,
    pub attempts: u32,
    pub max_attempts: u32,
    pub not_before: Option<Timestamp>,
    pub timeout_ms: Option<u64>,
    pub idempotency_key: Option<String>,
    /// The last turn's final text, or the result a worker returned.
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub failure: Option<String>,
    pub cancel_requested: bool,
    pub prompt_sent_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

/// What a message says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessagePayload {
    Text {
        content: String,
    },
    /// A delegated task was handed over.
    Delegation {
        task: TaskId,
        title: String,
        instructions: String,
    },
    /// A delegated task ended: the worker's answer for the task that asked for it.
    Result {
        task: TaskId,
        status: String,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// An immutable message. Its delivery is a separate record ([`Delivery`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: MessageId,
    /// `None` when the user sent it.
    pub sender: Option<AgentId>,
    pub recipient: AgentId,
    pub conversation_id: ConversationId,
    pub correlation_id: Option<MessageId>,
    /// The task it was sent from, when one.
    pub task_id: Option<TaskId>,
    pub payload: MessagePayload,
    pub created_at: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// Stored; the recipient's turn has not taken it yet.
    Pending,
    /// A turn took it into its prompt.
    Claimed,
    /// The turn that took it ended well.
    Delivered,
    /// It could not be delivered (the recipient was deleted, or retries ran out).
    Failed,
}

impl DeliveryStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Pending, Self::Claimed, Self::Delivered, Self::Failed]
            .into_iter()
            .find(|d| d.name() == s)
    }
}

/// The delivery of one message to its recipient, and the task that takes it in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    pub id: DeliveryId,
    pub message_id: MessageId,
    pub recipient: AgentId,
    pub task_id: Option<TaskId>,
    pub status: DeliveryStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub updated_at: Timestamp,
}
