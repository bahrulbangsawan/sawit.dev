//! The terminal tools: how long `terminal_run` waits, which terminal and pane each names, and
//! what each asks the app.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars;
use sawit_core::app_control::{self, Command, Request};
use serde_json::json;

use super::{fake_app_answering, native, said};
use crate::app::terminal::{MAX_WAIT, run_wait};
use crate::app::{
    TerminalCloseArgs, TerminalInputArgs, TerminalReadArgs, TerminalRenameArgs, TerminalRunArgs,
    TerminalSplitArgs,
};

/// `terminal_run` waits as asked, within 0 to 120 s, whatever number the agent sends: a
/// huge, infinite or NaN `wait_secs` must not panic the tool call, which would never answer.
#[test]
fn terminal_run_waits_within_bounds() {
    assert_eq!(run_wait(None), Duration::from_secs(2));
    assert_eq!(run_wait(Some(0.5)), Duration::from_millis(500));
    assert_eq!(run_wait(Some(-3.0)), Duration::ZERO);
    assert_eq!(run_wait(Some(1e20)), MAX_WAIT);
    assert_eq!(run_wait(Some(f64::INFINITY)), MAX_WAIT);
    assert_eq!(run_wait(Some(f64::NEG_INFINITY)), Duration::ZERO);
    assert_eq!(run_wait(Some(f64::NAN)), Duration::from_secs(2));
}

