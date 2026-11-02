//! The protocol's tests: requests and replies as they go over the socket, and the key names.

use super::*;

#[test]
fn round_trip() {
    let request = Request {
        project: Some("/code/app".into()),
        agent_folder: None,
        agent: None,
        command: Command::TerminalRun {
            command: "npm test".into(),
            cwd: None,
            new_tab: true,
        },
    };
    let bytes = encode(&request);
    assert_eq!(decode(&bytes), Some(Ok(request)));
}

/// The agent that sent an op goes with it as `{id, name, label}` and comes back as it went.
/// An op without one, as an older `sawit-mcp` sends it, still reads, and has none; and the
/// field is one an app that predates it reads past, as it reads past any field it doesn't
/// know.
#[test]
fn the_agent_round_trips_and_an_op_without_one_still_works() {
    let request = Request {
        project: None,
        agent_folder: Some("/code/shop".into()),
        agent: Some(crate::agents::AgentIdentity {
            id: "mcp-4821-0badf00d".into(),
            name: "Claude Code".into(),
            label: Some("shop".into()),
            version: None,
        }),
        command: Command::Canvas {
            op: serde_json::json!({"op": "get"}),
            file: None,
        },
    };
    let bytes = encode(&request);
    assert_eq!(
            &bytes[MAGIC.len()..],
            br#"{"agent_folder":"/code/shop","agent":{"id":"mcp-4821-0badf00d","name":"Claude Code","label":"shop"},"cmd":"canvas","op":{"op":"get"}}"#
        );
    assert_eq!(decode(&bytes), Some(Ok(request)));
    let old = decode(
        br#"sawit-control/1
{"cmd":"browser","op":{"op":"click","selector":"button"}}"#,
    );
    assert!(matches!(
        old,
        Some(Ok(Request {
            agent: None,
            command: Command::Browser { .. },
            ..
        }))
    ));
    // Every kind of command reads with an agent beside it, the ones without fields too.
    for wire in [
        r#"{"cmd":"state","agent":{"id":"a","name":"Codex"}}"#,
        r#"{"cmd":"focus","agent":{"id":"a","name":"Codex","version":"0.40"}}"#,
        r#"{"cmd":"show_panel","panel":"git","agent":{"id":"a","name":"Codex"}}"#,
    ] {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(wire.as_bytes());
        let read = decode(&bytes)
            .unwrap()
            .unwrap_or_else(|e| panic!("{wire}: {e}"));
        assert_eq!(read.agent.map(|a| a.name), Some("Codex".into()), "{wire}");
    }
    // What an app that predates the field does with it: a field it doesn't know is read past.
    let unknown = decode(
        br#"sawit-control/1
{"cmd":"focus","from_the_future":{"id":"a"}}"#,
    );
    assert!(matches!(
        unknown,
        Some(Ok(Request {
            command: Command::Focus,
            ..
        }))
    ));
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
        agent: None,
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
        agent: None,
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
        agent: None,
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
        agent: None,
        command: Command::OpenPage { page: Page::Skills },
    };
    let bytes = encode(&skills);
    assert_eq!(
        &bytes[MAGIC.len()..],
        br#"{"cmd":"open_page","page":"skills"}"#
    );
    assert_eq!(decode(&bytes), Some(Ok(skills)));
}

/// The New Project page opens by name, and an agent's create carries its stack with
/// upstream's ids.
#[test]
fn new_project_round_trips() {
    let page = Request {
        project: None,
        agent_folder: None,
        agent: None,
        command: Command::OpenPage {
            page: Page::NewProject,
        },
    };
    let bytes = encode(&page);
    assert_eq!(
        &bytes[MAGIC.len()..],
        br#"{"cmd":"open_page","page":"new_project"}"#
    );
    assert_eq!(decode(&bytes), Some(Ok(page)));
    let stack = crate::stack_builder::Stack {
        project_name: "shop".into(),
        backend: crate::stack_builder::Backend::Convex,
        ..Default::default()
    };
    let create = Request {
        project: None,
        agent_folder: None,
        agent: None,
        command: Command::NewProject {
            location: "/Users/me/Developer".into(),
            stack,
        },
    };
    let bytes = encode(&create);
    let wire = String::from_utf8_lossy(&bytes[MAGIC.len()..]).into_owned();
    assert!(
        wire.starts_with(r#"{"cmd":"new_project","location":"/Users/me/Developer","stack":{"#),
        "{wire}"
    );
    assert!(wire.contains(r#""backend":"convex""#), "{wire}");
    assert_eq!(decode(&bytes), Some(Ok(create)));
    // A stack given in part reads as the default for the rest.
    let partial = br#"sawit-control/1
{"cmd":"new_project","location":"/tmp","stack":{"backend":"express"}}"#;
    let Some(Ok(Request {
        command: Command::NewProject { stack, .. },
        ..
    })) = decode(partial)
    else {
        panic!("a partial stack didn't read");
    };
    assert_eq!(stack.backend, crate::stack_builder::Backend::Express);
    assert_eq!(stack.runtime, crate::stack_builder::Runtime::Bun);
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
            agent: None,
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
        agent: None,
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
            agent: None,
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
        agent: None,
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
            agent: None,
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
        agent: None,
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
        agent: None,
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
