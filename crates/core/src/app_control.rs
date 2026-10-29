//! Driving the running app from outside it (the `sawit-mcp` server): one JSON
//! command per connection over the same Unix socket the `sawit` CLI uses.
//! The client writes [`MAGIC`] and the JSON request, closes its sending half,
//! and reads one reply line: `ok <json>` or `error: <message>`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::instances::AppPaths;

/// Starts a control request; open requests start with `sawit-open/1`.
pub const MAGIC: &[u8] = b"sawit-control/1\n";

pub const REPLY_OK: &str = "ok ";
pub const REPLY_ERROR: &str = "error: ";

/// A command for one window: the one whose project folder holds `project`
/// (the deepest), else the frontmost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    Instances,
    Files,
    Git,
    Database,
    Run,
    Ports,
    Network,
    Prompts,
    Ssh,
    ShellConfig,
}

/// Single-tab pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    Canvas,
    Mcp,
    Tools,
    Changelog,
    GitGraph,
    Github,
    Instance,
}

/// Data the app keeps in memory and saves itself; after another process
/// changes the file, the app reads it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    Instances,
    Connections,
    Prompts,
    Ssh,
    RunCommands,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Windows, their project, layout, tabs and terminals.
    State,
    /// Brings the window forward.
    Focus,
    /// A file (at a 1-based line/column) or a folder, as `sawit <path>` does.
    Open {
        path: PathBuf,
        #[serde(default)]
        line: Option<u32>,
        #[serde(default)]
        column: Option<u32>,
        #[serde(default)]
        new_window: bool,
    },
    ShowPanel {
        panel: Panel,
    },
    /// Shows (`true`) or hides each area; `None` leaves it as is.
    Layout {
        #[serde(default)]
        sidebar: Option<bool>,
        #[serde(default)]
        terminal: Option<bool>,
        #[serde(default)]
        center: Option<bool>,
    },
    MaximizeTerminal,
    OpenPage {
        page: Page,
    },
    /// Activates the tab with this index (from `state`) or title.
    ActivateTab {
        tab: String,
    },
    CloseTab {
        tab: String,
    },
    /// Types `command` and Enter into a shell: a new one in `cwd` when given
    /// or `new_tab`, else the active or newest idle shell.
    TerminalRun {
        command: String,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default)]
        new_tab: bool,
    },
    /// The text of a terminal tab (index from `state`, default the active
    /// one): its scrollback tail and screen, up to `lines` lines.
    TerminalRead {
        #[serde(default)]
        tab: Option<usize>,
        #[serde(default)]
        lines: Option<usize>,
    },
    /// Sends raw input to a terminal: text as typed, or a key such as
    /// `ctrl-c`, `enter`, `up`.
    TerminalInput {
        #[serde(default)]
        tab: Option<usize>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        key: Option<String>,
    },
    TerminalClose {
        tab: usize,
    },
    /// The Run panel's scripts and saved commands, with running state.
    RunList,
    /// Starts a script from the Run panel (`dev`, or `web · dev` in a
    /// workspace package, as `RunList` names it) or a saved command, in its
    /// own terminal.
    RunScript {
        name: String,
    },
    StopScript {
        name: String,
    },
    /// Opens an SSH session to a saved server (id) or `~/.ssh/config` alias.
    SshConnect {
        host: String,
        #[serde(default)]
        new_session: bool,
    },
    OpenTable {
        connection: String,
        schema: String,
        table: String,
    },
    OpenSql {
        connection: String,
        #[serde(default)]
        sql: Option<String>,
    },
    OpenDiagram {
        connection: String,
        schema: String,
    },
    OpenPrompt {
        id: String,
    },
    /// Changes the canvas (opening it if needed); see `web/canvas`.
    Canvas {
        op: Value,
    },
    Reload {
        store: Store,
    },
}

/// What a terminal receives for a named key: `enter`, `tab`, `escape`,
/// `backspace`, `up`/`down`/`left`/`right`, or `ctrl-<letter>`.
pub fn key_bytes(name: &str) -> Option<Vec<u8>> {
    let name = name.trim().to_ascii_lowercase();
    let fixed: &[u8] = match name.as_str() {
        "enter" | "return" => b"\r",
        "tab" => b"\t",
        "escape" | "esc" => b"\x1b",
        "backspace" => b"\x7f",
        "space" => b" ",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" => b"\x1b[5~",
        "pagedown" => b"\x1b[6~",
        "delete" => b"\x1b[3~",
        _ => {
            let letter = name
                .strip_prefix("ctrl-")
                .or_else(|| name.strip_prefix("ctrl+"))?;
            let [c] = letter.as_bytes() else { return None };
            return match c {
                b'a'..=b'z' => Some(vec![c & 0x1f]),
                b'[' => Some(vec![0x1b]),
                b'\\' => Some(vec![0x1c]),
                _ => None,
            };
        }
    };
    Some(fixed.to_vec())
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
            command: Command::Reload { store },
        };
        let _ = send(paths, &request, Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let request = Request {
            project: Some("/code/app".into()),
            command: Command::TerminalRun {
                command: "npm test".into(),
                cwd: None,
                new_tab: true,
            },
        };
        let bytes = encode(&request);
        assert_eq!(decode(&bytes), Some(Ok(request)));
    }

    #[test]
    fn open_requests_are_not_control_requests() {
        assert_eq!(decode(b"sawit-open/1\0n"), None);
    }

    #[test]
    fn wire_format_is_flat_json() {
        let bytes = encode(&Request {
            project: None,
            command: Command::ShowPanel {
                panel: Panel::Ports,
            },
        });
        assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"cmd":"show_panel","panel":"ports"}"#
        );
        assert!(
            decode(b"sawit-control/1\n{\"cmd\":\"nope\"}")
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn tools_page_round_trips() {
        let request = Request {
            project: None,
            command: Command::OpenPage { page: Page::Tools },
        };
        let bytes = encode(&request);
        assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"cmd":"open_page","page":"tools"}"#
        );
        assert_eq!(decode(&bytes), Some(Ok(request)));
    }

    #[test]
    fn keys() {
        assert_eq!(key_bytes("ctrl-c"), Some(vec![3]));
        assert_eq!(key_bytes("Ctrl+D"), Some(vec![4]));
        assert_eq!(key_bytes("enter"), Some(b"\r".to_vec()));
        assert_eq!(key_bytes("up"), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes("ctrl-1"), None);
        assert_eq!(key_bytes("hyper"), None);
    }

    #[test]
    fn replies() {
        assert_eq!(
            parse_reply(r#"ok {"a":1}"#),
            Ok(serde_json::json!({"a": 1}))
        );
        assert_eq!(parse_reply("error: no window"), Err("no window".into()));
        let message = "line one\nC:\\path";
        assert_eq!(parse_reply(&reply_error(message)), Err(message.into()));
        assert!(!reply_error(message).contains('\n'));
    }
}
