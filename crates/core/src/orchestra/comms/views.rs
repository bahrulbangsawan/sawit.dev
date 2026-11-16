//! What the tools answer with: an agent as others discover it, and a task as its asker sees it.

use serde::Serialize;

use crate::orchestra::ids::{AgentId, TaskId, ThreadId, Timestamp};
use crate::orchestra::model::{AgentProfile, Task, TaskPayload};

/// An agent, as `list_agents` shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentCard {
    pub id: AgentId,
    pub name: String,
    pub role: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub provider: String,
    /// `enabled`/`disabled`, and the runtime state (`ready`, `busy`, …).
    pub enabled: bool,
    pub state: String,
    pub queued_tasks: u32,
    /// Whether the caller may delegate tasks to it.
    pub can_delegate: bool,
    /// Whether the caller may message it.
    pub can_message: bool,
}

impl AgentCard {
    pub fn new(
        p: &AgentProfile,
        state: &str,
        queued: u32,
        can_message: bool,
        can_delegate: bool,
    ) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            role: p.role.clone(),
            description: p.description.clone(),
            capabilities: p.capabilities.clone(),
            provider: p.provider.clone(),
            enabled: p.enabled,
            state: state.to_string(),
            queued_tasks: queued,
            can_delegate,
            can_message,
        }
    }
}

/// A task, as `get_task_status` and `list_agent_tasks` show it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TaskView {
    pub id: TaskId,
    pub agent: AgentId,
    pub thread: ThreadId,
    pub title: String,
    pub kind: &'static str,
    pub status: &'static str,
    pub parent_task_id: Option<TaskId>,
    pub root_task_id: TaskId,
    pub depth: u32,
    pub requested_by: Option<AgentId>,
    pub permission: &'static str,
    pub attempts: u32,
    pub max_attempts: u32,
    /// The agent's final answer, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub open_children: u32,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<Timestamp>,
}

impl TaskView {
    pub fn new(t: &Task, open_children: u32) -> Self {
        Self {
            id: t.id.clone(),
            agent: t.agent_id.clone(),
            thread: t.thread_id.clone(),
            title: t.title.clone(),
            kind: match t.payload {
                TaskPayload::Prompt { .. } => "prompt",
                TaskPayload::Delegated { .. } => "delegated",
                TaskPayload::Message { .. } => "message",
            },
            status: t.status.name(),
            parent_task_id: t.parent_task_id.clone(),
            root_task_id: t.root_task_id.clone(),
            depth: t.depth,
            requested_by: t.requested_by.clone(),
            permission: t.permission.name(),
            attempts: t.attempts,
            max_attempts: t.max_attempts,
            result: t
                .result
                .as_ref()
                .and_then(|r| r.get("text"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            error: t.error.clone(),
            open_children,
            created_at: t.created_at,
            updated_at: t.updated_at,
            finished_at: t.finished_at,
        }
    }
}
