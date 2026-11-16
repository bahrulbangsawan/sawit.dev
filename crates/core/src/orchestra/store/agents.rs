//! Agents, their threads, the provider sessions bound to threads, and the scheduler's lease.

use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, params};

use super::{from_json, to_json};
use crate::orchestra::ids::{AgentId, ConversationId, ThreadId, Timestamp, now_ms, random_hex};
use crate::orchestra::model::{AgentProfile, AgentState};
use crate::orchestra::{OrchestraError, Result};

/// An agent as stored: its profile and the runtime state the scheduler last recorded.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentRow {
    pub profile: AgentProfile,
    pub state: String,
    pub state_detail: Option<String>,
}

pub fn insert_agent(c: &Connection, p: &AgentProfile) -> Result<()> {
    let taken: Option<String> = c
        .query_row(
            "SELECT id FROM agents WHERE (id = ?1 OR name = ?2) AND deleted_at IS NULL",
            params![p.id.as_str(), p.name],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = taken {
        return Err(OrchestraError::Conflict(format!(
            "an agent with this id or name exists already ({id})"
        )));
    }
    let gone: bool = c
        .query_row(
            "SELECT 1 FROM agents WHERE id = ?1",
            [p.id.as_str()],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if gone {
        return Err(OrchestraError::Conflict(format!(
            "the id {} belonged to a deleted agent; pick another",
            p.id
        )));
    }
    c.execute(
        "INSERT INTO agents (id, name, profile, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![p.id.as_str(), p.name, to_json(p)?, p.enabled, p.created_at],
    )?;
    Ok(())
}

pub fn update_agent(c: &Connection, p: &AgentProfile) -> Result<()> {
    let n = c.execute(
        "UPDATE agents SET name = ?2, profile = ?3, enabled = ?4, updated_at = ?5
         WHERE id = ?1 AND deleted_at IS NULL",
        params![p.id.as_str(), p.name, to_json(p)?, p.enabled, p.updated_at],
    )?;
    if n == 0 {
        return Err(OrchestraError::NotFound(format!("no agent {}", p.id)));
    }
    Ok(())
}

/// Marks the agent deleted. Its tasks and messages stay, for history.
pub fn delete_agent(c: &Connection, id: &AgentId) -> Result<()> {
    let open: i64 = c.query_row(
        "SELECT count(*) FROM tasks WHERE agent_id = ?1
         AND status NOT IN ('completed', 'failed', 'cancelled')",
        [id.as_str()],
        |r| r.get(0),
    )?;
    if open > 0 {
        return Err(OrchestraError::Conflict(format!(
            "agent {id} has {open} unfinished task(s): cancel them first"
        )));
    }
    let n = c.execute(
        "UPDATE agents SET deleted_at = ?2, enabled = 0 WHERE id = ?1 AND deleted_at IS NULL",
        params![id.as_str(), now_ms()],
    )?;
    if n == 0 {
        return Err(OrchestraError::NotFound(format!("no agent {id}")));
    }
    c.execute(
        "UPDATE deliveries SET status = 'failed', last_error = 'recipient deleted', updated_at = ?2
         WHERE recipient = ?1 AND status IN ('pending', 'claimed')",
        params![id.as_str(), now_ms()],
    )?;
    Ok(())
}

fn row_to_agent(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRow> {
    let profile: String = r.get(0)?;
    let mut profile: AgentProfile = from_json(&profile)?;
    profile.enabled = r.get(1)?;
    Ok(AgentRow {
        profile,
        state: r.get(2)?,
        state_detail: r.get(3)?,
    })
}

/// The agent `key` names, by id or by name; deleted agents are not found.
pub fn get_agent(c: &Connection, key: &str) -> Result<AgentRow> {
    c.query_row(
        "SELECT profile, enabled, state, state_detail FROM agents
         WHERE (id = ?1 OR name = ?1) AND deleted_at IS NULL ORDER BY id = ?1 DESC LIMIT 1",
        [key],
        row_to_agent,
    )
    .optional()?
    .ok_or_else(|| OrchestraError::NotFound(format!("no agent `{key}`")))
}

/// Every agent that isn't deleted, by name.
pub fn list_agents(c: &Connection) -> Result<Vec<AgentRow>> {
    let mut stmt = c.prepare(
        "SELECT profile, enabled, state, state_detail FROM agents
         WHERE deleted_at IS NULL ORDER BY name",
    )?;
    let rows = stmt.query_map([], row_to_agent)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Records the runtime state the scheduler sees, for other processes and after a restart.
pub fn record_state(
    c: &Connection,
    id: &AgentId,
    state: AgentState,
    detail: Option<&str>,
) -> Result<()> {
    c.execute(
        "UPDATE agents SET state = ?2, state_detail = ?3 WHERE id = ?1",
        params![id.as_str(), state.name(), detail],
    )?;
    Ok(())
}

/// A thread of an agent, as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadRow {
    pub id: ThreadId,
    pub agent_id: AgentId,
    pub conversation_id: Option<ConversationId>,
    pub kind: String,
    pub title: String,
    /// The secret its `sawit-mcp` proves the thread's identity with.
    pub token: String,
    /// Its own worktree, for an isolated agent.
    pub workdir: Option<PathBuf>,
}

fn row_to_thread(r: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadRow> {
    Ok(ThreadRow {
        id: ThreadId(r.get(0)?),
        agent_id: AgentId(r.get(1)?),
        conversation_id: r.get::<_, Option<String>>(2)?.map(ConversationId),
        kind: r.get(3)?,
        title: r.get(4)?,
        token: r.get(5)?,
        workdir: r.get::<_, Option<String>>(6)?.map(PathBuf::from),
    })
}

const THREAD_COLS: &str = "id, agent_id, conversation_id, kind, title, token, workdir";

pub fn get_thread(c: &Connection, id: &ThreadId) -> Result<ThreadRow> {
    c.query_row(
        &format!("SELECT {THREAD_COLS} FROM threads WHERE id = ?1"),
        [id.as_str()],
        row_to_thread,
    )
    .optional()?
    .ok_or_else(|| OrchestraError::NotFound(format!("no thread {id}")))
}

pub fn agent_threads(c: &Connection, agent: &AgentId) -> Result<Vec<ThreadRow>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {THREAD_COLS} FROM threads WHERE agent_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([agent.as_str()], row_to_thread)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn insert_thread(c: &Connection, t: &ThreadRow) -> Result<()> {
    c.execute(
        "INSERT INTO threads (id, agent_id, conversation_id, kind, title, token, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            t.id.as_str(),
            t.agent_id.as_str(),
            t.conversation_id.as_ref().map(|c| c.as_str()),
            t.kind,
            t.title,
            t.token,
            now_ms()
        ],
    )?;
    Ok(())
}

/// The agent's main thread, made the first time it is asked for.
pub fn main_thread(c: &Connection, agent: &AgentId) -> Result<ThreadRow> {
    let found = c
        .query_row(
            &format!("SELECT {THREAD_COLS} FROM threads WHERE agent_id = ?1 AND kind = 'main'"),
            [agent.as_str()],
            row_to_thread,
        )
        .optional()?;
    if let Some(t) = found {
        return Ok(t);
    }
    let t = ThreadRow {
        id: ThreadId::generate(),
        agent_id: agent.clone(),
        conversation_id: None,
        kind: "main".into(),
        title: "Main".into(),
        token: random_hex(16),
        workdir: None,
    };
    insert_thread(c, &t)?;
    Ok(t)
}

/// A new side thread of the agent (a delegated task's own lane).
pub fn side_thread(c: &Connection, agent: &AgentId, title: &str) -> Result<ThreadRow> {
    let t = ThreadRow {
        id: ThreadId::generate(),
        agent_id: agent.clone(),
        conversation_id: None,
        kind: "side".into(),
        title: title.chars().take(120).collect(),
        token: random_hex(16),
        workdir: None,
    };
    insert_thread(c, &t)?;
    Ok(t)
}

/// The agent's thread for a conversation, made the first time: a message and its replies land
/// in the same thread of each side.
pub fn conversation_thread(
    c: &Connection,
    agent: &AgentId,
    conv: &ConversationId,
) -> Result<ThreadRow> {
    let found = c
        .query_row(
            &format!(
                "SELECT {THREAD_COLS} FROM threads WHERE agent_id = ?1 AND conversation_id = ?2"
            ),
            params![agent.as_str(), conv.as_str()],
            row_to_thread,
        )
        .optional()?;
    if let Some(t) = found {
        return Ok(t);
    }
    let t = ThreadRow {
        id: ThreadId::generate(),
        agent_id: agent.clone(),
        conversation_id: Some(conv.clone()),
        kind: "conversation".into(),
        title: format!("Conversation {conv}"),
        token: random_hex(16),
        workdir: None,
    };
    insert_thread(c, &t)?;
    Ok(t)
}

pub fn set_thread_workdir(c: &Connection, id: &ThreadId, dir: &std::path::Path) -> Result<()> {
    c.execute(
        "UPDATE threads SET workdir = ?2 WHERE id = ?1",
        params![id.as_str(), dir.to_string_lossy()],
    )?;
    Ok(())
}

/// The provider session bound to a thread: `(provider, session id, cwd)`.
pub fn session_binding(
    c: &Connection,
    thread: &ThreadId,
) -> Result<Option<(String, String, String)>> {
    Ok(c.query_row(
        "SELECT provider, session_id, cwd FROM provider_sessions WHERE thread_id = ?1",
        [thread.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()?)
}

pub fn bind_session(
    c: &Connection,
    thread: &ThreadId,
    provider: &str,
    session: &str,
    cwd: &str,
) -> Result<()> {
    let now = now_ms();
    c.execute(
        "INSERT INTO provider_sessions (thread_id, provider, session_id, cwd, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(thread_id) DO UPDATE SET provider = ?2, session_id = ?3, cwd = ?4, updated_at = ?5",
        params![thread.as_str(), provider, session, cwd, now],
    )?;
    Ok(())
}

pub fn unbind_session(c: &Connection, thread: &ThreadId) -> Result<()> {
    c.execute(
        "DELETE FROM provider_sessions WHERE thread_id = ?1",
        [thread.as_str()],
    )?;
    Ok(())
}

/// Takes or renews the lease `name` for `owner` until `now + ttl_ms`. True when `owner` holds it.
pub fn take_lease(
    c: &Connection,
    name: &str,
    owner: &str,
    now: Timestamp,
    ttl_ms: i64,
) -> Result<bool> {
    c.execute(
        "INSERT INTO leases (name, owner, expires_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET owner = ?2, expires_at = ?3
         WHERE leases.owner = ?2 OR leases.expires_at < ?4",
        params![name, owner, now + ttl_ms, now],
    )?;
    let holder: String = c.query_row("SELECT owner FROM leases WHERE name = ?1", [name], |r| {
        r.get(0)
    })?;
    Ok(holder == owner)
}

pub fn release_lease(c: &Connection, name: &str, owner: &str) -> Result<()> {
    c.execute(
        "DELETE FROM leases WHERE name = ?1 AND owner = ?2",
        params![name, owner],
    )?;
    Ok(())
}
