//! How tasks move once they run, as store transactions: a turn's end (done, waiting on delegated
//! work, retried, failed, held for review), the result of a delegated task routed back to the
//! task that asked for it (and that task woken once all its delegations are done), and the
//! cancellation of a task with everything delegated from it. Each function runs inside the
//! caller's transaction and returns the events to publish once it commits.

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::events::RuntimeEvent;
use super::ids::{ConversationId, DeliveryId, MessageId, TaskId, Timestamp, now_ms};
use super::model::{AgentMessage, Delivery, DeliveryStatus, MessagePayload, Task, TaskStatus};
use super::store::{mail, tasks};
use super::{Limits, OrchestraError, Result};

/// What a failed turn may do next, as the executor judged it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Run it again after a pause (nothing ran, or running it again changes nothing), if it has
    /// attempts left; fail it otherwise.
    Retry,
    /// It failed for good.
    Fail,
    /// It may have changed things before it broke: hold it for a person to review.
    Review,
}

/// How a turn ended.
#[derive(Clone, Debug, PartialEq)]
pub enum TurnEnd {
    Done {
        text: String,
    },
    Cancelled,
    Failed {
        error: String,
        category: &'static str,
        disposition: Disposition,
    },
}

/// The pause before retry `attempt` (1-based): 2 s, 4 s, 8 s, … up to five minutes.
pub fn backoff_ms(attempt: u32) -> i64 {
    (2_000i64 << attempt.saturating_sub(1).min(8)).min(300_000)
}

fn moved(task: &Task) -> RuntimeEvent {
    RuntimeEvent::TaskStatusChanged {
        task: task.id.clone(),
        agent: task.agent_id.clone(),
        status: task.status,
    }
}

/// Records the end of the task's turn and moves it on.
pub fn end_turn(
    c: &Connection,
    id: &TaskId,
    end: TurnEnd,
    limits: &Limits,
) -> Result<(Task, Vec<RuntimeEvent>)> {
    let task = tasks::get_task(c, id)?;
    let live = [TaskStatus::Starting, TaskStatus::Running];
    if !live.contains(&task.status) {
        return Err(OrchestraError::Conflict(format!(
            "task {id} is {}, not running",
            task.status.name()
        )));
    }
    let claimed = mail::task_deliveries(c, id, DeliveryStatus::Claimed)?;
    let mut events = Vec::new();
    let end = if task.cancel_requested {
        TurnEnd::Cancelled
    } else {
        end
    };
    let task = match end {
        TurnEnd::Done { text } => {
            mail::set_deliveries(
                c,
                &claimed,
                &[DeliveryStatus::Claimed],
                DeliveryStatus::Delivered,
                None,
            )?;
            tasks::set_outcome(c, id, Some(&json!({"text": text})), None, None)?;
            let open = tasks::open_children(c, id)?;
            if open > 0 {
                let t = tasks::transition(
                    c,
                    id,
                    &live,
                    TaskStatus::Waiting,
                    Some("waiting on delegated work"),
                )?;
                events.push(RuntimeEvent::TaskWaiting {
                    task: id.clone(),
                    agent: t.agent_id.clone(),
                    open_children: open,
                });
                t
            } else if !mail::pending_for_task(c, id)?.is_empty() {
                let t = tasks::requeue(c, id, &live, None, "replies arrived during the turn")?;
                events.push(moved(&t));
                t
            } else {
                let t = tasks::transition(c, id, &live, TaskStatus::Completed, None)?;
                events.push(RuntimeEvent::TaskCompleted {
                    task: id.clone(),
                    agent: t.agent_id.clone(),
                    thread: t.thread_id.clone(),
                });
                t
            }
        }
        TurnEnd::Cancelled => {
            mail::set_deliveries(
                c,
                &claimed,
                &[DeliveryStatus::Claimed],
                DeliveryStatus::Failed,
                Some("cancelled"),
            )?;
            let t = tasks::transition(c, id, &live, TaskStatus::Cancelled, None)?;
            events.push(RuntimeEvent::TaskCancelled {
                task: id.clone(),
                agent: t.agent_id.clone(),
            });
            t
        }
        TurnEnd::Failed {
            error,
            category,
            disposition,
        } => {
            mail::set_deliveries(
                c,
                &claimed,
                &[DeliveryStatus::Claimed],
                DeliveryStatus::Pending,
                Some(&error),
            )?;
            tasks::set_outcome(c, id, None, Some(&error), Some(category))?;
            let retry = disposition == Disposition::Retry && task.attempts < task.max_attempts;
            let t = if retry {
                let at: Timestamp = now_ms() + backoff_ms(task.attempts);
                tasks::requeue(c, id, &live, Some(at), &format!("retry after: {category}"))?
            } else if disposition == Disposition::Review {
                tasks::transition(c, id, &live, TaskStatus::Interrupted, Some(category))?
            } else {
                let failed = mail::task_deliveries(c, id, DeliveryStatus::Pending)?;
                mail::set_deliveries(
                    c,
                    &failed,
                    &[DeliveryStatus::Pending],
                    DeliveryStatus::Failed,
                    Some(&error),
                )?;
                tasks::transition(c, id, &live, TaskStatus::Failed, Some(category))?
            };
            events.push(RuntimeEvent::TaskFailed {
                task: id.clone(),
                agent: t.agent_id.clone(),
                error,
                retrying: retry,
            });
            if t.status == TaskStatus::Interrupted {
                events.push(moved(&t));
            }
            t
        }
    };
    if task.status.is_terminal() {
        events.extend(settle(c, &task, limits)?);
    }
    Ok((task, events))
}