/// Reading or typing into a split tab names the pane: its focused pane may be the one the user
/// types in, while the agent's command runs in the other. Closing without one closes the tab.
#[test]
fn only_closing_defaults_to_the_whole_tab() {
    let pane = |schema: schemars::Schema| {
        schema.as_value()["properties"]["pane"]["description"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    for read_or_input in [
        pane(schemars::schema_for!(TerminalReadArgs)),
        pane(schemars::schema_for!(TerminalInputArgs)),
    ] {
        assert!(
            read_or_input.contains("needed when the tab is split"),
            "{read_or_input}"
        );
        assert!(!read_or_input.contains("whole tab"), "{read_or_input}");
    }
    let close = pane(schemars::schema_for!(TerminalCloseArgs));
    assert!(close.contains("the whole tab"), "{close}");
    assert!(!close.contains("focused pane"), "{close}");
}

/// Keys go only to a terminal the agent names: `terminal_input` needs the tab, as
/// `terminal_close` does. Sent to "the active terminal", a ctrl-c meant for the agent's dev
/// server would interrupt the program the user types in, Claude Code included. Reading still
/// defaults to the terminal the user has in front.
#[test]
fn terminal_input_needs_the_tab() {
    let required = |schema: schemars::Schema| {
        let required = schema.as_value()["required"].as_array().cloned();
        required.unwrap_or_default().contains(&json!("tab"))
    };
    assert!(required(schemars::schema_for!(TerminalInputArgs)));
    assert!(required(schemars::schema_for!(TerminalCloseArgs)));
    assert!(!required(schemars::schema_for!(TerminalReadArgs)));
    let untargeted = serde_json::from_value::<TerminalInputArgs>(json!({ "key": "ctrl-c" }));
    assert!(untargeted.is_err(), "keys went out with no terminal named");
    let tab = |schema: schemars::Schema| {
        schema.as_value()["properties"]["tab"]["description"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    let read = tab(schemars::schema_for!(TerminalReadArgs));
    assert!(read.contains("the user has in front"), "{read}");
    assert!(read.contains("terminal_run"), "{read}");
}

/// `terminal_run` reads back the pane the app says the command went to: it needn't be the
/// tab's focused pane, which the app leaves alone while the user types in a terminal.
#[tokio::test]
async fn terminal_run_reads_the_pane_it_ran_in() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let asked = fake_app_answering(
        &server.paths,
        vec![
            json!({"terminal": 2, "pane": 1}),
            json!({"text": "$ make test\nok"}),
        ],
    );
    let ran = server
        .terminal_run(Parameters(TerminalRunArgs {
            command: "make test".into(),
            cwd: None,
            new_tab: false,
            wait_secs: Some(0.0),
            lines: None,
            project: None,
        }))
        .await;
    assert_eq!(said(&ran), "Terminal 2, pane 1:\n$ make test\nok");
    let request = |asked: &Receiver<Request>| {
        asked
            .recv_timeout(Duration::from_secs(5))
            .expect("the app was asked")
            .command
    };
    assert!(matches!(request(&asked), Command::TerminalRun { .. }));
    assert_eq!(
        request(&asked),
        Command::TerminalRead {
            tab: Some(2),
            pane: Some(1),
            lines: Some(100),
        }
    );
}

/// `terminal_split` asks the app to split the pane it names the way it names, then reads back
/// the new pane the app says it made, as `terminal_run` reads the pane its command went to.
/// A way that isn't right or down, or a folder that isn't one, is refused before the app is
/// asked.
#[tokio::test]
async fn terminal_split_reads_the_new_pane() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let asked = fake_app_answering(
        &server.paths,
        vec![
            json!({"terminal": 2, "pane": 1}),
            json!({"text": "$ npm test -- --watch\nwatching"}),
        ],
    );
    let split = |direction: &str, cwd: Option<&str>| TerminalSplitArgs {
        tab: 2,
        pane: Some(0),
        direction: direction.into(),
        command: Some("npm test -- --watch".into()),
        cwd: cwd.map(String::from),
        wait_secs: Some(0.0),
        lines: Some(20),
        project: None,
    };
    let request = |asked: &Receiver<Request>| {
        asked
            .recv_timeout(Duration::from_secs(5))
            .expect("the app was asked")
            .command
    };
    let folder = dir.path().join("web");
    std::fs::create_dir_all(&folder).unwrap();
    let ran = server
        .terminal_split(Parameters(split(
            " Down ",
            Some(&folder.display().to_string()),
        )))
        .await;
    assert_eq!(
        said(&ran),
        "Terminal 2, pane 1:\n$ npm test -- --watch\nwatching"
    );
    assert_eq!(
        request(&asked),
        Command::TerminalSplit {
            tab: 2,
            pane: Some(0),
            direction: app_control::SplitDirection::Down,
            command: Some("npm test -- --watch".into()),
            cwd: Some(folder),
        }
    );
    assert_eq!(
        request(&asked),
        Command::TerminalRead {
            tab: Some(2),
            pane: Some(1),
            lines: Some(20),
        }
    );
    for (direction, cwd, why) in [
        ("left", None, "right, down"),
        ("right", Some("web"), "Nothing was split"),
        ("right", Some("/no/such/folder"), "Nothing was split"),
    ] {
        let refused = server
            .terminal_split(Parameters(split(direction, cwd)))
            .await;
        assert_eq!(
            refused.is_error,
            Some(true),
            "{direction}: {}",
            said(&refused)
        );
        assert!(
            said(&refused).contains(why),
            "{direction}: {}",
            said(&refused)
        );
    }
    assert!(
        asked.recv_timeout(Duration::from_millis(300)).is_err(),
        "the app was asked to split with a way or a folder that isn't one"
    );
}

/// `terminal_close` passes the agent's `confirm` on: the app refuses an unconfirmed close
/// that would end what runs there, and a `sawit-mcp` that dropped it could never close one.
#[tokio::test]
async fn terminal_close_passes_its_confirm_on() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let asked = fake_app_answering(
        &server.paths,
        vec![json!({"closed": 1}), json!({"closed": 1})],
    );
    for confirm in [false, true] {
        let args: TerminalCloseArgs = serde_json::from_value(match confirm {
            true => json!({"tab": 1, "pane": 0, "confirm": true}),
            false => json!({"tab": 1, "pane": 0}),
        })
        .unwrap();
        server.terminal_close(Parameters(args)).await;
        let request = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
        assert_eq!(
            request.command,
            Command::TerminalClose {
                tab: 1,
                pane: Some(0),
                confirm,
            }
        );
    }
}

/// `terminal_rename` names the tab, or with a pane that pane, and passes the name on as given: a
/// blank one clears it. Without a name it doesn't read, so the app isn't asked.
#[tokio::test]
async fn terminal_rename_names_the_tab_or_its_pane() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let asked = fake_app_answering(
        &server.paths,
        vec![
            json!({"terminal": 2, "pane": null, "named": "tab", "name": "dev server"}),
            json!({"terminal": 2, "pane": 1, "named": "pane", "name": "zsh"}),
        ],
    );
    for (args, tab, pane, name) in [
        (
            json!({"tab": 2, "name": "dev server"}),
            2,
            None,
            "dev server",
        ),
        (json!({"tab": 2, "pane": 1, "name": ""}), 2, Some(1), ""),
    ] {
        let args: TerminalRenameArgs = serde_json::from_value(args).unwrap();
        let r = server.terminal_rename(Parameters(args)).await;
        assert_ne!(r.is_error, Some(true), "{}", said(&r));
        let request = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
        assert_eq!(
            request.command,
            Command::TerminalRename {
                tab,
                pane,
                name: name.into(),
            }
        );
    }
    assert!(serde_json::from_value::<TerminalRenameArgs>(json!({"tab": 2})).is_err());
}

