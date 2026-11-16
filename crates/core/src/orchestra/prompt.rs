//! The text of a turn: what the task asks, what arrived for it (results of the work it
//! delegated, a message), and, when the provider session is new, who the agent is, what it may
//! use, and a recap of its thread's earlier work when the old session couldn't be restored.

use std::fmt::Write as _;

use super::model::{AgentMessage, AgentProfile, MessagePayload, Task, TaskPayload};

/// Earlier tasks of the thread recapped in a new session, at most.
const RECAP_TASKS: usize = 5;
/// Characters of each earlier result recapped.
const RECAP_CHARS: usize = 600;

/// What a turn is built from.
pub struct TurnInput<'a> {
    pub profile: &'a AgentProfile,
    pub task: &'a Task,
    /// Messages delivered to this task (for a message task, its message first).
    pub inbox: &'a [AgentMessage],
    /// The session is new: introduce the agent.
    pub new_session: bool,
    /// Earlier finished tasks of the thread, newest first, for a new session on an old thread.
    pub earlier: &'a [Task],
    /// The communication tools are reachable (the MCP bridge or in-process).
    pub tools: bool,
    pub cwd: &'a std::path::Path,
}

fn result_text(t: &Task) -> String {
    t.result
        .as_ref()
        .and_then(|r| r.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .chars()
        .take(RECAP_CHARS)
        .collect()
}

pub fn build(input: &TurnInput<'_>) -> String {
    let p = input.profile;
    let t = input.task;
    let mut out = String::new();
    if input.new_session {
        let _ = writeln!(
            out,
            "You are {} ({}), an agent in Sawit's agent runtime.",
            p.name, p.id
        );
        if !p.role.is_empty() {
            let _ = writeln!(out, "Your role: {}.", p.role);
        }
        if !p.instructions.trim().is_empty() {
            let _ = writeln!(out, "\n{}\n", p.instructions.trim());
        }
        let _ = writeln!(out, "You work in {}.", input.cwd.display());
        if input.tools {
            out.push_str(
                "Other agents can help: the `sawit` MCP server gives you list_agents, send_message, \
                 delegate_task, get_task_status, list_agent_tasks and cancel_task. A task you delegate \
                 runs on its own; end your turn when you have handed work off, and its result comes \
                 back to you in this conversation when it is done.\n",
            );
        }
        if !input.earlier.is_empty() {
            out.push_str(
                "\nEarlier work in this conversation (its full history couldn't be restored):\n",
            );
            for e in input.earlier.iter().take(RECAP_TASKS).rev() {
                let _ = writeln!(
                    out,
                    "- {} [{}]: {}",
                    e.title,
                    e.status.name(),
                    result_text(e)
                );
            }
        }
        out.push('\n');
    }
    let _ = writeln!(out, "Task {} — {}", t.id, t.title);
    match &t.payload {
        TaskPayload::Prompt { text } => {
            if !resumed(t) || input.new_session {
                let _ = writeln!(out, "\n{text}");
            }
        }
        TaskPayload::Delegated { instructions, .. } => {
            if !resumed(t) || input.new_session {
                let who = t
                    .requested_by
                    .as_ref()
                    .map_or("the user".to_string(), |a| a.to_string());
                let _ = writeln!(out, "Delegated to you by {who}.\n\n{instructions}");
                out.push_str(
                    "\nWhen you are done, answer with your result: what you did, the files you \
                     changed, and anything left to follow up.\n",
                );
            }
        }
        TaskPayload::Message { .. } => {}
    }
    let mut results = Vec::new();
    for m in input.inbox {
        match &m.payload {
            MessagePayload::Text { content } => {
                let from = m
                    .sender
                    .as_ref()
                    .map_or("the user".to_string(), |a| a.to_string());
                let _ = writeln!(
                    out,
                    "\nMessage {} from {from}:\n{content}\n(To reply, call send_message with recipient \"{from}\" and correlation_id \"{}\".)",
                    m.id, m.id
                );
            }
            MessagePayload::Result {
                task,
                status,
                summary,
                error,
            } => {
                let mut line = format!(
                    "- task {task} by {} [{status}]",
                    m.sender.as_ref().map_or("?", |s| s.as_str())
                );
                if !summary.is_empty() {
                    let _ = write!(line, ": {summary}");
                }
                if let Some(e) = error {
                    let _ = write!(line, " (error: {e})");
                }
                results.push(line);
            }
            // The task's own instructions already say it.
            MessagePayload::Delegation { .. } => {}
        }
    }
    if !results.is_empty() {
        out.push_str("\nResults of the work you delegated:\n");
        out.push_str(&results.join("\n"));
        out.push_str("\n\nReview them and continue the task; delegate more if needed, or give your final answer.\n");
    }
    out
}

/// Whether the task ran a turn before (it is resuming after its delegated work, not starting).
fn resumed(t: &Task) -> bool {
    t.result.is_some()
}
