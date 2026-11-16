//! Tasks: inserted, claimed atomically, moved through their states with every move recorded,
//! listed and recovered after a restart.

use rusqlite::{Connection, OptionalExtension, params};

use super::{bad_column, from_json, to_json};
use crate::orchestra::ids::{AgentId, TaskId, ThreadId, Timestamp, now_ms};
use crate::orchestra::model::{PermissionMode, Task, TaskPayload, TaskStatus};
use crate::orchestra::{OrchestraError, Result};

pub use super::recovery::{Recovered, recover};

const COLS: &str = "id, agent_id, thread_id, parent_task_id, root_task_id, depth, requested_by, \
    title, payload, status, permission, attempts, max_attempts, not_before, timeout_ms, \
    idempotency_key, result, error, failure, cancel_requested, prompt_sent_at, created_at, \
    updated_at, finished_at";

fn row_to_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let payload: String = r.get(8)?;
    let status: String = r.get(9)?;
    let permission: String = r.get(10)?;
    let result: Option<String> = r.get(16)?;
    Ok(Task {
        id: TaskId(r.get(0)?),
        agent_id: AgentId(r.get(1)?),
        thread_id: ThreadId(r.get(2)?),
        parent_task_id: r.get::<_, Option<String>>(3)?.map(TaskId),
        root_task_id: TaskId(r.get(4)?),
        depth: r.get(5)?,
        requested_by: r.get::<_, Option<String>>(6)?.map(AgentId),
        title: r.get(7)?,
        payload: from_json(&payload)?,
        status: TaskStatus::parse(&status).ok_or_else(|| bad_column("task status", &status))?,
        permission: PermissionMode::parse(&permission)
            .ok_or_else(|| bad_column("permission", &permission))?,
        attempts: r.get(11)?,
        max_attempts: r.get(12)?,
        not_before: r.get(13)?,
        timeout_ms: r.get::<_, Option<i64>>(14)?.map(|t| t.max(0) as u64),
        idempotency_key: r.get(15)?,
        result: result.as_deref().map(from_json).transpose()?,
        error: r.get(17)?,
        failure: r.get(18)?,
        cancel_requested: r.get(19)?,
        prompt_sent_at: r.get(20)?,
        created_at: r.get(21)?,
        updated_at: r.get(22)?,
        finished_at: r.get(23)?,
    })
}

/// A task to insert. Its id is chosen by the caller so the records made with it in the same
/// transaction (a delegation message) can name it.
#[derive(Clone, Debug)]
pub struct NewTaskRow {
    pub id: TaskId,
    pub agent_id: AgentId,
    pub thread_id: ThreadId,
    pub parent_task_id: Option<TaskId>,
    pub root_task_id: Option<TaskId>,
    pub depth: u32,
    pub requested_by: Option<AgentId>,
    pub title: String,
    pub payload: TaskPayload,
    pub permission: PermissionMode,
    pub max_attempts: u32,
    pub timeout_ms: Option<u64>,
    pub idempotency_key: Option<String>,
    pub held: bool,
}

/// Inserts the task, or returns the one inserted before with the same requester and idempotency
/// key (`false` then: nothing new was made).
pub fn insert_task(c: &Connection, t: &NewTaskRow) -> Result<(Task, bool)> {
    if let Some(key) = &t.idempotency_key {
        let found = c
            .query_row(
                &format!(
                    "SELECT {COLS} FROM tasks WHERE coalesce(requested_by, '') = ?1
                     AND idempotency_key = ?2"
                ),
                params![t.requested_by.as_ref().map_or("", |a| a.as_str()), key],
                row_to_task,
            )
            .optional()?;
        if let Some(found) = found {
            return Ok((found, false));
        }
    }
    let now = now_ms();
    let status = if t.held {
        TaskStatus::Held
    } else {
        TaskStatus::Queued
    };
    c.execute(
        "INSERT INTO tasks (id, agent_id, thread_id, parent_task_id, root_task_id, depth,
            requested_by, title, payload, status, permission, max_attempts, timeout_ms,
            idempotency_key, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
        params![
            t.id.as_str(),
            t.agent_id.as_str(),
            t.thread_id.as_str(),
            t.parent_task_id.as_ref().map(|p| p.as_str()),
            t.root_task_id.as_ref().unwrap_or(&t.id).as_str(),
            t.depth,
            t.requested_by.as_ref().map(|a| a.as_str()),
            t.title,
            to_json(&t.payload)?,
            status.name(),
            t.permission.name(),
            t.max_attempts.max(1),
            t.timeout_ms.map(|v| v as i64),
            t.idempotency_key,
            now
        ],
    )?;
    record(c, &t.id, None, status, Some("submitted"))?;
    Ok((get_task(c, &t.id)?, true))
}

