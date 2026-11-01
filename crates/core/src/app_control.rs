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
/// (the deepest). Without `project`, the one whose project folder holds
/// `agent_folder`, the folder the agent works in, else the frontmost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_folder: Option<PathBuf>,
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
    Browser,
    Mcp,
    Tools,
    Skills,
    Changelog,
    GitGraph,
    Github,
    Instance,
    Keymap,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceAction {
    Select,
    Start,
    Stop,
    Restart,
}

/// Where a terminal split puts the new pane, as the pane menu's Split right and Split down do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    /// Side by side, the new pane on the right (⌘D).
    Right,
    /// Stacked, the new pane below (⇧⌘D).
    Down,
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
    /// Activates the tab with this index (from `state`) or title, and its project.
    ActivateTab {
        tab: String,
    },
    /// Closes the tab with this index or title; one that would lose something (unsaved changes,
    /// unsent annotations, a Dev Tools install) asks the user first, and the answer says so.
    CloseTab {
        tab: String,
    },
    /// Types `command` and Enter into a shell: a new one in `cwd` when given
    /// or `new_tab` (without `cwd`, in the agent's own project, as for its
    /// browser), else the active or newest idle shell, never the one the
    /// user is typing in.
    TerminalRun {
        command: String,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default)]
        new_tab: bool,
    },
    /// The text of a terminal pane, in the tab with this index (from
    /// `state`, default the active one): its scrollback tail and screen, up
    /// to `lines` lines.
    TerminalRead {
        #[serde(default)]
        tab: Option<usize>,
        /// The pane within the tab. Without one: the active tab's focused
        /// pane, or a named tab's only pane (a split tab needs it named).
        #[serde(default)]
        pane: Option<usize>,
        #[serde(default)]
        lines: Option<usize>,
    },
    /// Sends raw input to a terminal: text as typed, or a key named in
    /// [`KEY_NAMES`] (`ctrl-c`, `enter`, `up`). `sawit-mcp` always names the
    /// tab.
    TerminalInput {
        #[serde(default)]
        tab: Option<usize>,
        /// The pane within the tab, as for `TerminalRead`.
        #[serde(default)]
        pane: Option<usize>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        key: Option<String>,
    },
    /// Closes a terminal tab, or only one of its panes, ending what runs
    /// there. Without `confirm` it is refused while a program runs there (a
    /// dev server, an SSH session, an agent) or the user is typing in it.
    TerminalClose {
        tab: usize,
        #[serde(default)]
        pane: Option<usize>,
        #[serde(default)]
        confirm: bool,
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
        /// Runs it right away (once connected), as ⌘↩ does.
        #[serde(default)]
        run: bool,
    },
    /// Selects an instance in the Instances tab and starts, stops or
    /// restarts it there. A start whose port is taken is held, and the
    /// instance tab explains the conflict, as when clicking Start.
    Instance {
        name: String,
        action: InstanceAction,
    },
    /// Runs the Network tab's diagnosis, and pings `ping` when given.
    NetworkDiagnose {
        #[serde(default)]
        ping: Option<String>,
    },
    OpenDiagram {
        connection: String,
        schema: String,
    },
    OpenPrompt {
        id: String,
    },
    /// Changes the canvas (opening it if needed): one op of `sawit_board::ops`, answered at once.
    /// The canvas is the one of the agent's project, or with `file` the canvas of that file (an
    /// absolute path to a `.sawit` file, a `.figcraft` one from before, or an `.excalidraw`
    /// file), in a canvas tab of its own as `sawit <file>` opens it.
    Canvas {
        op: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<PathBuf>,
    },
    /// Drives the project's browser tab; see `crate::browser`. Only the ops that show a page
    /// ([`BrowserOp::shows_tab`](crate::browser::BrowserOp::shows_tab): navigate, screenshot,
    /// resize) open it; the others answer that it isn't open ([`crate::browser::NOT_OPEN`]).
    Browser {
        op: crate::browser::BrowserOp,
    },
    Reload {
        store: Store,
    },
    /// Splits pane `pane` of terminal tab `tab` (a split tab needs it
    /// named), as the pane menu's Split right and Split down do, and starts
    /// a shell in the new half: in `cwd`, else the split pane's folder. It
    /// types `command` and Enter there when given. The new pane never takes
    /// the keyboard, and a split that would leave a half under 20 columns by
    /// 4 rows is refused.
    TerminalSplit {
        tab: usize,
        #[serde(default)]
        pane: Option<usize>,
        direction: SplitDirection,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    /// Reads or changes the key bindings, which every window shares; see
    /// [`crate::keymap`].
    Keymap {
        op: crate::keymap::KeymapOp,
    },
}

