//! Messages (immutable) and their deliveries (the state of each, kept apart from the message).
//! A delivery names the task that takes the message in: a message task made for it, or the
//! waiting parent task a delegated result goes back to.

use rusqlite::{Connection, OptionalExtension, params};

use super::{bad_column, from_json, to_json};
use crate::orchestra::ids::{AgentId, ConversationId, DeliveryId, MessageId, TaskId, now_ms};
use crate::orchestra::model::{AgentMessage, Delivery, DeliveryStatus};
use crate::orchestra::{OrchestraError, Result};

const MSG_COLS: &str =
    "id, sender, recipient, conversation_id, correlation_id, task_id, payload, created_at";

fn row_to_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentMessage> {
    let payload: String = r.get(6)?;
    Ok(AgentMessage {
        id: MessageId(r.get(0)?),
        sender: r.get::<_, Option<String>>(1)?.map(AgentId),
        recipient: AgentId(r.get(2)?),
        conversation_id: ConversationId(r.get(3)?),
        correlation_id: r.get::<_, Option<String>>(4)?.map(MessageId),
        task_id: r.get::<_, Option<String>>(5)?.map(TaskId),
        payload: from_json(&payload)?,
        created_at: r.get(7)?,
    })
}

pub fn insert_message(c: &Connection, m: &AgentMessage) -> Result<()> {
    c.execute(
        "INSERT INTO messages (id, sender, recipient, conversation_id, correlation_id, task_id,
            payload, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            m.id.as_str(),
            m.sender.as_ref().map(|s| s.as_str()),
            m.recipient.as_str(),
            m.conversation_id.as_str(),
            m.correlation_id.as_ref().map(|c| c.as_str()),
            m.task_id.as_ref().map(|t| t.as_str()),
            to_json(&m.payload)?,
            m.created_at
        ],
    )?;
    Ok(())
}

pub fn get_message(c: &Connection, id: &MessageId) -> Result<AgentMessage> {
    c.query_row(
        &format!("SELECT {MSG_COLS} FROM messages WHERE id = ?1"),
        [id.as_str()],
        row_to_message,
    )
    .optional()?
    .ok_or_else(|| OrchestraError::NotFound(format!("no message {id}")))
}

/// Messages to or from `agent`, newest first.
pub fn agent_messages(c: &Connection, agent: &AgentId, limit: u32) -> Result<Vec<AgentMessage>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {MSG_COLS} FROM messages WHERE recipient = ?1 OR sender = ?1 ORDER BY seq DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![agent.as_str(), limit.min(1000)], row_to_message)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A conversation's messages, oldest first.
pub fn conversation(c: &Connection, conv: &ConversationId) -> Result<Vec<AgentMessage>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {MSG_COLS} FROM messages WHERE conversation_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map([conv.as_str()], row_to_message)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

const DLV_COLS: &str =
    "id, message_id, recipient, task_id, status, attempts, last_error, updated_at";

fn row_to_delivery(r: &rusqlite::Row<'_>) -> rusqlite::Result<Delivery> {
    let status: String = r.get(4)?;
    Ok(Delivery {
        id: DeliveryId(r.get(0)?),
        message_id: MessageId(r.get(1)?),
        recipient: AgentId(r.get(2)?),
        task_id: r.get::<_, Option<String>>(3)?.map(TaskId),
        status: DeliveryStatus::parse(&status)
            .ok_or_else(|| bad_column("delivery status", &status))?,
        attempts: r.get(5)?,
        last_error: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

pub fn insert_delivery(c: &Connection, d: &Delivery) -> Result<()> {
    let now = now_ms();
    c.execute(
        "INSERT INTO deliveries (id, message_id, recipient, task_id, status, attempts, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
        params![
            d.id.as_str(),
            d.message_id.as_str(),
            d.recipient.as_str(),
            d.task_id.as_ref().map(|t| t.as_str()),
            d.status.name(),
            now
        ],
    )?;
    Ok(())
}

pub fn get_delivery(c: &Connection, id: &DeliveryId) -> Result<Delivery> {
    c.query_row(
        &format!("SELECT {DLV_COLS} FROM deliveries WHERE id = ?1"),
        [id.as_str()],
        row_to_delivery,
    )
    .optional()?
    .ok_or_else(|| OrchestraError::NotFound(format!("no delivery {id}")))
}

pub fn message_deliveries(c: &Connection, msg: &MessageId) -> Result<Vec<Delivery>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {DLV_COLS} FROM deliveries WHERE message_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map([msg.as_str()], row_to_delivery)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The pending deliveries for a task, oldest first, with their messages.
pub fn pending_for_task(c: &Connection, task: &TaskId) -> Result<Vec<(Delivery, AgentMessage)>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {} FROM deliveries d WHERE d.task_id = ?1 AND d.status = 'pending' ORDER BY d.seq",
        DLV_COLS
            .split(", ")
            .map(|c| format!("d.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let rows: Vec<Delivery> = stmt
        .query_map([task.as_str()], row_to_delivery)?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter()
        .map(|d| {
            let m = get_message(c, &d.message_id)?;
            Ok((d, m))
        })
        .collect()
}

/// Moves deliveries from one of `from` to `to`, counting an attempt when they are claimed.
pub fn set_deliveries(
    c: &Connection,
    ids: &[DeliveryId],
    from: &[DeliveryStatus],
    to: DeliveryStatus,
    error: Option<&str>,
) -> Result<usize> {
    let now = now_ms();
    let mut n = 0;
    for id in ids {
        for f in from {
            n += c.execute(
                "UPDATE deliveries SET status = ?2, updated_at = ?3, last_error = coalesce(?4, last_error),
                    attempts = attempts + (?2 = 'claimed')
                 WHERE id = ?1 AND status = ?5",
                params![id.as_str(), to.name(), now, error, f.name()],
            )?;
        }
    }
    Ok(n)
}

/// Every delivery a task holds in `status`.
pub fn task_deliveries(
    c: &Connection,
    task: &TaskId,
    status: DeliveryStatus,
) -> Result<Vec<DeliveryId>> {
    let mut stmt =
        c.prepare("SELECT id FROM deliveries WHERE task_id = ?1 AND status = ?2 ORDER BY seq")?;
    let rows = stmt.query_map(params![task.as_str(), status.name()], |r| {
        Ok(DeliveryId(r.get(0)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Deliveries waiting in an agent's mailbox.
pub fn pending_count(c: &Connection, agent: &AgentId) -> Result<u32> {
    Ok(c.query_row(
        "SELECT count(*) FROM deliveries WHERE recipient = ?1 AND status IN ('pending', 'claimed')",
        [agent.as_str()],
        |r| r.get(0),
    )?)
}