pub fn get_task(c: &Connection, id: &TaskId) -> Result<Task> {
    c.query_row(
        &format!("SELECT {COLS} FROM tasks WHERE id = ?1"),
        [id.as_str()],
        row_to_task,
    )
    .optional()?
    .ok_or_else(|| OrchestraError::NotFound(format!("no task {id}")))
}

fn record(
    c: &Connection,
    id: &TaskId,
    from: Option<TaskStatus>,
    to: TaskStatus,
    note: Option<&str>,
) -> Result<()> {
    c.execute(
        "INSERT INTO task_transitions (task_id, from_status, to_status, note, at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.as_str(),
            from.map(|f| f.name()),
            to.name(),
            note,
            now_ms()
        ],
    )?;
    Ok(())
}

/// Moves the task to `to` if its status is one of `from`; `Conflict` otherwise, so a
/// concurrent move (a cancellation racing a completion) is never overwritten.
pub fn transition(
    c: &Connection,
    id: &TaskId,
    from: &[TaskStatus],
    to: TaskStatus,
    note: Option<&str>,
) -> Result<Task> {
    let task = get_task(c, id)?;
    if !from.contains(&task.status) {
        return Err(OrchestraError::Conflict(format!(
            "task {id} is {}, not {}",
            task.status.name(),
            from.iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(" or ")
        )));
    }
    let now = now_ms();
    let finished = to.is_terminal().then_some(now);
    c.execute(
        "UPDATE tasks SET status = ?2, updated_at = ?3, finished_at = coalesce(?4, finished_at)
         WHERE id = ?1 AND status = ?5",
        params![id.as_str(), to.name(), now, finished, task.status.name()],
    )?;
    record(c, id, Some(task.status), to, note)?;
    get_task(c, id)
}

/// Sets the outcome columns of a task (with its status moved by [`transition`] in the same
/// transaction).
pub fn set_outcome(
    c: &Connection,
    id: &TaskId,
    result: Option<&serde_json::Value>,
    error: Option<&str>,
    failure: Option<&str>,
) -> Result<()> {
    c.execute(
        "UPDATE tasks SET result = coalesce(?2, result), error = ?3, failure = ?4 WHERE id = ?1",
        params![
            id.as_str(),
            result.map(to_json).transpose()?,
            error,
            failure
        ],
    )?;
    Ok(())
}

/// Requeues the task for a retry no sooner than `not_before`.
pub fn requeue(
    c: &Connection,
    id: &TaskId,
    from: &[TaskStatus],
    not_before: Option<Timestamp>,
    note: &str,
) -> Result<Task> {
    transition(c, id, from, TaskStatus::Queued, Some(note))?;
    c.execute(
        "UPDATE tasks SET not_before = ?2, claimed_by = NULL, prompt_sent_at = NULL WHERE id = ?1",
        params![id.as_str(), not_before],
    )?;
    get_task(c, id)
}

