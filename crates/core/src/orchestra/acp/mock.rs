//! A small ACP agent for tests, speaking the agent side of the protocol over any byte stream. What
//! a prompt does is chosen by words in it:
//! - `ask-permission`: asks to edit a file and answers with the option it was given;
//! - `read-file:<path>` / `write-file:<path>`: reads or writes through the client;
//! - `tool-call`: reports a tool call and its completion;
//! - `slow`: waits until cancelled (or 30 s); `ignore-cancel`: waits 60 s whatever happens;
//! - `crash`: drops the connection mid-turn (exits, in a process).
//!
//! Every turn streams `echo: <prompt>` first and `done` last.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

use super::rpc::{Incoming, Rpc, RpcError};

pub struct MockAgent {
    rpc: OnceLock<Weak<Rpc>>,
    load_session: bool,
    next: AtomicU64,
    cancels: Mutex<HashMap<String, CancellationToken>>,
    /// Exit the process on `crash` (when it runs as a child process).
    pub exit_on_crash: bool,
}

/// Serves a mock agent on `reader`/`writer`; the returned connection can be closed to simulate
/// the agent going away.
pub fn serve<R, W>(reader: R, writer: W, load_session: bool, exit_on_crash: bool) -> Arc<Rpc>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let agent = Arc::new(MockAgent {
        rpc: OnceLock::new(),
        load_session,
        next: AtomicU64::new(1),
        cancels: Mutex::default(),
        exit_on_crash,
    });
    let rpc = Rpc::start(reader, writer, Arc::new(agent.clone()));
    let _ = agent.rpc.set(Arc::downgrade(&rpc));
    rpc
}

fn chunk(session: &str, text: &str) -> Value {
    json!({"sessionId": session, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}})
}

fn word_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(key)? + key.len();
    text[at..].split_whitespace().next()
}

impl MockAgent {
    async fn prompt(self: Arc<Self>, params: Value) -> Result<Value, RpcError> {
        let rpc = self
            .rpc
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| RpcError::new(RpcError::INTERNAL, "gone"))?;
        let session = params["sessionId"].as_str().unwrap_or_default().to_string();
        let text = params
            .pointer("/prompt/0/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let cancel = CancellationToken::new();
        self.cancels
            .lock()
            .unwrap()
            .insert(session.clone(), cancel.clone());
        let say = |t: String| rpc.notify("session/update", chunk(&session, &t));
        say(format!("echo: {}", text.lines().last().unwrap_or_default())).await?;
        if text.contains("ask-permission") {
            let answer = rpc
                .request(
                    "session/request_permission",
                    json!({
                        "sessionId": session,
                        "toolCall": {"toolCallId": "t1", "title": "Edit main.rs", "kind": "edit"},
                        "options": [
                            {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
                            {"optionId": "no", "name": "Reject", "kind": "reject_once"}
                        ]
                    }),
                )
                .await?;
            let picked = answer
                .pointer("/outcome/optionId")
                .and_then(Value::as_str)
                .unwrap_or("cancelled");
            say(format!(" permission={picked}")).await?;
        }
        if let Some(path) = word_after(&text, "read-file:") {
            let r = rpc
                .request(
                    "fs/read_text_file",
                    json!({"sessionId": session, "path": path}),
                )
                .await;
            say(match r {
                Ok(v) => format!(" content={}", v["content"].as_str().unwrap_or_default()),
                Err(e) => format!(" read-error={}", e.message),
            })
            .await?;
        }
        if let Some(path) = word_after(&text, "write-file:") {
            let r = rpc
                .request(
                    "fs/write_text_file",
                    json!({"sessionId": session, "path": path, "content": "written by mock"}),
                )
                .await;
            say(match r {
                Ok(_) => " write=ok".to_string(),
                Err(e) => format!(" write-error={}", e.message),
            })
            .await?;
        }
        if text.contains("tool-call") {
            let update =
                |u: Value| rpc.notify("session/update", json!({"sessionId": session, "update": u}));
            update(json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "List files", "kind": "read", "status": "pending"})).await?;
            update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed"})).await?;
        }
        if text.contains("crash") {
            if self.exit_on_crash {
                std::process::exit(3);
            }
            rpc.close("crashed");
            // A crashed agent answers nothing.
            std::future::pending::<()>().await;
        }
        if text.contains("ignore-cancel") {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        if text.contains("slow") {
            tokio::select! {
                _ = cancel.cancelled() => return Ok(json!({"stopReason": "cancelled"})),
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
            }
        }
        say(" done".into()).await?;
        Ok(json!({"stopReason": "end_turn"}))
    }
}

impl Incoming for Arc<MockAgent> {
    fn request(
        &self,
        method: String,
        params: Value,
    ) -> BoxFuture<'static, Result<Value, RpcError>> {
        let me = self.clone();
        Box::pin(async move {
            match method.as_str() {
                "initialize" => Ok(json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {"loadSession": me.load_session, "promptCapabilities": {"image": false}},
                    "authMethods": [],
                    "agentInfo": {"name": "mock-agent", "version": "1"}
                })),
                "session/new" => Ok(
                    json!({"sessionId": format!("mock-{}", me.next.fetch_add(1, Ordering::Relaxed))}),
                ),
                "session/load" => {
                    let id = params["sessionId"].as_str().unwrap_or_default().to_string();
                    if !me.load_session || !id.starts_with("mock-") {
                        return Err(RpcError::new(-32002, "no such session"));
                    }
                    // The history replay a client must not take for a new turn's output.
                    if let Some(rpc) = me.rpc.get().and_then(Weak::upgrade) {
                        rpc.notify("session/update", chunk(&id, "replayed history"))
                            .await?;
                    }
                    Ok(Value::Null)
                }
                "session/prompt" => me.prompt(params).await,
                other => Err(RpcError::method_not_found(other)),
            }
        })
    }

    fn notification(&self, method: &str, params: Value) {
        if method == "session/cancel"
            && let Some(s) = params["sessionId"].as_str()
            && let Some(t) = self.cancels.lock().unwrap().get(s)
        {
            t.cancel();
        }
    }
}
