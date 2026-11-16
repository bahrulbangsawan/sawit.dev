//! What the runtime tells the IDE, as it happens. Every event carries the ids that place it (the
//! agent, the task, the thread), so a view can update the right row. Events are a notification
//! only: the store stays the source of truth, and a listener that falls behind (the channel is
//! bounded) reloads from it.

use serde::Serialize;

use super::ids::{AgentId, MessageId, TaskId, ThreadId};
use super::model::{AgentState, TaskStatus};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RuntimeEvent {
    AgentCreated {
        agent: AgentId,
    },
    AgentUpdated {
        agent: AgentId,
    },
    AgentDeleted {
        agent: AgentId,
    },
    AgentStatusChanged {
        agent: AgentId,
        state: AgentState,
        detail: Option<String>,
    },

    TaskQueued {
        task: TaskId,
        agent: AgentId,
        thread: ThreadId,
        parent: Option<TaskId>,
    },
    TaskStarted {
        task: TaskId,
        agent: AgentId,
        thread: ThreadId,
        attempt: u32,
    },
    /// A piece of a running turn's output (coalesced: one event per burst of chunks).
    TaskProgress {
        task: TaskId,
        agent: AgentId,
        thread: ThreadId,
        kind: String,
        text: String,
    },
    /// A turn ended with delegated work out; the task resumes when it is done.
    TaskWaiting {
        task: TaskId,
        agent: AgentId,
        open_children: u32,
    },
    TaskCompleted {
        task: TaskId,
        agent: AgentId,
        thread: ThreadId,
    },
    TaskFailed {
        task: TaskId,
        agent: AgentId,
        error: String,
        retrying: bool,
    },
    TaskCancelled {
        task: TaskId,
        agent: AgentId,
    },
    /// Any other status move (held, released, interrupted, requeued).
    TaskStatusChanged {
        task: TaskId,
        agent: AgentId,
        status: TaskStatus,
    },

    MessageSent {
        message: MessageId,
        sender: Option<AgentId>,
        recipient: AgentId,
    },
    MessageReceived {
        message: MessageId,
        recipient: AgentId,
        task: Option<TaskId>,
    },

    ProviderConnected {
        agent: AgentId,
        provider: String,
    },
    ProviderDisconnected {
        agent: AgentId,
        provider: String,
        reason: String,
    },

    SessionStarted {
        agent: AgentId,
        thread: ThreadId,
        session: String,
        restored: bool,
    },
    SessionEnded {
        agent: AgentId,
        thread: ThreadId,
        session: String,
    },

    /// The scheduler took over the store (and recovered what a previous run left).
    Recovered {
        requeued: usize,
        retried: usize,
        interrupted: usize,
    },
}