/// Claims the agent's next runnable task for `owner`: the oldest queued task whose time has come,
/// in a thread with no turn running or waiting (one turn per thread, FIFO within it). Call it in
/// an `IMMEDIATE` transaction ([`super::Store::tx`]): the claim then can't race another
/// scheduler's.
pub fn claim_next(
    c: &Connection,
    agent: &AgentId,
    owner: &str,
    now: Timestamp,
) -> Result<Option<Task>> {
    let next: Option<String> = c
        .query_row(
            "SELECT t.id FROM tasks t WHERE t.agent_id = ?1 AND t.status = 'queued'
               AND (t.not_before IS NULL OR t.not_before <= ?2)
               AND NOT EXISTS (SELECT 1 FROM tasks o WHERE o.thread_id = t.thread_id
                   AND o.status IN ('starting', 'running', 'waiting'))
               AND NOT EXISTS (SELECT 1 FROM tasks e WHERE e.thread_id = t.thread_id
                   AND e.status = 'queued' AND e.seq < t.seq)
             ORDER BY t.seq LIMIT 1",
            params![agent.as_str(), now],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = next.map(TaskId) else {
        return Ok(None);
    };
    let n = c.execute(
        "UPDATE tasks SET status = 'starting', claimed_by = ?2, claimed_at = ?3,
            attempts = attempts + 1, updated_at = ?3
         WHERE id = ?1 AND status = 'queued'",
        params![id.as_str(), owner, now],
    )?;
    if n == 0 {
        return Ok(None);
    }
    record(
        c,
        &id,
        Some(TaskStatus::Queued),
        TaskStatus::Starting,
        Some(owner),
    )?;
    get_task(c, &id).map(Some)
}

/// The prompt went to the provider: from here a replay could repeat side effects.
pub fn mark_running(c: &Connection, id: &TaskId) -> Result<Task> {
    let task = transition(c, id, &[TaskStatus::Starting], TaskStatus::Running, None)?;
    c.execute(
        "UPDATE tasks SET prompt_sent_at = ?2 WHERE id = ?1",
        params![id.as_str(), now_ms()],
    )?;
    Ok(task)
}

pub fn request_cancel(c: &Connection, id: &TaskId) -> Result<()> {
    c.execute(
        "UPDATE tasks SET cancel_requested = 1, updated_at = ?2 WHERE id = ?1",
        params![id.as_str(), now_ms()],
    )?;
    Ok(())
}

/// What to list.
#[derive(Clone, Debug, Default)]
pub struct TaskFilter {
    pub agent: Option<AgentId>,
    pub requested_by: Option<AgentId>,
    pub thread: Option<ThreadId>,
    pub parent: Option<TaskId>,
    pub root: Option<TaskId>,
    pub statuses: Vec<TaskStatus>,
    pub limit: Option<u32>,
}

/// Tasks matching `f`, newest first.
pub fn list_tasks(c: &Connection, f: &TaskFilter) -> Result<Vec<Task>> {
    let mut sql = format!("SELECT {COLS} FROM tasks WHERE 1 = 1");
    let mut args: Vec<String> = Vec::new();
    let mut add = |sql: &mut String, cond: &str, v: String| {
        args.push(v);
        sql.push_str(&format!(" AND {cond} = ?{}", args.len()));
    };
    if let Some(a) = &f.agent {
        add(&mut sql, "agent_id", a.0.clone());
    }
    if let Some(a) = &f.requested_by {
        add(&mut sql, "requested_by", a.0.clone());
    }
    if let Some(t) = &f.thread {
        add(&mut sql, "thread_id", t.0.clone());
    }
    if let Some(p) = &f.parent {
        add(&mut sql, "parent_task_id", p.0.clone());
    }
    if let Some(r) = &f.root {
        add(&mut sql, "root_task_id", r.0.clone());
    }
    if !f.statuses.is_empty() {
        let names: Vec<String> = f
            .statuses
            .iter()
            .map(|s| format!("'{}'", s.name()))
            .collect();
        sql.push_str(&format!(" AND status IN ({})", names.join(", ")));
    }
    sql.push_str(&format!(
        " ORDER BY seq DESC LIMIT {}",
        f.limit.unwrap_or(100).min(1000)
    ));
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), row_to_task)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The task and every task delegated from it, at any depth, parents first.
pub fn subtree(c: &Connection, id: &TaskId) -> Result<Vec<Task>> {
    let mut stmt = c.prepare(&format!(
        "WITH RECURSIVE tree(id, lvl) AS (
            SELECT id, 0 FROM tasks WHERE id = ?1
            UNION ALL SELECT t.id, tree.lvl + 1 FROM tasks t JOIN tree ON t.parent_task_id = tree.id
         ) SELECT {} FROM tasks JOIN tree USING (id) ORDER BY tree.lvl, tasks.seq",
        COLS.split(", ")
            .map(|c| format!("tasks.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let rows = stmt.query_map([id.as_str()], row_to_task)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The agents along the chain of delegations that led to the task, the root task's first and
/// the task's own last.
pub fn chain_agents(c: &Connection, id: &TaskId) -> Result<Vec<AgentId>> {
    let mut stmt = c.prepare(
        "WITH RECURSIVE up(id, parent, agent, lvl) AS (
            SELECT id, parent_task_id, agent_id, 0 FROM tasks WHERE id = ?1
            UNION ALL SELECT t.id, t.parent_task_id, t.agent_id, up.lvl + 1
                FROM tasks t JOIN up ON t.id = up.parent WHERE up.lvl < 64
         ) SELECT agent FROM up ORDER BY lvl DESC",
    )?;
    let rows = stmt.query_map([id.as_str()], |r| Ok(AgentId(r.get(0)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Unfinished tasks delegated by `parent`.
pub fn open_children(c: &Connection, parent: &TaskId) -> Result<u32> {
    Ok(c.query_row(
        "SELECT count(*) FROM tasks WHERE parent_task_id = ?1
         AND status NOT IN ('completed', 'failed', 'cancelled')",
        [parent.as_str()],
        |r| r.get(0),
    )?)
}

/// Unfinished delegated tasks under the root task.
pub fn open_delegations(c: &Connection, root: &TaskId) -> Result<u32> {
    Ok(c.query_row(
        "SELECT count(*) FROM tasks WHERE root_task_id = ?1 AND id != ?1
         AND status NOT IN ('completed', 'failed', 'cancelled')",
        [root.as_str()],
        |r| r.get(0),
    )?)
}

pub fn queued_count(c: &Connection, agent: &AgentId) -> Result<u32> {
    Ok(c.query_row(
        "SELECT count(*) FROM tasks WHERE agent_id = ?1 AND status IN ('queued', 'held')",
        [agent.as_str()],
        |r| r.get(0),
    )?)
}

/// The ids of tasks claimed or running whose cancellation was asked for (by any process).
pub fn cancel_requested(c: &Connection) -> Result<Vec<TaskId>> {
    let mut stmt = c.prepare(
        "SELECT id FROM tasks WHERE cancel_requested = 1 AND status IN ('starting', 'running')",
    )?;
    let rows = stmt.query_map([], |r| Ok(TaskId(r.get(0)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The task a thread is running now, if any.
pub fn running_in_thread(c: &Connection, thread: &ThreadId) -> Result<Option<Task>> {
    Ok(c.query_row(
        &format!(
            "SELECT {COLS} FROM tasks WHERE thread_id = ?1 AND status IN ('starting', 'running')
             ORDER BY seq DESC LIMIT 1"
        ),
        [thread.as_str()],
        row_to_task,
    )
    .optional()?)
}

/// One recorded move of a task: `(from, to, note, at)`.
pub type Transition = (Option<String>, String, Option<String>, Timestamp);

/// The recorded moves of a task.
pub fn transitions(c: &Connection, id: &TaskId) -> Result<Vec<Transition>> {
    let mut stmt = c.prepare(
        "SELECT from_status, to_status, note, at FROM task_transitions WHERE task_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([id.as_str()], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
