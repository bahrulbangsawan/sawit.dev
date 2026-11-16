//! The tools' operations. Each is one store transaction: the records it makes (a task, its
//! message and the message's delivery) are stored together or not at all.

use serde::Serialize;

use super::views::{AgentCard, TaskView};
use super::{Caller, Comms};
use crate::orchestra::events::RuntimeEvent;
use crate::orchestra::ids::{
    AgentId, ConversationId, DeliveryId, MessageId, TaskId, ThreadId, now_ms,
};
use crate::orchestra::model::{
    AgentMessage, Delivery, DeliveryStatus, MessagePayload, PermissionMode, Task, TaskPayload,
    TaskStatus,
};
use crate::orchestra::policy;
use crate::orchestra::store::tasks::NewTaskRow;
use crate::orchestra::store::{agents, mail, tasks};
use crate::orchestra::{OrchestraError, Result};

/// Work the user (or a client acting for them) hands an agent.
#[derive(Clone, Debug, Default)]
pub struct SubmitTask {
    /// The agent, by id or name.
    pub agent: String,
    pub text: String,
    pub title: Option<String>,
    /// The thread to run in; the agent's main thread when `None`, a new side thread when
    /// `new_thread`.
    pub thread: Option<ThreadId>,
    pub new_thread: bool,
    /// At most the agent's own mode.
    pub permission: Option<PermissionMode>,
    pub max_attempts: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub idempotency_key: Option<String>,
    /// Stored held: it runs once released.
    pub held: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sent {
    pub message_id: MessageId,
    pub delivery_id: DeliveryId,
    pub conversation_id: ConversationId,
    /// The recipient's task that takes the message in.
    pub task_id: TaskId,
    pub status: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Delegated {
    pub task_id: TaskId,
    pub thread_id: ThreadId,
    pub message_id: Option<MessageId>,
    pub delivery_id: Option<DeliveryId>,
    /// False when the idempotency key named a task made before.
    pub created: bool,
    pub status: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CancelReport {
    pub cancelled: Vec<TaskId>,
    /// Running: asked to stop.
    pub stopping: Vec<TaskId>,
}

fn title_of(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Task")
        .trim();
    let mut t: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        t.push('…');
    }
    t
}

impl Comms {
    pub fn list_agents(&self, caller: &Caller) -> Result<Vec<AgentCard>> {
        self.store.with(|c| {
            let me = Self::profile(c, caller)?;
            let mut out = Vec::new();
            for row in agents::list_agents(c)? {
                let p = &row.profile;
                if !policy::can_discover(me.as_ref(), p) {
                    continue;
                }
                let can_message = policy::check_contact(me.as_ref(), p).is_ok();
                let can_delegate = policy::check_delegate(me.as_ref(), p).is_ok();
                out.push(AgentCard::new(
                    p,
                    &row.state,
                    tasks::queued_count(c, &p.id)?,
                    can_message,
                    can_delegate,
                ));
            }
            Ok(out)
        })
    }

    /// Hands the user's work to an agent: a root task.
    pub fn submit_task(&self, req: &SubmitTask) -> Result<TaskView> {
        policy::check_size(&self.limits, "the task", &req.text)?;
        let (view, events) = self.store.tx(|c| {
            let agent = agents::get_agent(c, &req.agent)?.profile;
            if !agent.enabled {
                return Err(OrchestraError::Denied(format!(
                    "agent {} is disabled",
                    agent.id
                )));
            }
            self.check_queue(c, &agent.id)?;
            let title = req.title.clone().unwrap_or_else(|| title_of(&req.text));
            let thread = match (&req.thread, req.new_thread) {
                (Some(t), _) => {
                    let t = agents::get_thread(c, t)?;
                    if t.agent_id != agent.id {
                        return Err(OrchestraError::Invalid(format!(
                            "thread {} isn't {}'s",
                            t.id, agent.id
                        )));
                    }
                    t
                }
                (None, true) => agents::side_thread(c, &agent.id, &title)?,
                (None, false) => agents::main_thread(c, &agent.id)?,
            };
            let (task, _) = tasks::insert_task(
                c,
                &NewTaskRow {
                    id: TaskId::generate(),
                    agent_id: agent.id.clone(),
                    thread_id: thread.id,
                    parent_task_id: None,
                    root_task_id: None,
                    depth: 0,
                    requested_by: None,
                    title,
                    payload: TaskPayload::Prompt {
                        text: req.text.clone(),
                    },
                    permission: req
                        .permission
                        .map_or(agent.permission, |p| p.min(agent.permission)),
                    max_attempts: req.max_attempts.unwrap_or(self.limits.default_max_attempts),
                    timeout_ms: req.timeout_ms,
                    idempotency_key: req.idempotency_key.clone(),
                    held: req.held,
                },
            )?;
            Ok((TaskView::new(&task, 0), vec![queued(&task)]))
        })?;
        self.after(events);
        Ok(view)
    }

    fn check_queue(&self, c: &rusqlite::Connection, agent: &AgentId) -> Result<()> {
        let queued = tasks::queued_count(c, agent)?;
        if queued >= self.limits.max_queued_per_agent {
            return Err(OrchestraError::Limit(format!(
                "{agent} already has {queued} queued tasks (limit {}); try again later",
                self.limits.max_queued_per_agent
            )));
        }
        Ok(())
    }

    pub fn send_message(
        &self,
        caller: &Caller,
        recipient: &str,
        content: &str,
        correlation: Option<MessageId>,
    ) -> Result<Sent> {
        policy::check_size(&self.limits, "the message", content)?;
        let (sent, events) = self.store.tx(|c| {
            let me = Self::profile(c, caller)?;
            let to = agents::get_agent(c, recipient)?.profile;
            policy::check_contact(me.as_ref(), &to)?;
            let pending = mail::pending_count(c, &to.id)?;
            if pending >= self.limits.max_pending_messages {
                return Err(OrchestraError::Limit(format!(
                    "{}'s mailbox is full ({pending} messages)",
                    to.id
                )));
            }
            self.check_queue(c, &to.id)?;
            let conversation = match &correlation {
                Some(id) => {
                    let earlier = mail::get_message(c, id)?;
                    let involved = |a: &Option<AgentId>| a.as_ref() == caller.agent.as_ref();
                    if caller.agent.is_some()
                        && !involved(&earlier.sender)
                        && !involved(&Some(earlier.recipient.clone()))
                    {
                        return Err(OrchestraError::Denied(format!(
                            "message {id} wasn't to or from you"
                        )));
                    }
                    earlier.conversation_id
                }
                None => ConversationId::generate(),
            };
            // A message sent from a task counts as one more step of that task's chain, so agents
            // messaging each other back and forth stop at the depth limit.
            let from = Self::current_task(c, caller, None)?;
            let depth = from.as_ref().map_or(0, |t| t.depth + 1);
            if depth > self.limits.max_delegation_depth {
                return Err(OrchestraError::Limit(format!(
                    "this chain of work is {depth} steps deep (limit {})",
                    self.limits.max_delegation_depth
                )));
            }
            let ceiling = from
                .as_ref()
                .map(|t| t.permission)
                .or(me.as_ref().map(|m| m.permission));
            let thread = agents::conversation_thread(c, &to.id, &conversation)?;
            let (message_id, delivery_id, task_id) = (
                MessageId::generate(),
                DeliveryId::generate(),
                TaskId::generate(),
            );
            let sender = me
                .as_ref()
                .map_or("the user".to_string(), |m| m.name.clone());
            let (task, _) = tasks::insert_task(
                c,
                &NewTaskRow {
                    id: task_id.clone(),
                    agent_id: to.id.clone(),
                    thread_id: thread.id,
                    parent_task_id: None,
                    root_task_id: from.as_ref().map(|t| t.root_task_id.clone()),
                    depth,
                    requested_by: caller.agent.clone(),
                    title: format!("Message from {sender}"),
                    payload: TaskPayload::Message {
                        delivery: delivery_id.clone(),
                    },
                    permission: ceiling.map_or(to.permission, |p| p.min(to.permission)),
                    max_attempts: self.limits.default_max_attempts,
                    timeout_ms: None,
                    idempotency_key: None,
                    held: false,
                },
            )?;
            let message = AgentMessage {
                id: message_id.clone(),
                sender: caller.agent.clone(),
                recipient: to.id.clone(),
                conversation_id: conversation.clone(),
                correlation_id: correlation.clone(),
                task_id: from.map(|t| t.id),
                payload: MessagePayload::Text {
                    content: content.to_string(),
                },
                created_at: now_ms(),
            };
            mail::insert_message(c, &message)?;
            mail::insert_delivery(c, &delivery(&delivery_id, &message_id, &to.id, &task_id))?;
            let events = vec![
                RuntimeEvent::MessageSent {
                    message: message_id.clone(),
                    sender: caller.agent.clone(),
                    recipient: to.id.clone(),
                },
                RuntimeEvent::MessageReceived {
                    message: message_id.clone(),
                    recipient: to.id.clone(),
                    task: Some(task_id.clone()),
                },
                queued(&task),
            ];
            Ok((
                Sent {
                    message_id,
                    delivery_id,
                    conversation_id: conversation,
                    task_id,
                    status: "accepted",
                },
                events,
            ))
        })?;
        self.after(events);
        Ok(sent)
    }

    pub fn delegate_task(
        &self,
        caller: &Caller,
        recipient: &str,
        title: &str,
        instructions: &str,
        parent: Option<TaskId>,
        idempotency_key: Option<String>,
    ) -> Result<Delegated> {
        policy::check_size(&self.limits, "the instructions", instructions)?;
        policy::check_size(&self.limits, "the title", title)?;
        let title: String = title.chars().take(200).collect();
        let (out, events) = self.store.tx(|c| {
            let me = Self::profile(c, caller)?;
            let to = agents::get_agent(c, recipient)?.profile;
            policy::check_delegate(me.as_ref(), &to)?;
            let parent = Self::current_task(c, caller, parent.as_ref())?;
            if caller.agent.is_some() && parent.is_none() {
                return Err(OrchestraError::Invalid(
                    "delegate_task needs the task you are working on (parent_task_id)".into(),
                ));
            }
            if let Some(p) = &parent {
                if caller.agent.is_some()
                    && !matches!(p.status, TaskStatus::Starting | TaskStatus::Running)
                {
                    return Err(OrchestraError::Conflict(format!(
                        "task {} is {}, not running",
                        p.id,
                        p.status.name()
                    )));
                }
                let chain: Vec<AgentId> = tasks::chain_agents(c, &p.id)?;
                policy::check_chain(
                    &self.limits,
                    &chain,
                    p.depth,
                    tasks::open_delegations(c, &p.root_task_id)?,
                    &to.id,
                )?;
            }
            self.check_queue(c, &to.id)?;
            let mode = match &parent {
                Some(p) => policy::delegated_mode(p.permission, &to),
                None => to.permission,
            };
            let thread = agents::side_thread(c, &to.id, &title)?;
            let task_id = TaskId::generate();
            let (task, created) = tasks::insert_task(
                c,
                &NewTaskRow {
                    id: task_id.clone(),
                    agent_id: to.id.clone(),
                    thread_id: thread.id.clone(),
                    parent_task_id: parent.as_ref().map(|p| p.id.clone()),
                    root_task_id: parent.as_ref().map(|p| p.root_task_id.clone()),
                    depth: parent.as_ref().map_or(0, |p| p.depth + 1),
                    requested_by: caller.agent.clone(),
                    title: title.clone(),
                    payload: TaskPayload::Delegated {
                        title: title.clone(),
                        instructions: instructions.to_string(),
                    },
                    permission: mode,
                    max_attempts: self.limits.default_max_attempts,
                    timeout_ms: None,
                    idempotency_key,
                    held: false,
                },
            )?;
            if !created {
                let v = Delegated {
                    task_id: task.id.clone(),
                    thread_id: task.thread_id.clone(),
                    message_id: None,
                    delivery_id: None,
                    created,
                    status: task.status.name(),
                };
                return Ok((v, Vec::new()));
            }
            let message = AgentMessage {
                id: MessageId::generate(),
                sender: caller.agent.clone(),
                recipient: to.id.clone(),
                conversation_id: ConversationId::generate(),
                correlation_id: None,
                task_id: Some(task.id.clone()),
                payload: MessagePayload::Delegation {
                    task: task.id.clone(),
                    title,
                    instructions: instructions.to_string(),
                },
                created_at: now_ms(),
            };
            mail::insert_message(c, &message)?;
            let delivery_id = DeliveryId::generate();
            mail::insert_delivery(c, &delivery(&delivery_id, &message.id, &to.id, &task.id))?;
            let events = vec![
                RuntimeEvent::MessageSent {
                    message: message.id.clone(),
                    sender: caller.agent.clone(),
                    recipient: to.id.clone(),
                },
                queued(&task),
            ];
            let v = Delegated {
                task_id: task.id.clone(),
                thread_id: thread.id,
                message_id: Some(message.id),
                delivery_id: Some(delivery_id),
                created,
                status: task.status.name(),
            };
            Ok((v, events))
        })?;
        self.after(events);
        Ok(out)
    }
}

fn queued(task: &Task) -> RuntimeEvent {
    RuntimeEvent::TaskQueued {
        task: task.id.clone(),
        agent: task.agent_id.clone(),
        thread: task.thread_id.clone(),
        parent: task.parent_task_id.clone(),
    }
}

fn delivery(id: &DeliveryId, message: &MessageId, to: &AgentId, task: &TaskId) -> Delivery {
    Delivery {
        id: id.clone(),
        message_id: message.clone(),
        recipient: to.clone(),
        task_id: Some(task.clone()),
        status: DeliveryStatus::Pending,
        attempts: 0,
        last_error: None,
        updated_at: now_ms(),
    }
}
