//! Running one claimed task: its agent's runtime (started on demand), its thread's folder and
//! provider session (restored when the provider can), the turn's prompt, the turn itself (with
//! its cancellation, timeout and shutdown), its streamed output, and how it ended. Every way a
//! turn can end is mapped to a [`TurnEnd`] the store records ([`flow::end_turn`]); whether a
//! failure is retried depends on whether the prompt was sent and whether running it again could
//! repeat side effects.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::comms::{Caller, Comms};
use super::events::RuntimeEvent;
use super::flow::{self, Disposition, TurnEnd};
use super::ids::{AgentId, TaskId, ThreadId};
use super::model::{AgentMessage, DeliveryStatus, Isolation, Task, TaskStatus};
use super::prompt::{self, TurnInput};
use super::provider::{
    McpServer, ProviderError, ProviderEvent, SessionRequest, StopReason, ToolHost, TurnContext,
};
use super::runtime::Inner;
use super::store::tasks::TaskFilter;
use super::store::{agents, mail, tasks};
use super::workspace;

/// How long a cancelled turn's provider gets to stop before the turn is let go.
const CANCEL_GRACE: Duration = Duration::from_secs(15);

/// The communication tools, bound to the agent and task of a turn, for in-process providers.
struct BoundTools {
    comms: Comms,
    caller: Caller,
}

impl ToolHost for BoundTools {
    fn call(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> BoxFuture<'static, Result<serde_json::Value, String>> {
        let (comms, caller, name) = (self.comms.clone(), self.caller.clone(), name.to_string());
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                comms.call(&caller, &name, &args).map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
        })
    }
}

/// Runs the task's turn and records how it ended. The scheduler then gives back its capacity
/// ([`Inner::finished`]).
pub(crate) async fn run(inner: Arc<Inner>, task: Task, cancel: CancellationToken) {
    let started = std::time::Instant::now();
    let end = turn(&inner, &task, &cancel).await;
    log::info!(
        target: "orchestra",
        "task {} of agent {} ended after {:?}: {}",
        task.id,
        task.agent_id,
        started.elapsed(),
        match &end { TurnEnd::Done { .. } => "done", TurnEnd::Cancelled => "cancelled", TurnEnd::Failed { category, .. } => category }
    );
    let (id, limits) = (task.id.clone(), inner.config.limits.clone());
    match inner
        .store
        .run(move |s| s.tx(|c| flow::end_turn(c, &id, end, &limits)))
        .await
    {
        Ok((_, events)) => inner.publish(events),
        Err(e) => {
            log::warn!(target: "orchestra", "task {}: its end could not be recorded: {e}", task.id)
        }
    }
}

fn failed(error: impl Into<String>, category: &'static str, disposition: Disposition) -> TurnEnd {
    TurnEnd::Failed {
        error: error.into(),
        category,
        disposition,
    }
}

/// What a turn that broke after its prompt was sent may do: run again only when that changes
/// nothing outside the agent; otherwise a person reviews it.
fn after_sending(task: &Task) -> Disposition {
    if task.permission.side_effect_free() {
        Disposition::Retry
    } else {
        Disposition::Review
    }
}

