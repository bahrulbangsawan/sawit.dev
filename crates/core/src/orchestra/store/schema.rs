//! The store's versioned schema. Each migration runs once, in order, in one transaction, and
//! sets SQLite's `user_version` to its number; a store written by a newer Sawit is refused rather
//! than misread.

use rusqlite::Connection;

use crate::orchestra::{OrchestraError, Result};

const MIGRATIONS: &[&str] = &[
    // 1: agents, threads, provider sessions, tasks and their transitions, messages, deliveries,
    // and the scheduler's lease.
    "CREATE TABLE agents (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        profile TEXT NOT NULL,
        enabled INTEGER NOT NULL,
        state TEXT NOT NULL DEFAULT 'registered',
        state_detail TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        deleted_at INTEGER
    );
    CREATE UNIQUE INDEX agents_name ON agents(name) WHERE deleted_at IS NULL;

    CREATE TABLE threads (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL REFERENCES agents(id),
        conversation_id TEXT,
        kind TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        token TEXT NOT NULL,
        workdir TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX threads_agent ON threads(agent_id);
    CREATE UNIQUE INDEX threads_conversation ON threads(agent_id, conversation_id)
        WHERE conversation_id IS NOT NULL;
    CREATE UNIQUE INDEX threads_main ON threads(agent_id) WHERE kind = 'main';

    CREATE TABLE provider_sessions (
        thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
        provider TEXT NOT NULL,
        session_id TEXT NOT NULL,
        cwd TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE tasks (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL UNIQUE,
        agent_id TEXT NOT NULL REFERENCES agents(id),
        thread_id TEXT NOT NULL REFERENCES threads(id),
        parent_task_id TEXT REFERENCES tasks(id),
        root_task_id TEXT NOT NULL,
        depth INTEGER NOT NULL,
        requested_by TEXT REFERENCES agents(id),
        title TEXT NOT NULL,
        payload TEXT NOT NULL,
        status TEXT NOT NULL,
        permission TEXT NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0,
        max_attempts INTEGER NOT NULL,
        not_before INTEGER,
        timeout_ms INTEGER,
        idempotency_key TEXT,
        result TEXT,
        error TEXT,
        failure TEXT,
        cancel_requested INTEGER NOT NULL DEFAULT 0,
        claimed_by TEXT,
        claimed_at INTEGER,
        prompt_sent_at INTEGER,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        finished_at INTEGER
    );
    CREATE INDEX tasks_queue ON tasks(agent_id, status, seq);
    CREATE INDEX tasks_thread ON tasks(thread_id, status);
    CREATE INDEX tasks_parent ON tasks(parent_task_id);
    CREATE INDEX tasks_root ON tasks(root_task_id, status);
    CREATE INDEX tasks_open ON tasks(status)
        WHERE status IN ('queued', 'starting', 'running', 'waiting');
    CREATE UNIQUE INDEX tasks_idempotency ON tasks(coalesce(requested_by, ''), idempotency_key)
        WHERE idempotency_key IS NOT NULL;

    CREATE TABLE task_transitions (
        id INTEGER PRIMARY KEY,
        task_id TEXT NOT NULL REFERENCES tasks(id),
        from_status TEXT,
        to_status TEXT NOT NULL,
        note TEXT,
        at INTEGER NOT NULL
    );
    CREATE INDEX transitions_task ON task_transitions(task_id, id);

    CREATE TABLE messages (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL UNIQUE,
        sender TEXT REFERENCES agents(id),
        recipient TEXT NOT NULL REFERENCES agents(id),
        conversation_id TEXT NOT NULL,
        correlation_id TEXT REFERENCES messages(id),
        task_id TEXT REFERENCES tasks(id),
        payload TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX messages_conversation ON messages(conversation_id, seq);
    CREATE INDEX messages_recipient ON messages(recipient, seq);

    CREATE TABLE deliveries (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL UNIQUE,
        message_id TEXT NOT NULL REFERENCES messages(id),
        recipient TEXT NOT NULL REFERENCES agents(id),
        task_id TEXT REFERENCES tasks(id),
        status TEXT NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0,
        last_error TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX deliveries_pending ON deliveries(recipient, status, seq);
    CREATE INDEX deliveries_task ON deliveries(task_id, status, seq);

    CREATE TABLE leases (
        name TEXT PRIMARY KEY,
        owner TEXT NOT NULL,
        expires_at INTEGER NOT NULL
    );",
];

/// The schema version this build writes.
pub const VERSION: i64 = MIGRATIONS.len() as i64;

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > VERSION {
        return Err(OrchestraError::Store(format!(
            "the agent store is version {current}, newer than this Sawit reads ({VERSION}): update Sawit"
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
