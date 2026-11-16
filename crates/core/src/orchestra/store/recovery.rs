//! Recovery after a restart: the work a stopped runtime left claimed or running is queued again
//! when that is safe (never sent, or free of side effects), or held for review.

use rusqlite::Connection;

use super::tasks::{TaskFilter, list_tasks, requeue, set_outcome, transition};
use crate::orchestra::Result;
use crate::orchestra::ids::{TaskId, now_ms};
use crate::orchestra::model::TaskStatus;

/// What recovery did after a restart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovered {
    /// Claimed but never sent: queued again.
    pub requeued: Vec<TaskId>,
    /// Sent, and replaying them changes nothing: queued again.
    pub retried: Vec<TaskId>,
    /// Sent, and they may have changed things: held for review.
    pub interrupted: Vec<TaskId>,
    /// Deliveries a lost turn had taken, pending again.
    pub deliveries: usize,
}

/// Puts the work a stopped runtime left behind back in order. Run it in one transaction while
/// holding the scheduler lease, before scheduling anything.
pub fn recover(c: &Connection) -> Result<Recovered> {
    let mut out = Recovered::default();
    let open = list_tasks(
        c,
        &TaskFilter {
            statuses: vec![TaskStatus::Starting, TaskStatus::Running],
            limit: Some(1000),
            ..Default::default()
        },
    )?;
    for t in open {
        let cancelled = t.cancel_requested;
        if cancelled {
            transition(
                c,
                &t.id,
                &[t.status],
                TaskStatus::Cancelled,
                Some("cancelled while the runtime was stopped"),
            )?;
        } else if t.status == TaskStatus::Starting || t.prompt_sent_at.is_none() {
            requeue(
                c,
                &t.id,
                &[t.status],
                None,
                "recovered: the prompt was never sent",
            )?;
            out.requeued.push(t.id);
        } else if t.permission.side_effect_free() && t.attempts < t.max_attempts {
            requeue(c, &t.id, &[t.status], None, "recovered: safe to run again")?;
            out.retried.push(t.id);
        } else {
            transition(
                c,
                &t.id,
                &[t.status],
                TaskStatus::Interrupted,
                Some("the runtime stopped mid-turn"),
            )?;
            set_outcome(
                c,
                &t.id,
                None,
                Some(
                    "interrupted: the runtime stopped while it ran; it may have changed files, so it waits for review",
                ),
                Some("interrupted"),
            )?;
            out.interrupted.push(t.id);
        }
    }
    out.deliveries = c.execute(
        "UPDATE deliveries SET status = 'pending', updated_at = ?1 WHERE status = 'claimed'",
        [now_ms()],
    )?;
    c.execute(
        "UPDATE agents SET state = 'registered', state_detail = NULL WHERE deleted_at IS NULL",
        [],
    )?;
    Ok(out)
}