/// Routes a finished delegated task's result to the task that asked for it, as a message and a
/// delivery to that task, and wakes the asking task when none of its delegations is still open.
pub fn settle(c: &Connection, task: &Task, limits: &Limits) -> Result<Vec<RuntimeEvent>> {
    let Some(parent_id) = &task.parent_task_id else {
        return Ok(Vec::new());
    };
    let parent = tasks::get_task(c, parent_id)?;
    if parent.status.is_terminal() {
        return Ok(Vec::new());
    }
    let asked: Option<(MessageId, ConversationId)> = c
        .query_row(
            "SELECT id, conversation_id FROM messages WHERE task_id = ?1 AND sender IS ?2 ORDER BY seq LIMIT 1",
            rusqlite::params![task.id.as_str(), parent.agent_id.as_str()],
            |r| Ok((MessageId(r.get(0)?), ConversationId(r.get(1)?))),
        )
        .optional()?;
    let summary: String = task
        .result
        .as_ref()
        .and_then(|r| r.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .chars()
        .take(limits.max_message_bytes / 4)
        .collect();
    let message = AgentMessage {
        id: MessageId::generate(),
        sender: Some(task.agent_id.clone()),
        recipient: parent.agent_id.clone(),
        conversation_id: asked
            .as_ref()
            .map_or_else(ConversationId::generate, |a| a.1.clone()),
        correlation_id: asked.map(|a| a.0),
        task_id: Some(task.id.clone()),
        payload: MessagePayload::Result {
            task: task.id.clone(),
            status: task.status.name().into(),
            summary,
            error: task.error.clone(),
        },
        created_at: now_ms(),
    };
    mail::insert_message(c, &message)?;
    mail::insert_delivery(
        c,
        &Delivery {
            id: DeliveryId::generate(),
            message_id: message.id.clone(),
            recipient: parent.agent_id.clone(),
            task_id: Some(parent.id.clone()),
            status: DeliveryStatus::Pending,
            attempts: 0,
            last_error: None,
            updated_at: now_ms(),
        },
    )?;
    let mut events = vec![
        RuntimeEvent::MessageSent {
            message: message.id.clone(),
            sender: message.sender.clone(),
            recipient: parent.agent_id.clone(),
        },
        RuntimeEvent::MessageReceived {
            message: message.id,
            recipient: parent.agent_id.clone(),
            task: Some(parent.id.clone()),
        },
    ];
    if parent.status == TaskStatus::Waiting && tasks::open_children(c, &parent.id)? == 0 {
        let woken = tasks::requeue(
            c,
            &parent.id,
            &[TaskStatus::Waiting],
            None,
            "delegated work finished",
        )?;
        events.push(moved(&woken));
    }
    Ok(events)
}

/// What a cancellation did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cancelled {
    /// Stopped before they ran (or while waiting): cancelled now.
    pub cancelled: Vec<TaskId>,
    /// Running: asked to stop; they end as cancelled when their provider stops.
    pub signalled: Vec<TaskId>,
    pub events: Vec<RuntimeEvent>,
}

/// Cancels the task and everything delegated from it that hasn't ended.
pub fn cancel_tree(c: &Connection, id: &TaskId, limits: &Limits) -> Result<Cancelled> {
    let tree = tasks::subtree(c, id)?;
    if tree.is_empty() {
        return Err(OrchestraError::NotFound(format!("no task {id}")));
    }
    let mut out = Cancelled::default();
    for t in tree {
        match t.status {
            s if s.is_terminal() => {}
            TaskStatus::Starting | TaskStatus::Running => {
                tasks::request_cancel(c, &t.id)?;
                out.signalled.push(t.id);
            }
            from => {
                let done =
                    tasks::transition(c, &t.id, &[from], TaskStatus::Cancelled, Some("cancelled"))?;
                for status in [DeliveryStatus::Pending, DeliveryStatus::Claimed] {
                    let ids = mail::task_deliveries(c, &t.id, status)?;
                    mail::set_deliveries(
                        c,
                        &ids,
                        &[status],
                        DeliveryStatus::Failed,
                        Some("cancelled"),
                    )?;
                }
                out.events.push(RuntimeEvent::TaskCancelled {
                    task: done.id.clone(),
                    agent: done.agent_id.clone(),
                });
                out.events.extend(settle(c, &done, limits)?);
                out.cancelled.push(done.id);
            }
        }
    }
    Ok(out)
}
