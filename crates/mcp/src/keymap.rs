//! Tools for the key bindings, as the Keymap Editor shows and changes them: what a key does, which
//! key runs an action, and binding or unbinding keys (see `sawit_core::keymap`). They need the
//! running app: each feature binds its default keys in code, so only the app knows them.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Command;
use sawit_core::keymap::{BindingRef, KeymapList, KeymapOp};
use serde_json::{Value, json};

use crate::app::pretty;
use crate::{Sawit, fail, ok};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct KeymapListArgs {
    #[schemars(
        description = "Words that must all be in a binding's action, label, keys, context or args (any case), such as `close tab`, `split` or `Terminal`"
    )]
    pub filter: Option<String>,
    #[schemars(
        description = "Only the bindings whose keys start with these keystrokes, written as keymap.json writes them: cmd-k, cmd-shift-p, ctrl-alt-left (modifiers in any order)"
    )]
    pub keys: Option<String>,
    #[schemars(
        description = "Also list the actions no key runs, which can then be bound (there are many: give a filter too)"
    )]
    #[serde(default)]
    pub unbound: bool,
}

/// A binding named as keymap_list shows it.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BindingRefArgs {
    #[schemars(description = "Its keys, as keymap_list shows them (cmd-k w)")]
    pub keys: String,
    #[schemars(
        description = "Its context, as keymap_list shows it; leave it out when no other binding has these keys, or give \"\" for one that works anywhere"
    )]
    pub context: Option<String>,
    #[schemars(description = "Its action, as keymap_list shows it")]
    pub action: String,
    #[schemars(description = "Its args, when keymap_list shows some")]
    pub args: Option<Value>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct KeymapBindArgs {
    #[schemars(
        description = "Keystrokes as keymap.json writes them: cmd-shift-w, ctrl-alt-left, or a chord such as cmd-k cmd-s"
    )]
    pub keys: String,
    #[schemars(
        description = "The action the keys run, as keymap_list shows actions: workspace::CloseAllTabs (list with unbound true to find one no key runs yet)"
    )]
    pub action: String,
    #[schemars(
        description = "The action's arguments, for one that takes them, such as {\"id\": \"tool.move\"} for board::Command"
    )]
    pub args: Option<Value>,
    #[schemars(
        description = "Where the keys work, as keymap_list shows contexts: Workspace, Editor, Terminal, Editor && !Input, … Default: anywhere"
    )]
    pub context: Option<String>,
    #[schemars(
        description = "Change this binding to the new keys, action and context instead of adding one, as the Keymap Editor's Edit does: a default is turned off and the new binding added"
    )]
    pub replace: Option<BindingRefArgs>,
    #[schemars(
        description = "Write keymap.json even though it has comments, which the rewrite drops (tell the user first)"
    )]
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct KeymapUnbindArgs {
    #[schemars(description = "The binding's keys, as keymap_list shows them (cmd-k w)")]
    pub keys: String,
    #[schemars(
        description = "Its context, as keymap_list shows it, when other bindings have these keys (\"\" for one that works anywhere)"
    )]
    pub context: Option<String>,
    #[schemars(description = "Its action, when other bindings have these keys")]
    pub action: Option<String>,
    #[schemars(description = "Its args, when other bindings of the action have these keys")]
    pub args: Option<Value>,
    #[schemars(
        description = "Bring back a default the user turned off (keymap_list shows it as removed) instead"
    )]
    #[serde(default)]
    pub restore: bool,
    #[schemars(
        description = "Write keymap.json even though it has comments, which the rewrite drops (tell the user first)"
    )]
    #[serde(default)]
    pub confirm: bool,
}

/// `keymap_list`'s answer: the file, its errors and the rows, one row a line. There are hundreds
/// of rows, and pretty-printed they pass the 100 KB a tool may answer.
fn list_text(list: &KeymapList) -> String {
    let rows: Vec<String> = list
        .rows
        .iter()
        .map(|row| format!("    {}", json!(row)))
        .collect();
    format!(
        "{{\n  \"file\": {},\n  \"writable\": {},\n  \"errors\": {},\n  \"count\": {},\n  \"rows\": [\n{}\n  ]\n}}",
        json!(list.path),
        list.writable,
        json!(list.errors),
        list.rows.len(),
        rows.join(",\n")
    )
}

impl Sawit {
    /// Sends keymap op `op` to the running app.
    async fn keymap(&self, op: KeymapOp) -> Result<Value, String> {
        self.app_call(None, Command::Keymap { op }).await
    }
}

