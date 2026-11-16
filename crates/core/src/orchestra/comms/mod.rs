//! The tools agents work together with, over the store alone so that they run the same in the
//! app and in `sawit-mcp`: `list_agents`, `send_message`, `delegate_task`, `get_task_status`,
//! `list_agent_tasks` and `cancel_task`. Every call is made as a [`Caller`] (an agent in one of
//! its tasks, or the user) and authorized here ([`super::policy`]). A successful send or
//! delegation means the message and task are durably stored, not that the recipient has acted.

mod ops;
mod query;
mod views;

use std::sync::Arc;

use serde_json::{Value, json};

pub use ops::{CancelReport, Delegated, Sent, SubmitTask};
pub use views::{AgentCard, TaskView};

use super::events::RuntimeEvent;
use super::ids::{AgentId, TaskId, ThreadId};
use super::model::{AgentProfile, Task};
use super::store::{Store, agents, tasks};
use super::{Limits, OrchestraError, Result};

/// Who calls a tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// `None`: the user (or a client acting for them), who may do anything.
    pub agent: Option<AgentId>,
    /// The task the agent is running, when it calls from a turn.
    pub task: Option<TaskId>,
}

impl Caller {
    pub fn user() -> Self {
        Self {
            agent: None,
            task: None,
        }
    }

    pub fn agent(agent: AgentId, task: Option<TaskId>) -> Self {
        Self {
            agent: Some(agent),
            task,
        }
    }
}

/// The names of the tools [`Comms::call`] answers.
pub const TOOLS: [&str; 6] = [
    "list_agents",
    "send_message",
    "delegate_task",
    "get_task_status",
    "list_agent_tasks",
    "cancel_task",
];

/// The communication service over a store.
#[derive(Clone)]
pub struct Comms {
    pub store: Store,
    pub limits: Limits,
    /// Called after a change that gives the scheduler work (in the app: wakes it at once).
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Called with the events of each committed change.
    publish: Option<Arc<dyn Fn(Vec<RuntimeEvent>) + Send + Sync>>,
}

impl Comms {
    pub fn new(store: Store, limits: Limits) -> Self {
        Self {
            store,
            limits,
            wake: None,
            publish: None,
        }
    }

    pub fn with_hooks(
        mut self,
        wake: Arc<dyn Fn() + Send + Sync>,
        publish: Arc<dyn Fn(Vec<RuntimeEvent>) + Send + Sync>,
    ) -> Self {
        self.wake = Some(wake);
        self.publish = Some(publish);
        self
    }

    fn after(&self, events: Vec<RuntimeEvent>) {
        if let Some(p) = &self.publish {
            p(events);
        }
        if let Some(w) = &self.wake {
            w();
        }
    }

    /// The caller a `sawit-mcp` started for a runtime thread proves itself as: the agent, the
    /// thread and its secret token, from the environment the runtime gave the MCP server. The
    /// caller's task is the one the thread is running now.
    pub fn caller_for_thread(
        &self,
        agent: &AgentId,
        thread: &ThreadId,
        token: &str,
    ) -> Result<Caller> {
        self.store.with(|c| {
            let t = agents::get_thread(c, thread)?;
            if &t.agent_id != agent || t.token != token {
                return Err(OrchestraError::Denied(
                    "this agent identity doesn't check out".into(),
                ));
            }
            let task = tasks::running_in_thread(c, thread)?.map(|t| t.id);
            Ok(Caller::agent(agent.clone(), task))
        })
    }

    pub(crate) fn profile(
        c: &rusqlite::Connection,
        caller: &Caller,
    ) -> Result<Option<AgentProfile>> {
        caller
            .agent
            .as_ref()
            .map(|a| agents::get_agent(c, a.as_str()).map(|r| r.profile))
            .transpose()
    }

    /// The caller's current task, which must be its own and in a turn.
    pub(crate) fn current_task(
        c: &rusqlite::Connection,
        caller: &Caller,
        given: Option<&TaskId>,
    ) -> Result<Option<Task>> {
        let Some(agent) = &caller.agent else {
            return given.map(|id| tasks::get_task(c, id)).transpose();
        };
        let Some(id) = given.or(caller.task.as_ref()) else {
            return Ok(None);
        };
        let task = tasks::get_task(c, id)?;
        if &task.agent_id != agent {
            return Err(OrchestraError::Denied(format!("task {id} isn't {agent}'s")));
        }
        Ok(Some(task))
    }

    /// Runs a tool by name with JSON arguments, as an agent's tool call arrives.
    pub fn call(&self, caller: &Caller, name: &str, args: &Value) -> Result<Value> {
        let s = |k: &str| {
            args.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let need =
            |k: &str| s(k).ok_or_else(|| OrchestraError::Invalid(format!("`{k}` is required")));
        match name {
            "list_agents" => Ok(json!({"agents": val(&self.list_agents(caller)?)})),
            "send_message" => {
                let sent = self.send_message(
                    caller,
                    need("recipient")?,
                    need("content")?,
                    s("correlation_id").map(Into::into),
                )?;
                Ok(val(&sent))
            }
            "delegate_task" => {
                let d = self.delegate_task(
                    caller,
                    need("recipient")?,
                    need("title")?,
                    need("instructions")?,
                    s("parent_task_id").map(Into::into),
                    s("idempotency_key").map(str::to_string),
                )?;
                Ok(val(&d))
            }
            "get_task_status" => Ok(val(&self.get_task_status(caller, &need("task_id")?.into())?)),
            "list_agent_tasks" => {
                let status = s("status").map(str::to_string);
                Ok(
                    json!({"tasks": val(&self.list_agent_tasks(caller, s("agent"), status.as_deref())?)}),
                )
            }
            "cancel_task" => Ok(val(&self.cancel_task(caller, &need("task_id")?.into())?)),
            other => Err(OrchestraError::NotFound(format!(
                "no tool `{other}`; there are {}",
                TOOLS.join(", ")
            ))),
        }
    }
}

fn val<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}
