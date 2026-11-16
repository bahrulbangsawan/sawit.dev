//! The client side of the Agent Client Protocol (https://agentclientprotocol.com), version 1:
//! `initialize`, `session/new`, `session/load`, `session/prompt` and the `session/cancel`
//! notification out; `session/update` notifications, and the `session/request_permission`,
//! `fs/read_text_file` and `fs/write_text_file` requests in. Terminals are not offered
//! (`clientCapabilities.terminal` is false), so the agent never sends `terminal/*`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::rpc::{Rpc, RpcError};
use crate::orchestra::provider::{McpServer, ProviderError, StopReason, TurnContext, TurnOutcome};

/// The protocol version this client speaks.
pub const PROTOCOL_VERSION: u64 = 1;

/// What the agent said it can do in `initialize`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentCaps {
    pub load_session: bool,
    pub image: bool,
    pub audio: bool,
    pub embedded_context: bool,
    pub mcp_http: bool,
    pub mcp_sse: bool,
    /// The ids of the sign-in methods it offers; a session may fail until one is done.
    pub auth_methods: Vec<String>,
    pub agent_name: Option<String>,
}

/// One turn in flight in a session: where its updates go, and its answer so far.
pub(super) struct Turn {
    pub(super) ctx: TurnContext,
    pub(super) text: String,
}

pub(super) type Turns = Arc<Mutex<HashMap<String, Turn>>>;

pub(super) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A connection to one ACP agent.
pub struct AcpClient {
    rpc: Arc<Rpc>,
    turns: Turns,
    pub caps: AgentCaps,
}

/// The handler for what the agent sends us.
pub struct ClientHandler {
    pub(super) turns: Turns,
}

impl ClientHandler {
    pub fn new() -> (Arc<Self>, TurnsHandle) {
        let turns: Turns = Arc::default();
        (
            Arc::new(Self {
                turns: turns.clone(),
            }),
            TurnsHandle(turns),
        )
    }
}

/// The turns table a handler and its client share.
pub struct TurnsHandle(Turns);

fn rpc_to_provider(e: RpcError) -> ProviderError {
    if e.code == RpcError::DISCONNECTED {
        ProviderError::Disconnected(e.message)
    } else if e.code == -32000 {
        ProviderError::Protocol(format!(
            "{}: the agent needs a sign-in; run it once in a terminal",
            e.message
        ))
    } else {
        ProviderError::Protocol(e.to_string())
    }
}

fn mcp_json(servers: &[McpServer]) -> Value {
    Value::Array(
        servers
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "command": s.command,
                    "args": s.args,
                    "env": s.env.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

impl AcpClient {
    /// Initializes the connection: protocol version and capabilities, both ways.
    pub async fn connect(rpc: Arc<Rpc>, turns: TurnsHandle) -> Result<AcpClient, ProviderError> {
        let answer = rpc
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": {"readTextFile": true, "writeTextFile": true},
                        "terminal": false
                    },
                    "clientInfo": {"name": "sawit", "title": "Sawit", "version": env!("CARGO_PKG_VERSION")}
                }),
            )
            .await
            .map_err(|e| ProviderError::Start(format!("initialize: {e}")))?;
        let version = answer
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if version != PROTOCOL_VERSION {
            return Err(ProviderError::Start(format!(
                "the agent speaks ACP version {version}; Sawit speaks {PROTOCOL_VERSION}"
            )));
        }
        let ac = answer
            .get("agentCapabilities")
            .cloned()
            .unwrap_or(Value::Null);
        let flag = |v: &Value, k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        let prompt = ac.get("promptCapabilities").cloned().unwrap_or(Value::Null);
        let mcp = ac.get("mcpCapabilities").cloned().unwrap_or(Value::Null);
        let caps = AgentCaps {
            load_session: flag(&ac, "loadSession"),
            image: flag(&prompt, "image"),
            audio: flag(&prompt, "audio"),
            embedded_context: flag(&prompt, "embeddedContext"),
            mcp_http: flag(&mcp, "http"),
            mcp_sse: flag(&mcp, "sse"),
            auth_methods: answer
                .get("authMethods")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.get("id")?.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            agent_name: answer
                .pointer("/agentInfo/name")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        Ok(AcpClient {
            rpc,
            turns: turns.0,
            caps,
        })
    }

    pub fn is_closed(&self) -> bool {
        self.rpc.is_closed()
    }

    pub fn rpc(&self) -> &Arc<Rpc> {
        &self.rpc
    }

    pub async fn new_session(
        &self,
        cwd: &Path,
        mcp: &[McpServer],
    ) -> Result<String, ProviderError> {
        let answer = self
            .rpc
            .request(
                "session/new",
                json!({"cwd": cwd, "mcpServers": mcp_json(mcp)}),
            )
            .await
            .map_err(rpc_to_provider)?;
        answer
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                ProviderError::Protocol("session/new answered without a sessionId".into())
            })
    }

    /// Resumes a session. The agent replays its history as updates first; with no turn
    /// registered for the session those are dropped.
    pub async fn load_session(
        &self,
        id: &str,
        cwd: &Path,
        mcp: &[McpServer],
    ) -> Result<(), ProviderError> {
        if !self.caps.load_session {
            return Err(ProviderError::Unsupported(
                "the agent can't load sessions".into(),
            ));
        }
        self.rpc
            .request(
                "session/load",
                json!({"sessionId": id, "cwd": cwd, "mcpServers": mcp_json(mcp)}),
            )
            .await
            .map(|_| ())
            .map_err(rpc_to_provider)
    }

    /// Runs one prompt turn. Cancelling `ctx.cancel` sends `session/cancel`; the agent then ends
    /// the turn with `cancelled`.
    pub async fn prompt(
        &self,
        session: &str,
        text: String,
        ctx: TurnContext,
    ) -> Result<TurnOutcome, ProviderError> {
        let cancel = ctx.cancel.clone();
        lock(&self.turns).insert(
            session.to_string(),
            Turn {
                ctx,
                text: String::new(),
            },
        );
        let params = json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]});
        let request = self.rpc.request("session/prompt", params);
        tokio::pin!(request);
        let mut cancel_sent = false;
        let answer = loop {
            tokio::select! {
                a = &mut request => break a,
                _ = cancel.cancelled(), if !cancel_sent => {
                    cancel_sent = true;
                    let _ = self.cancel(session).await;
                }
            }
        };
        let turn = lock(&self.turns).remove(session);
        let text = turn.map(|t| t.text).unwrap_or_default();
        let answer = answer.map_err(rpc_to_provider)?;
        let stop = match answer
            .get("stopReason")
            .and_then(Value::as_str)
            .unwrap_or("end_turn")
        {
            "end_turn" => StopReason::EndTurn,
            "max_tokens" => StopReason::MaxTokens,
            "max_turn_requests" => StopReason::MaxTurnRequests,
            "refusal" => StopReason::Refusal,
            "cancelled" => StopReason::Cancelled,
            other => StopReason::Other(other.to_string()),
        };
        Ok(TurnOutcome { stop, text })
    }

    pub async fn cancel(&self, session: &str) -> Result<(), ProviderError> {
        self.rpc
            .notify("session/cancel", json!({"sessionId": session}))
            .await
            .map_err(rpc_to_provider)
    }
}