async fn turn(inner: &Arc<Inner>, task: &Task, cancel: &CancellationToken) -> TurnEnd {
    let (agent_id, thread_id) = (task.agent_id.clone(), task.thread_id.clone());
    let loaded = inner
        .store
        .run({
            let (a, t) = (agent_id.clone(), thread_id.clone());
            move |s| {
                s.with(|c| {
                    Ok((
                        agents::get_agent(c, a.as_str())?.profile,
                        agents::get_thread(c, &t)?,
                    ))
                })
            }
        })
        .await;
    let (profile, thread) = match loaded {
        Ok(v) => v,
        Err(e) => return failed(e.to_string(), "store", Disposition::Fail),
    };
    let runtime = match inner.ensure_started(&profile).await {
        Ok(r) => r,
        Err(e) => return failed(e.to_string(), "provider_start", Disposition::Retry),
    };
    if cancel.is_cancelled() {
        return TurnEnd::Cancelled;
    }
    let cwd: PathBuf = match (profile.isolation, &thread.workdir) {
        (Isolation::Shared | Isolation::Free, _) => profile.workdir.clone(),
        (Isolation::Worktree, Some(dir)) => dir.clone(),
        (Isolation::Worktree, None) => {
            match workspace::worktree(&profile.workdir, &inner.config.worktrees_dir, &thread_id)
                .await
            {
                Ok(dir) => {
                    let (t, d) = (thread_id.clone(), dir.clone());
                    if let Err(e) = inner
                        .store
                        .run(move |s| s.with(|c| agents::set_thread_workdir(c, &t, &d)))
                        .await
                    {
                        return failed(e.to_string(), "store", Disposition::Fail);
                    }
                    dir
                }
                Err(e) => return failed(e.to_string(), "workspace", Disposition::Fail),
            }
        }
    };
    let caps = runtime.capabilities();
    let (session, new_session) = match inner.lifecycle.session(&agent_id, &thread_id) {
        Some(s) => (s, false),
        None => {
            let binding = {
                let t = thread_id.clone();
                inner
                    .store
                    .run(move |s| s.with(|c| agents::session_binding(c, &t)))
                    .await
                    .ok()
                    .flatten()
            };
            let cwd_text = cwd.to_string_lossy().into_owned();
            let restore = binding
                .filter(|(provider, _, bound_cwd)| {
                    caps.load_session && provider == &profile.provider && bound_cwd == &cwd_text
                })
                .map(|(_, id, _)| id);
            let mcp_servers = if caps.mcp_stdio {
                inner.bridge(&agent_id, &thread_id, &thread.token)
            } else {
                Vec::new()
            };
            let request = SessionRequest {
                thread: thread_id.clone(),
                cwd: cwd.clone(),
                restore,
                mcp_servers,
            };
            match runtime.open_session(request).await {
                Ok(s) => {
                    let (t, p, id) = (thread_id.clone(), profile.provider.clone(), s.id.clone());
                    let _ = inner
                        .store
                        .run(move |st| st.with(|c| agents::bind_session(c, &t, &p, &id, &cwd_text)))
                        .await;
                    inner
                        .lifecycle
                        .set_session(&agent_id, &thread_id, Some(s.clone()));
                    inner.publish(vec![RuntimeEvent::SessionStarted {
                        agent: agent_id.clone(),
                        thread: thread_id.clone(),
                        session: s.id.clone(),
                        restored: s.restored,
                    }]);
                    let fresh = !s.restored;
                    (s, fresh)
                }
                Err(ProviderError::Disconnected(why)) => {
                    inner.runtime_died(&agent_id, &profile.provider, &why);
                    return failed(why, "provider_disconnected", Disposition::Retry);
                }
                Err(e) => return failed(e.to_string(), "session", Disposition::Retry),
            }
        }
    };
    // Take what arrived for the task and mark the prompt sent, in one transaction.
    let prepared = {
        let (id, t) = (task.id.clone(), thread_id.clone());
        inner
            .store
            .run(move |s| {
                s.tx(|c| {
                    let now = tasks::get_task(c, &id)?;
                    if now.cancel_requested {
                        return Ok(None);
                    }
                    let pending = mail::pending_for_task(c, &id)?;
                    let ids: Vec<_> = pending.iter().map(|(d, _)| d.id.clone()).collect();
                    mail::set_deliveries(
                        c,
                        &ids,
                        &[DeliveryStatus::Pending],
                        DeliveryStatus::Claimed,
                        None,
                    )?;
                    let inbox: Vec<AgentMessage> = pending.into_iter().map(|(_, m)| m).collect();
                    let earlier = if new_session {
                        let filter = TaskFilter {
                            thread: Some(t),
                            statuses: vec![TaskStatus::Completed, TaskStatus::Failed],
                            limit: Some(5),
                            ..Default::default()
                        };
                        tasks::list_tasks(c, &filter)?
                            .into_iter()
                            .filter(|e| e.id != id)
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let now = tasks::mark_running(c, &id)?;
                    Ok(Some((now, inbox, earlier)))
                })
            })
            .await
    };
    let (task, inbox, earlier) = match prepared {
        Ok(Some(v)) => v,
        Ok(None) => return TurnEnd::Cancelled,
        Err(e) => return failed(e.to_string(), "store", Disposition::Retry),
    };
    let text = prompt::build(&TurnInput {
        profile: &profile,
        task: &task,
        inbox: &inbox,
        new_session,
        earlier: &earlier,
        tools: caps.mcp_stdio && inner.config.mcp_bridge.is_some() || !caps.mcp_stdio,
        cwd: &cwd,
    });
    inner.publish(vec![RuntimeEvent::TaskStarted {
        task: task.id.clone(),
        agent: agent_id.clone(),
        thread: thread_id.clone(),
        attempt: task.attempts,
    }]);
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    tokio::spawn(forward(
        inner.clone(),
        events_rx,
        task.id.clone(),
        agent_id.clone(),
        thread_id.clone(),
    ));
    let ctx = TurnContext {
        agent: agent_id.clone(),
        task: task.id.clone(),
        thread: thread_id.clone(),
        root: cwd.clone(),
        permission: task.permission,
        gate: inner.config.gate.clone(),
        tools: Arc::new(BoundTools {
            comms: inner.comms(),
            caller: Caller::agent(agent_id.clone(), Some(task.id.clone())),
        }),
        events: events_tx,
        cancel: cancel.clone(),
    };
    let limit = task
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(inner.config.limits.default_timeout);
    inner.lifecycle.begin_turn(&agent_id);
    let gave_up = async {
        cancel.cancelled().await;
        tokio::time::sleep(CANCEL_GRACE).await;
    };
    let result = tokio::select! {
        r = runtime.prompt(&session, text, ctx) => Ok(r),
        _ = tokio::time::sleep(limit) => Err("timeout"),
        _ = gave_up => Err("cancel_unacknowledged"),
        _ = inner.shutdown.cancelled() => Err("shutdown"),
    };
    inner.lifecycle.end_turn(&agent_id);
    match result {
        Ok(Ok(outcome)) if cancel.is_cancelled() => {
            let _ = outcome;
            TurnEnd::Cancelled
        }
        Ok(Ok(outcome)) => match outcome.stop {
            StopReason::EndTurn => TurnEnd::Done { text: outcome.text },
            StopReason::MaxTokens | StopReason::MaxTurnRequests => TurnEnd::Done {
                text: format!(
                    "{}\n\n[the agent stopped at its limit: {:?}]",
                    outcome.text, outcome.stop
                ),
            },
            StopReason::Refusal => {
                failed("the agent refused the task", "refusal", Disposition::Fail)
            }
            StopReason::Cancelled => failed(
                "the agent cancelled the turn itself",
                "provider_cancelled",
                Disposition::Fail,
            ),
            StopReason::Other(why) => failed(
                format!("the turn stopped: {why}"),
                "provider_stop",
                Disposition::Fail,
            ),
        },
        Ok(Err(ProviderError::Disconnected(why))) => {
            inner.runtime_died(&agent_id, &profile.provider, &why);
            if cancel.is_cancelled() {
                return TurnEnd::Cancelled;
            }
            failed(why, "provider_disconnected", after_sending(&task))
        }
        Ok(Err(e)) => failed(e.to_string(), "provider_error", Disposition::Fail),
        Err("cancel_unacknowledged") => {
            log::warn!(target: "orchestra", "task {}: the provider didn't stop within {CANCEL_GRACE:?}; restarting it", task.id);
            inner
                .stop_runtime(&agent_id, "it didn't acknowledge a cancellation")
                .await;
            TurnEnd::Cancelled
        }
        Err(why) => {
            let _ = runtime.cancel(&session).await;
            let disposition = after_sending(&task);
            let category = if why == "timeout" {
                "timeout"
            } else {
                "shutdown"
            };
            failed(
                format!("the turn was stopped: {category}"),
                category,
                disposition,
            )
        }
    }
}

/// Publishes a turn's streamed output, a burst of pieces at a time, so a chatty provider makes
/// a few events rather than thousands.
async fn forward(
    inner: Arc<Inner>,
    mut rx: mpsc::UnboundedReceiver<ProviderEvent>,
    task: TaskId,
    agent: AgentId,
    thread: ThreadId,
) {
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while batch.len() < 512 {
            match rx.try_recv() {
                Ok(e) => batch.push(e),
                Err(_) => break,
            }
        }
        let mut out: Vec<(String, String)> = Vec::new();
        for e in batch {
            let (kind, text) = match e {
                ProviderEvent::Message(t) => ("message", t),
                ProviderEvent::Thought(t) => ("thought", t),
                ProviderEvent::ToolCall {
                    title,
                    kind,
                    status,
                    ..
                } => ("tool", format!("{title} ({kind}, {status})")),
                ProviderEvent::ToolCallUpdate { id, status } => {
                    ("tool", format!("{id}: {}", status.unwrap_or_default()))
                }
                ProviderEvent::Plan(p) => ("plan", p.to_string()),
                ProviderEvent::Other(v) => ("other", v.to_string()),
            };
            match out.last_mut() {
                Some((k, t)) if k == kind && matches!(kind, "message" | "thought") => {
                    t.push_str(&text)
                }
                _ => out.push((kind.to_string(), text)),
            }
        }
        inner.publish(
            out.into_iter()
                .map(|(kind, text)| RuntimeEvent::TaskProgress {
                    task: task.clone(),
                    agent: agent.clone(),
                    thread: thread.clone(),
                    kind,
                    text,
                })
                .collect(),
        );
    }
}

/// The MCP server that gives an ACP agent's session the communication tools: `sawit-mcp`,
/// told which agent and thread it serves and the thread's secret.
pub(crate) fn bridge_server(
    launch: &super::model::Launch,
    agent: &AgentId,
    thread: &ThreadId,
    token: &str,
) -> McpServer {
    let mut env: Vec<(String, String)> = launch
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.push((ENV_AGENT.into(), agent.to_string()));
    env.push((ENV_THREAD.into(), thread.to_string()));
    env.push((ENV_TOKEN.into(), token.to_string()));
    McpServer {
        name: "sawit".into(),
        command: launch.command.clone(),
        args: launch.args.clone(),
        env,
    }
}

/// The environment variables that tell `sawit-mcp` it serves a runtime agent.
pub const ENV_AGENT: &str = "SAWIT_AGENT_ID";
pub const ENV_THREAD: &str = "SAWIT_AGENT_THREAD";
pub const ENV_TOKEN: &str = "SAWIT_AGENT_TOKEN";
