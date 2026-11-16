//! JSON-RPC 2.0 over a byte stream, one message per line, as ACP's stdio transport frames it.
//! Both sides send requests: [`Rpc::request`] correlates each answer with its id, and requests
//! the other side sends go to an [`Incoming`] handler. Notifications are handled in the order
//! they arrive (a session's updates must not be reordered); requests run concurrently, a bounded
//! number at a time. A line that isn't JSON-RPC is counted and skipped, never fatal. When the
//! stream ends every request still waiting fails at once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// The longest line read, so a runaway peer can't exhaust memory.
const MAX_LINE: usize = 64 * 1024 * 1024;
/// Incoming requests handled at once.
const MAX_INCOMING: usize = 32;

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{message} ({code})")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    /// Ours: the connection closed before the answer came.
    pub const DISCONNECTED: i64 = -32099;

    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            Self::METHOD_NOT_FOUND,
            format!("method not found: {method}"),
        )
    }

    pub fn invalid_params(why: impl Into<String>) -> Self {
        Self::new(Self::INVALID_PARAMS, why)
    }

    fn to_value(&self) -> Value {
        let mut v = json!({"code": self.code, "message": self.message});
        if let Some(d) = &self.data {
            v["data"] = d.clone();
        }
        v
    }

    fn from_value(v: &Value) -> Self {
        Self {
            code: v
                .get("code")
                .and_then(Value::as_i64)
                .unwrap_or(Self::INTERNAL),
            message: v
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string(),
            data: v.get("data").cloned(),
        }
    }
}

/// What handles the other side's requests and notifications.
pub trait Incoming: Send + Sync {
    fn request(&self, method: String, params: Value)
    -> BoxFuture<'static, Result<Value, RpcError>>;
    fn notification(&self, method: &str, params: Value);
}

type Pending = Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>;

pub struct Rpc {
    out: mpsc::Sender<Value>,
    pending: Arc<Pending>,
    next: AtomicU64,
    closed: CancellationToken,
    close_reason: Arc<Mutex<Option<String>>>,
    malformed: Arc<AtomicU64>,
}

impl Rpc {
    /// Starts reading `reader` and writing `writer` on the current Tokio runtime.
    pub fn start<R, W>(reader: R, writer: W, handler: Arc<dyn Incoming>) -> Arc<Rpc>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (out, out_rx) = mpsc::channel::<Value>(256);
        let rpc = Arc::new(Rpc {
            out: out.clone(),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next: AtomicU64::new(1),
            closed: CancellationToken::new(),
            close_reason: Arc::new(Mutex::new(None)),
            malformed: Arc::new(AtomicU64::new(0)),
        });
        tokio::spawn(write_loop(writer, out_rx, rpc.closer()));
        tokio::spawn(read_loop(
            reader,
            handler,
            out,
            rpc.closer(),
            rpc.pending.clone(),
            rpc.malformed.clone(),
        ));
        rpc
    }

    fn closer(&self) -> Closer {
        Closer {
            token: self.closed.clone(),
            reason: self.close_reason.clone(),
            pending: self.pending.clone(),
        }
    }

    /// Sends a request and waits for its answer.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        // Dropping this future (a cancelled caller) forgets the request.
        let _forget = Forget {
            pending: &self.pending,
            id,
        };
        if self.closed.is_cancelled() {
            return Err(self.disconnected());
        }
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if self.out.send(msg).await.is_err() {
            return Err(self.disconnected());
        }
        rx.await.unwrap_or_else(|_| Err(self.disconnected()))
    }

    /// Sends a notification (no answer).
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.out.send(msg).await.map_err(|_| self.disconnected())
    }

    /// Cancelled when the connection closes.
    pub fn closed(&self) -> CancellationToken {
        self.closed.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.closed.is_cancelled()
    }

    /// Lines that were not JSON-RPC, skipped so far.
    pub fn malformed(&self) -> u64 {
        self.malformed.load(Ordering::Relaxed)
    }

    /// Closes the connection: waiting requests fail, the reader and writer stop.
    pub fn close(&self, reason: &str) {
        self.closer().close(reason);
    }

    fn disconnected(&self) -> RpcError {
        let reason = lock(&self.close_reason)
            .clone()
            .unwrap_or_else(|| "connection closed".into());
        RpcError::new(RpcError::DISCONNECTED, reason)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Forget<'a> {
    pending: &'a Pending,
    id: u64,
}

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        lock(self.pending).remove(&self.id);
    }
}

