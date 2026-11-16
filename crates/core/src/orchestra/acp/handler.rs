//! What the agent asks of the client during a turn: permission to use a tool (answered by the
//! task's permission mode, or the IDE's gate), and files read or written inside the agent's
//! folder only. Session updates stream to the turn's listener in the order they arrive.

use std::path::{Path, PathBuf};

use futures::future::BoxFuture;
use serde_json::{Value, json};

use super::client::{ClientHandler, lock};
use super::rpc::{Incoming, RpcError};
use crate::orchestra::policy::{self, Verdict};
use crate::orchestra::provider::{
    PermissionDecision, PermissionRequest, ProviderEvent, TurnContext,
};

impl ClientHandler {
    fn turn_ctx(&self, params: &Value) -> Result<TurnContext, RpcError> {
        let session = params
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::invalid_params("sessionId is missing"))?;
        lock(&self.turns)
            .get(session)
            .map(|t| t.ctx.clone())
            .ok_or_else(|| {
                RpcError::new(
                    RpcError::INVALID_REQUEST,
                    "no turn is running in this session",
                )
            })
    }
}

/// `path`, which must be absolute, if it lies inside `root` once links are resolved. For a
/// file that doesn't exist yet its folder is resolved instead.
fn inside(root: &Path, path: &str) -> Result<PathBuf, RpcError> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(RpcError::invalid_params("path must be absolute"));
    }
    let root = std::fs::canonicalize(root)
        .map_err(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))?;
    let resolved = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => {
            let parent = path
                .parent()
                .ok_or_else(|| RpcError::invalid_params("path has no folder"))?;
            let name = path
                .file_name()
                .ok_or_else(|| RpcError::invalid_params("path has no file name"))?;
            std::fs::canonicalize(parent)
                .map_err(|e| RpcError::invalid_params(format!("{}: {e}", parent.display())))?
                .join(name)
        }
    };
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err(RpcError::new(
            -32001,
            format!("{} is outside the agent's folder", path.display()),
        ))
    }
}

async fn decide(
    ctx: &TurnContext,
    kind: &str,
    title: &str,
    options: Vec<(String, String)>,
    raw: Value,
) -> PermissionDecision {
    if ctx.cancel.is_cancelled() {
        return PermissionDecision::Cancelled;
    }
    match policy::permission(ctx.permission, kind) {
        Verdict::Allow => PermissionDecision::Allow,
        Verdict::Reject => PermissionDecision::Reject,
        Verdict::Ask => {
            let request = PermissionRequest {
                agent: ctx.agent.clone(),
                task: ctx.task.clone(),
                tool_title: title.to_string(),
                tool_kind: kind.to_string(),
                options,
                raw,
            };
            tokio::select! {
                d = ctx.gate.ask(request) => d,
                _ = ctx.cancel.cancelled() => PermissionDecision::Cancelled,
            }
        }
    }
}

