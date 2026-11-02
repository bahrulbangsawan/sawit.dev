//! Driving the running app from outside it (the `sawit-mcp` server): one JSON
//! command per connection over the same Unix socket the `sawit` CLI uses.
//! The client writes [`MAGIC`] and the JSON request, closes its sending half,
//! and reads one reply line: `ok <json>` or `error: <message>`.

mod command;
mod keys;
#[cfg(test)]
mod tests;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::instances::AppPaths;

pub use command::{Command, InstanceAction, Page, Panel, SplitDirection, Store};
pub use keys::{KEY_NAMES, key_bytes};

/// Starts a control request; open requests start with `sawit-open/1`.
pub const MAGIC: &[u8] = b"sawit-control/1\n";

pub const REPLY_OK: &str = "ok ";
pub const REPLY_ERROR: &str = "error: ";

/// A command for one window: the one whose project folder holds `project`
/// (the deepest). Without `project`, the one whose project folder holds
/// `agent_folder`, the folder the agent works in, else the frontmost.
///
/// `agent` says which agent session sent it ([`crate::agents`]), so that the app can show where
/// each agent works and what it does. It is optional both ways: an older `sawit-mcp` sends none,
/// and the app counts its ops as one anonymous agent's; an older app reads past the field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_folder: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<crate::agents::AgentIdentity>,
    #[serde(flatten)]
    pub command: Command,
}

pub fn encode(request: &Request) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend(serde_json::to_vec(request).expect("requests serialize"));
    bytes
}

/// `None` when `bytes` isn't a control request; `Some(Err)` when it is one
/// the app can't read.
pub fn decode(bytes: &[u8]) -> Option<Result<Request, String>> {
    let json = bytes.strip_prefix(MAGIC)?;
    Some(serde_json::from_slice(json).map_err(|e| format!("bad request: {e}")))
}

pub fn reply_ok(value: &Value) -> String {
    format!("{REPLY_OK}{value}")
}

/// Replies are one line, so line breaks in the message are escaped.
pub fn reply_error(message: &str) -> String {
    format!(
        "{REPLY_ERROR}{}",
        message.replace('\\', "\\\\").replace('\n', "\\n")
    )
}

fn unescape(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn parse_reply(line: &str) -> Result<Value, String> {
    if let Some(json) = line.strip_prefix(REPLY_OK) {
        return serde_json::from_str(json).map_err(|e| format!("bad reply from Sawit: {e}"));
    }
    if let Some(e) = line.strip_prefix(REPLY_ERROR) {
        return Err(unescape(e));
    }
    Err(format!("unexpected reply from Sawit: {line}"))
}

/// Whether the app is running and listening.
pub fn is_running(paths: &AppPaths) -> bool {
    UnixStream::connect(paths.cli_socket()).is_ok()
}

/// Sends `request` to the running app and waits for its reply. Blocking;
/// call it off async executors' worker threads.
pub fn send(paths: &AppPaths, request: &Request, timeout: Duration) -> Result<Value, String> {
    let mut stream = UnixStream::connect(paths.cli_socket()).map_err(|_| {
        "Sawit isn't running. Start it with the launch_app tool, or open Sawit.app.".to_string()
    })?;
    let _ = stream.set_read_timeout(Some(timeout));
    stream
        .write_all(&encode(request))
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|e| format!("unable to reach Sawit: {e}"))?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                "Sawit didn't answer in time.".to_string()
            }
            _ => format!("unable to read Sawit's reply: {e}"),
        })?;
    if line.is_empty() {
        return Err("Sawit closed the connection without answering.".into());
    }
    parse_reply(line.trim_end())
}

/// Tells a running app to re-read `store` from disk; does nothing when the
/// app isn't running.
pub fn notify_reload(paths: &AppPaths, store: Store) {
    if is_running(paths) {
        let request = Request {
            project: None,
            agent_folder: None,
            agent: None,
            command: Command::Reload { store },
        };
        let _ = send(paths, &request, Duration::from_secs(5));
    }
}