#[derive(Clone)]
struct Closer {
    token: CancellationToken,
    reason: Arc<Mutex<Option<String>>>,
    pending: Arc<Pending>,
}

impl Closer {
    fn close(&self, reason: &str) {
        lock(&self.reason).get_or_insert_with(|| reason.to_string());
        self.token.cancel();
        let waiting: Vec<_> = lock(&self.pending).drain().collect();
        for (_, tx) in waiting {
            let _ = tx.send(Err(RpcError::new(RpcError::DISCONNECTED, reason)));
        }
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut rx: mpsc::Receiver<Value>,
    closer: Closer,
) {
    loop {
        let msg = tokio::select! {
            // A closed connection writes nothing more, even what is queued.
            biased;
            _ = closer.token.cancelled() => None,
            m = rx.recv() => m,
        };
        let Some(msg) = msg else { break };
        let mut line = msg.to_string();
        line.push('\n');
        if let Err(e) = async {
            writer.write_all(line.as_bytes()).await?;
            writer.flush().await
        }
        .await
        {
            closer.close(&format!("write failed: {e}"));
            break;
        }
    }
    let _ = writer.shutdown().await;
}

async fn read_loop<R: AsyncRead + Unpin>(
    reader: R,
    handler: Arc<dyn Incoming>,
    out: mpsc::Sender<Value>,
    closer: Closer,
    pending: Arc<Pending>,
    malformed: Arc<AtomicU64>,
) {
    let mut reader = BufReader::new(reader);
    let limit = Arc::new(Semaphore::new(MAX_INCOMING));
    let mut buf = Vec::new();
    let reason = loop {
        buf.clear();
        let mut limited = (&mut reader).take(MAX_LINE as u64);
        let read = tokio::select! {
            r = limited.read_until(b'\n', &mut buf) => r,
            _ = closer.token.cancelled() => break "closed".to_string(),
        };
        match read {
            Ok(0) => break "the agent closed its output".to_string(),
            Err(e) => break format!("read failed: {e}"),
            Ok(_) if buf.len() >= MAX_LINE && !buf.ends_with(b"\n") => {
                break "a message was longer than the limit".to_string();
            }
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(text) else {
            malformed.fetch_add(1, Ordering::Relaxed);
            log::debug!(target: "orchestra::acp", "skipped a line that isn't JSON ({} bytes)", text.len());
            continue;
        };
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);
        let id = msg.get("id").cloned().filter(|i| !i.is_null());
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match (method, id) {
            (Some(method), Some(id)) => {
                let Ok(permit) = limit.clone().acquire_owned().await else {
                    break "closed".into();
                };
                let handler = handler.clone();
                let out = out.clone();
                tokio::spawn(async move {
                    let answer = handler.request(method, params).await;
                    let msg = match answer {
                        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e.to_value()}),
                    };
                    let _ = out.send(msg).await;
                    drop(permit);
                });
            }
            (Some(method), None) => handler.notification(&method, params),
            (None, Some(id)) => {
                let Some(id) = id.as_u64() else {
                    malformed.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                let answer = match (msg.get("result"), msg.get("error")) {
                    (_, Some(e)) if !e.is_null() => Err(RpcError::from_value(e)),
                    (Some(r), _) => Ok(r.clone()),
                    _ => Ok(Value::Null),
                };
                if let Some(tx) = lock(&pending).remove(&id) {
                    let _ = tx.send(answer);
                }
            }
            (None, None) => {
                malformed.fetch_add(1, Ordering::Relaxed);
            }
        }
    };
    closer.close(&reason);
}