impl Incoming for ClientHandler {
    fn request(
        &self,
        method: String,
        params: Value,
    ) -> BoxFuture<'static, Result<Value, RpcError>> {
        const KNOWN: [&str; 3] = [
            "session/request_permission",
            "fs/read_text_file",
            "fs/write_text_file",
        ];
        if !KNOWN.contains(&method.as_str()) {
            return Box::pin(async move { Err(RpcError::method_not_found(&method)) });
        }
        let ctx = self.turn_ctx(&params);
        Box::pin(async move {
            let ctx = ctx?;
            match method.as_str() {
                "session/request_permission" => {
                    let call = params.get("toolCall").cloned().unwrap_or(Value::Null);
                    let kind = call
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("other")
                        .to_string();
                    let title = call
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("a tool")
                        .to_string();
                    let options: Vec<(String, String)> = params
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|o| {
                                    Some((
                                        o.get("optionId")?.as_str()?.to_string(),
                                        o.get("kind")?.as_str()?.to_string(),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let decision =
                        decide(&ctx, &kind, &title, options.clone(), params.clone()).await;
                    let pick = |kinds: &[&str]| {
                        kinds
                            .iter()
                            .find_map(|k| options.iter().find(|(_, ok)| ok == k))
                            .map(|(id, _)| id.clone())
                    };
                    let chosen = match decision {
                        PermissionDecision::Allow => pick(&["allow_once", "allow_always"]),
                        PermissionDecision::Reject => pick(&["reject_once", "reject_always"]),
                        PermissionDecision::Cancelled => None,
                    };
                    let _ = ctx.events.send(ProviderEvent::Other(json!({
                        "permission": {"tool": title, "kind": kind, "decision": format!("{decision:?}").to_lowercase()}
                    })));
                    Ok(match chosen {
                        Some(id) => json!({"outcome": {"outcome": "selected", "optionId": id}}),
                        None => json!({"outcome": {"outcome": "cancelled"}}),
                    })
                }
                "fs/read_text_file" => {
                    if policy::permission(ctx.permission, "read") != Verdict::Allow {
                        return Err(RpcError::new(
                            -32001,
                            "reading files isn't allowed for this task",
                        ));
                    }
                    let path = params
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| RpcError::invalid_params("path is missing"))?;
                    let path = inside(&ctx.root, path)?;
                    let text = tokio::fs::read_to_string(&path).await.map_err(|e| {
                        RpcError::new(RpcError::INTERNAL, format!("{}: {e}", path.display()))
                    })?;
                    let line = params
                        .get("line")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .max(1) as usize;
                    let limit = params
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|l| l as usize);
                    let content = if line == 1 && limit.is_none() {
                        text
                    } else {
                        let lines = text.split_inclusive('\n').skip(line - 1);
                        match limit {
                            Some(n) => lines.take(n).collect(),
                            None => lines.collect(),
                        }
                    };
                    Ok(json!({"content": content}))
                }
                "fs/write_text_file" => {
                    let path = params
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| RpcError::invalid_params("path is missing"))?
                        .to_string();
                    let content = params
                        .get("content")
                        .and_then(Value::as_str)
                        .ok_or_else(|| RpcError::invalid_params("content is missing"))?
                        .to_string();
                    let title = format!("write {path}");
                    if decide(&ctx, "edit", &title, Vec::new(), params.clone()).await
                        != PermissionDecision::Allow
                    {
                        return Err(RpcError::new(
                            -32001,
                            "writing files isn't allowed for this task",
                        ));
                    }
                    let target = inside(&ctx.root, &path)?;
                    tokio::fs::write(&target, content).await.map_err(|e| {
                        RpcError::new(RpcError::INTERNAL, format!("{}: {e}", target.display()))
                    })?;
                    Ok(Value::Null)
                }
                other => Err(RpcError::method_not_found(other)),
            }
        })
    }

    fn notification(&self, method: &str, params: Value) {
        if method != "session/update" {
            return;
        }
        let Some(session) = params.get("sessionId").and_then(Value::as_str) else {
            return;
        };
        let mut turns = lock(&self.turns);
        let Some(turn) = turns.get_mut(session) else {
            return;
        };
        let update = params.get("update").cloned().unwrap_or(Value::Null);
        let text = |u: &Value| {
            u.pointer("/content/text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let s = |u: &Value, k: &str| u.get(k).and_then(Value::as_str).map(str::to_string);
        let event = match update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or("")
        {
            "agent_message_chunk" => {
                let t = text(&update);
                turn.text.push_str(&t);
                ProviderEvent::Message(t)
            }
            "agent_thought_chunk" => ProviderEvent::Thought(text(&update)),
            "tool_call" => ProviderEvent::ToolCall {
                id: s(&update, "toolCallId").unwrap_or_default(),
                title: s(&update, "title").unwrap_or_default(),
                kind: s(&update, "kind").unwrap_or_else(|| "other".into()),
                status: s(&update, "status").unwrap_or_else(|| "pending".into()),
            },
            "tool_call_update" => ProviderEvent::ToolCallUpdate {
                id: s(&update, "toolCallId").unwrap_or_default(),
                status: s(&update, "status"),
            },
            "plan" => ProviderEvent::Plan(update.get("entries").cloned().unwrap_or(Value::Null)),
            _ => ProviderEvent::Other(update),
        };
        let _ = turn.ctx.events.send(event);
    }
}