#[tool_router(router = keymap_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Sawit's key bindings as the Keymap Editor lists them: each row's action, label, args, keys (as keymap.json writes them: cmd-k cmd-s), display (⌘K ⌘S), context, source (default; user: in keymap.json; removed: a default the user turned off; unbound) and conflict (true when another binding runs on the same keys in that context: only one of them runs), with keymap.json's path and its errors. Answers what a key does (keys) or which key runs something (filter). There are hundreds of rows: pass filter or keys. Read-only; needs the app running, since the defaults live in it"
    )]
    async fn keymap_list(&self, Parameters(a): Parameters<KeymapListArgs>) -> CallToolResult {
        let op = KeymapOp::List {
            filter: a.filter,
            keys: a.keys,
            unbound: a.unbound,
        };
        let reply = match self.keymap(op).await {
            Ok(reply) => reply,
            Err(e) => return fail(e),
        };
        match serde_json::from_value::<KeymapList>(reply) {
            Ok(list) => ok(list_text(&list)),
            Err(e) => fail(format!(
                "Sawit answered with a list this sawit-mcp can't read: {e}"
            )),
        }
    }

    #[tool(
        description = "DESTRUCTIVE: changes the user's key bindings. Binds keys to an action, as the Keymap Editor's Create Keybinding does, or with `replace` changes a binding (a default is turned off and the new binding added). The keys, the action with its args and the context are checked as the editor checks them, and nothing is written when one is wrong. Another binding of the same keys in the same context stays, and the new one runs first: the reply gives the binding as keymap_list lists it and notes on what it clashes with, to tell the user. It writes keymap.json again, whole and without comments: a file with comments is refused unless confirm is true. Needs the app running"
    )]
    async fn keymap_bind(&self, Parameters(a): Parameters<KeymapBindArgs>) -> CallToolResult {
        let op = KeymapOp::Bind {
            keys: a.keys,
            action: a.action,
            args: a.args,
            context: a.context,
            replace: a.replace.map(|old| {
                Box::new(BindingRef {
                    keys: old.keys,
                    context: old.context,
                    action: old.action,
                    args: old.args,
                })
            }),
            confirm: a.confirm,
        };
        match self.keymap(op).await {
            Ok(reply) => ok(pretty(&reply)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "DESTRUCTIVE: changes the user's key bindings. Takes a binding out, as the Keymap Editor's remove button does: the user's own leaves keymap.json, and a default is turned off there (keymap_list then shows it as removed); restore true brings a turned-off default back. Name it by its keys, and its context, action or args when several bindings have those keys. It writes keymap.json again, whole and without comments: a file with comments is refused unless confirm is true. Needs the app running"
    )]
    async fn keymap_unbind(&self, Parameters(a): Parameters<KeymapUnbindArgs>) -> CallToolResult {
        let op = KeymapOp::Unbind {
            keys: a.keys,
            context: a.context,
            action: a.action,
            args: a.args,
            restore: a.restore,
            confirm: a.confirm,
        };
        match self.keymap(op).await {
            Ok(reply) => ok(pretty(&reply)),
            Err(e) => fail(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    use rmcp::handler::server::wrapper::Parameters;
    use sawit_core::app_control::{self, Command, Request};
    use sawit_core::instances::AppPaths;
    use sawit_core::keymap::{BindingRef, KeySource, KeymapList, KeymapOp, KeymapRow};
    use serde_json::{Value, json};

    use super::*;

    /// A server whose Sawit home is in `dir`, so that no app answers on its socket but the one a
    /// test stands in.
    fn server(dir: &Path) -> Sawit {
        Sawit::new(AppPaths {
            root: dir.join("home"),
        })
    }

    /// A stand-in for the running app that answers each request with the next of `answers`; the
    /// requests come out of the receiver.
    fn fake_app(paths: &AppPaths, answers: Vec<Value>) -> Receiver<Request> {
        std::fs::create_dir_all(&paths.root).unwrap();
        let listener = UnixListener::bind(paths.cli_socket()).unwrap();
        let (asked, requests) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut answers = answers.into_iter();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut bytes = Vec::new();
                if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                    continue;
                }
                let Some(Ok(request)) = app_control::decode(&bytes) else {
                    break;
                };
                let _ = asked.send(request);
                let Some(answer) = answers.next() else { break };
                let line = format!("{}\n", app_control::reply_ok(&answer));
                let _ = stream.write_all(line.as_bytes());
            }
        });
        requests
    }

    fn said(result: &CallToolResult) -> String {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    }

    fn asked(requests: &Receiver<Request>) -> Command {
        requests
            .recv_timeout(Duration::from_secs(5))
            .expect("the app was asked")
            .command
    }

    fn row(action: &str, keys: &str, source: KeySource) -> KeymapRow {
        KeymapRow {
            action: action.into(),
            label: action.to_lowercase(),
            args: None,
            keys: keys.into(),
            display: keys.to_uppercase(),
            context: Some("Workspace".into()),
            source,
            conflict: false,
        }
    }

    /// keymap_list asks the app for the rows the agent filters, and answers with JSON that reads
    /// back as the app's list: one row a line, so that hundreds of rows stay short.
    #[tokio::test]
    async fn keymap_list_answers_the_apps_rows_one_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let sawit = server(dir.path());
        let list = KeymapList {
            path: "/home/keymap.json".into(),
            writable: true,
            errors: vec![],
            rows: vec![
                row("workspace::CloseAllTabs", "cmd-k w", KeySource::Default),
                KeymapRow {
                    conflict: true,
                    args: Some(json!({"id": "tool.move"})),
                    ..row("board::Command", "cmd-k t", KeySource::User)
                },
            ],
        };
        let requests = fake_app(&sawit.paths, vec![json!(list)]);
        let answer = sawit
            .keymap_list(Parameters(KeymapListArgs {
                filter: None,
                keys: Some("cmd-k".into()),
                unbound: false,
            }))
            .await;
        assert_ne!(answer.is_error, Some(true), "{}", said(&answer));
        assert_eq!(
            asked(&requests),
            Command::Keymap {
                op: KeymapOp::List {
                    filter: None,
                    keys: Some("cmd-k".into()),
                    unbound: false,
                }
            }
        );
        let text = said(&answer);
        assert_eq!(text.lines().count(), 10, "{text}");
        let read: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(read["count"], 2);
        assert_eq!(read["file"], "/home/keymap.json");
        let rows: Vec<KeymapRow> = serde_json::from_value(read["rows"].clone()).unwrap();
        assert_eq!(rows, list.rows);
        // An empty list is still JSON.
        let empty = list_text(&KeymapList {
            rows: vec![],
            ..list
        });
        assert_eq!(serde_json::from_str::<Value>(&empty).unwrap()["count"], 0);
    }

    /// keymap_bind and keymap_unbind send what the agent names, `replace` and `confirm`
    /// included, and answer with the app's reply; an error the app gives is the tool's error.
    #[tokio::test]
    async fn keymap_bind_and_unbind_send_what_the_agent_names() {
        let dir = tempfile::tempdir().unwrap();
        let sawit = server(dir.path());
        let requests = fake_app(
            &sawit.paths,
            vec![
                json!({"bound": {"keys": "cmd-shift-w"}}),
                json!({"turned_off": {"keys": "cmd-k w"}}),
            ],
        );
        let bound = sawit
            .keymap_bind(Parameters(KeymapBindArgs {
                keys: "cmd-shift-w".into(),
                action: "workspace::CloseAllTabs".into(),
                args: None,
                context: Some("Workspace".into()),
                replace: Some(BindingRefArgs {
                    keys: "cmd-k w".into(),
                    context: None,
                    action: "workspace::CloseAllTabs".into(),
                    args: None,
                }),
                confirm: true,
            }))
            .await;
        assert!(said(&bound).contains("cmd-shift-w"), "{}", said(&bound));
        assert_eq!(
            asked(&requests),
            Command::Keymap {
                op: KeymapOp::Bind {
                    keys: "cmd-shift-w".into(),
                    action: "workspace::CloseAllTabs".into(),
                    args: None,
                    context: Some("Workspace".into()),
                    replace: Some(Box::new(BindingRef {
                        keys: "cmd-k w".into(),
                        context: None,
                        action: "workspace::CloseAllTabs".into(),
                        args: None,
                    })),
                    confirm: true,
                }
            }
        );
        let unbound = sawit
            .keymap_unbind(Parameters(KeymapUnbindArgs {
                keys: "cmd-k w".into(),
                context: None,
                action: None,
                args: None,
                restore: false,
                confirm: false,
            }))
            .await;
        assert!(said(&unbound).contains("turned_off"), "{}", said(&unbound));
        assert!(matches!(
            asked(&requests),
            Command::Keymap {
                op: KeymapOp::Unbind {
                    restore: false,
                    confirm: false,
                    ..
                }
            }
        ));
    }

    /// With no app running the keymap tools say so: the defaults live in the app.
    #[tokio::test]
    async fn the_keymap_tools_need_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let sawit = server(dir.path());
        let answer = sawit
            .keymap_list(Parameters(KeymapListArgs {
                filter: None,
                keys: None,
                unbound: false,
            }))
            .await;
        assert_eq!(answer.is_error, Some(true));
        assert!(said(&answer).contains("isn't running"), "{}", said(&answer));
    }
}