/// The keys [`key_bytes`] knows, as `terminal_input` and its errors list them.
pub const KEY_NAMES: &str = "enter, tab, escape, backspace, space, up, down, left, right, home, \
end, pageup, pagedown, delete or ctrl-<letter> (return, esc and ctrl+<letter> work too)";

/// What a terminal receives for a named key, one of [`KEY_NAMES`] (in any
/// case), or `ctrl-[` and `ctrl-\`.
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
            agent_folder: None,
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
            agent_folder: None,
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

    /// Without a project, the agent's folder goes along with the command.
    #[test]
    fn the_agents_folder_round_trips() {
        let request = Request {
            project: None,
            agent_folder: Some("/code/app".into()),
            command: Command::TerminalRead {
                tab: None,
                pane: None,
                lines: None,
            },
        };
        let bytes = encode(&request);
        assert!(String::from_utf8_lossy(&bytes).contains(r#""agent_folder":"/code/app""#));
        assert_eq!(decode(&bytes), Some(Ok(request)));
    }

    #[test]
    fn wire_format_is_flat_json() {
        let bytes = encode(&Request {
            project: None,
            agent_folder: None,
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
            agent_folder: None,
            command: Command::OpenPage { page: Page::Tools },
        };
        let bytes = encode(&request);
        assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"cmd":"open_page","page":"tools"}"#
        );
        assert_eq!(decode(&bytes), Some(Ok(request)));
        let skills = Request {
            project: None,
            agent_folder: None,
            command: Command::OpenPage { page: Page::Skills },
        };
        let bytes = encode(&skills);
        assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"cmd":"open_page","page":"skills"}"#
        );
        assert_eq!(decode(&bytes), Some(Ok(skills)));
    }

    #[test]
    fn instance_network_and_run_round_trip() {
        for (command, wire) in [
            (
                Command::Instance {
                    name: "shop".into(),
                    action: InstanceAction::Start,
                },
                r#"{"cmd":"instance","name":"shop","action":"start"}"#,
            ),
            (
                Command::NetworkDiagnose {
                    ping: Some("github.com".into()),
                },
                r#"{"cmd":"network_diagnose","ping":"github.com"}"#,
            ),
            (
                Command::OpenSql {
                    connection: "shop".into(),
                    sql: Some("select 1".into()),
                    run: true,
                },
                r#"{"cmd":"open_sql","connection":"shop","sql":"select 1","run":true}"#,
            ),
        ] {
            let request = Request {
                project: None,
                agent_folder: None,
                command,
            };
            let bytes = encode(&request);
            assert_eq!(&bytes[MAGIC.len()..], wire.as_bytes());
            assert_eq!(decode(&bytes), Some(Ok(request)));
        }
        // `run` and `ping` are optional.
        let old = decode(
            br#"sawit-control/1
{"cmd":"open_sql","connection":"shop"}"#,
        );
        assert!(matches!(
            old,
            Some(Ok(Request {
                command: Command::OpenSql { run: false, .. },
                ..
            }))
        ));
        assert!(
            decode(
                br#"sawit-control/1
{"cmd":"instance","name":"a","action":"burn"}"#
            )
            .unwrap()
            .is_err()
        );
    }

    #[test]
    fn browser_ops_round_trip() {
        let request = Request {
            project: None,
            agent_folder: None,
            command: Command::Browser {
                op: crate::browser::BrowserOp::Navigate {
                    url: "http://localhost:5173".into(),
                },
            },
        };
        let bytes = encode(&request);
        assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"cmd":"browser","op":{"op":"navigate","url":"http://localhost:5173"}}"#
        );
        assert_eq!(decode(&bytes), Some(Ok(request)));
    }

    /// A canvas op names the canvas file it is for, when it is for one; without one it goes as it
    /// always went, and a request from an older `sawit-mcp` reads as the project's canvas.
    #[test]
    fn canvas_ops_round_trip_with_the_file_they_name() {
        let op = serde_json::json!({"op": "get"});
        for (file, wire) in [
            (
                Some(PathBuf::from("/code/shop/design/home.sawit")),
                r#"{"cmd":"canvas","op":{"op":"get"},"file":"/code/shop/design/home.sawit"}"#,
            ),
            (None, r#"{"cmd":"canvas","op":{"op":"get"}}"#),
        ] {
            let request = Request {
                project: None,
                agent_folder: None,
                command: Command::Canvas {
                    op: op.clone(),
                    file,
                },
            };
            let bytes = encode(&request);
            assert_eq!(&bytes[MAGIC.len()..], wire.as_bytes());
            assert_eq!(decode(&bytes), Some(Ok(request)));
        }
        let old = decode(b"sawit-control/1\n{\"cmd\":\"canvas\",\"op\":{\"op\":\"clear\"}}");
        assert!(matches!(
            old,
            Some(Ok(Request {
                command: Command::Canvas { file: None, .. },
                ..
            }))
        ));
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

    /// The one list of key names that terminal_input's description and its error give is true:
    /// every name in it is a key `key_bytes` sends, and every key it sends by a name is in it.
    #[test]
    fn the_key_names_given_are_the_keys_there_are() {
        let named: Vec<&str> = KEY_NAMES
            .split(|c: char| c == ',' || c == '(' || c == ')' || c.is_whitespace())
            .filter(|word| !word.is_empty() && !matches!(*word, "or" | "and" | "work" | "too"))
            .collect();
        for name in &named {
            let key = name.replace("<letter>", "x");
            assert!(
                key_bytes(&key).is_some(),
                "{name} is listed, but isn't a key"
            );
        }
        for key in [
            "enter",
            "return",
            "tab",
            "escape",
            "esc",
            "backspace",
            "space",
            "up",
            "down",
            "left",
            "right",
            "home",
            "end",
            "pageup",
            "pagedown",
            "delete",
        ] {
            assert!(named.contains(&key), "{key} isn't in {KEY_NAMES}");
        }
        assert!(named.contains(&"ctrl-<letter>") && named.contains(&"ctrl+<letter>"));
    }

    /// An agent's split names its tab and the way to split, and leaves the rest out when it has
    /// nothing to say: the app splits the tab's only pane, starts the shell in its folder and runs
    /// nothing.
    #[test]
    fn a_terminal_split_round_trips() {
        let request = Request {
            project: None,
            agent_folder: Some("/code/app".into()),
            command: Command::TerminalSplit {
                tab: 2,
                pane: Some(1),
                direction: SplitDirection::Down,
                command: Some("npm test -- --watch".into()),
                cwd: Some("/code/app/web".into()),
            },
        };
        let bytes = encode(&request);
        assert_eq!(decode(&bytes), Some(Ok(request)));
        let bare = decode(
            br#"sawit-control/1
{"cmd":"terminal_split","tab":0,"direction":"right"}"#,
        );
        assert_eq!(
            bare,
            Some(Ok(Request {
                project: None,
                agent_folder: None,
                command: Command::TerminalSplit {
                    tab: 0,
                    pane: None,
                    direction: SplitDirection::Right,
                    command: None,
                    cwd: None,
                },
            }))
        );
        for wrong in [
            br#"{"cmd":"terminal_split","tab":0,"direction":"left"}"#.as_slice(),
            br#"{"cmd":"terminal_split","direction":"down"}"#.as_slice(),
        ] {
            let mut bytes = MAGIC.to_vec();
            bytes.extend_from_slice(wrong);
            assert!(
                decode(&bytes).unwrap().is_err(),
                "{}",
                String::from_utf8_lossy(wrong)
            );
        }
    }

    /// A close from a `sawit-mcp` that predates `confirm` reads as unconfirmed, so the app still
    /// asks before ending what runs there.
    #[test]
    fn a_terminal_close_is_unconfirmed_unless_it_says_so() {
        let old = decode(
            br#"sawit-control/1
{"cmd":"terminal_close","tab":1}"#,
        );
        assert!(matches!(
            old,
            Some(Ok(Request {
                command: Command::TerminalClose {
                    tab: 1,
                    pane: None,
                    confirm: false
                },
                ..
            }))
        ));
        let request = Request {
            project: None,
            agent_folder: None,
            command: Command::TerminalClose {
                tab: 1,
                pane: Some(0),
                confirm: true,
            },
        };
        let bytes = encode(&request);
        assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"cmd":"terminal_close","tab":1,"pane":0,"confirm":true}"#
        );
        assert_eq!(decode(&bytes), Some(Ok(request)));
    }

    /// The keymap's ops go to the app inside one `keymap` command.
    #[test]
    fn keymap_ops_round_trip() {
        let request = Request {
            project: None,
            agent_folder: None,
            command: Command::Keymap {
                op: crate::keymap::KeymapOp::List {
                    filter: Some("close tab".into()),
                    keys: None,
                    unbound: true,
                },
            },
        };
        let bytes = encode(&request);
        assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"cmd":"keymap","op":{"op":"list","filter":"close tab","unbound":true}}"#
        );
        assert_eq!(decode(&bytes), Some(Ok(request)));
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