/// MCP-4: `terminal_run` starts its shell in the folder it names, or not at all. The PTY
/// ignores a `chdir` that fails, so a folder with `~`, a relative one or one that isn't there
/// started the shell in the app's own folder (`/` for Sawit.app), and the command ran there
/// though the tool answered ok. `~` is the home folder; a relative or missing folder is an
/// error, and the app isn't asked.
#[tokio::test]
async fn terminal_run_starts_where_it_says_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let asked = fake_app_answering(
        &server.paths,
        vec![
            json!({"terminal": 1, "pane": 0}),
            json!({"text": ""}),
            json!({"terminal": 2, "pane": 0}),
            json!({"text": ""}),
        ],
    );
    let run = |cwd: &str| TerminalRunArgs {
        command: "rm -rf build".into(),
        cwd: Some(cwd.into()),
        new_tab: false,
        wait_secs: Some(0.0),
        lines: None,
        project: None,
    };
    let cwd_asked = |asked: &Receiver<Request>| {
        let request = asked.recv_timeout(Duration::from_secs(5)).expect("asked");
        let Command::TerminalRun { cwd, .. } = request.command else {
            panic!("{:?}", request.command)
        };
        asked.recv_timeout(Duration::from_secs(5)).expect("read");
        cwd
    };

    // A folder that is there, written with spaces around it, and the home folder as `~`.
    let folder = dir.path().join("web");
    std::fs::create_dir_all(&folder).unwrap();
    let ran = server
        .terminal_run(Parameters(run(&format!(" {} ", folder.display()))))
        .await;
    assert_ne!(ran.is_error, Some(true), "{}", said(&ran));
    assert_eq!(cwd_asked(&asked), Some(folder));
    if let Some(home) = std::env::var_os("HOME") {
        let ran = server.terminal_run(Parameters(run("~"))).await;
        assert_ne!(ran.is_error, Some(true), "{}", said(&ran));
        assert_eq!(cwd_asked(&asked), Some(PathBuf::from(home)));
    }

    for (cwd, why) in [
        ("web", "must be an absolute path"),
        ("/no/such/folder", "is not a folder"),
    ] {
        let refused = server.terminal_run(Parameters(run(cwd))).await;
        assert_eq!(refused.is_error, Some(true), "{cwd}: {}", said(&refused));
        assert!(said(&refused).contains(why), "{cwd}: {}", said(&refused));
    }
    assert!(
        asked.recv_timeout(Duration::from_millis(300)).is_err(),
        "the app was asked to run in a folder that isn't one"
    );
}
