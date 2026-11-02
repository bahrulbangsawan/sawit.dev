//! The canvas tools with Sawit running: every op goes to the app, and its answers become theirs.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc::Receiver;

use rmcp::handler::server::wrapper::Parameters;
use sawit_core::app_control::{self, Command, Request};
use sawit_core::instances::AppPaths;

use super::*;
use crate::app::canvas::dispatch::op_reply;
use crate::app::canvas::files::FILE_PARAMS;
use crate::app::canvas::{
    CanvasCommandArgs, CanvasCommandsArgs, CanvasElementsArgs, CanvasTargetArgs,
};
use crate::app::tests::{answer, native, said};

/// A stand-in for the running app: it listens where the app does, and answers each canvas
/// request with the next of `answers` (`{ok: true}` once they are used up). The ops it was
/// asked to run come out of the receiver.
fn fake_app(paths: &AppPaths, answers: Vec<Value>) -> Receiver<Value> {
    std::fs::create_dir_all(&paths.root).unwrap();
    let listener = UnixListener::bind(paths.cli_socket()).unwrap();
    let (asked, ops) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut answers = answers.into_iter();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut bytes = Vec::new();
            // `is_running` only connects, and sends nothing.
            if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                continue;
            }
            let Some(Ok(Request {
                command: Command::Canvas { op, .. },
                ..
            })) = app_control::decode(&bytes)
            else {
                break;
            };
            let _ = asked.send(op);
            let answer = answers.next().unwrap_or_else(|| json!({"ok": true}));
            let line = format!("{}\n", app_control::reply_ok(&answer));
            let _ = stream.write_all(line.as_bytes());
        }
    });
    ops
}

/// The next op the stand-in app was asked for. It is waited for, but not for ever: a tool that
/// didn't go to the app fails the test instead of hanging it.
fn next(asked: &Receiver<Value>) -> Value {
    asked
        .recv_timeout(Duration::from_secs(5))
        .expect("the app was asked for an op")
}

#[tokio::test]
async fn a_running_app_answers_every_canvas_tool() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = dir.path().join("project");
    write_board(&board_file(&project), 3);
    let asked = fake_app(
        &server.paths,
        vec![
            json!({"ok": true, "elements": [{"id": "7", "type": "sticky"}]}),
            json!({"ok": false, "error": "add: the engine said no"}),
            json!({"ok": true, "result": {"id": "9"}}),
            json!({"ok": true, "commands": [{"id": "a.b", "label": "A B", "params": "{}"}]}),
        ],
    );

    // The app's answer to `get`, though the native canvas is on and the file could be read:
    // the app is the one that has the canvas.
    let got = server
        .canvas_get_tool(CanvasTargetArgs {
            file: None,
            project: project_arg(&project),
        })
        .await;
    assert_eq!(answer(&got)["elements"].as_array().unwrap().len(), 1);
    assert_eq!(next(&asked), json!({"op": "get"}));

    // An op the app answers with `{ok: false, error}` is the tool's error, in the engine's words.
    let failed = server
        .canvas_add_elements(Parameters(CanvasElementsArgs {
            elements: vec![json!({"type": "rectangle"})],
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_eq!(failed.is_error, Some(true), "{}", said(&failed));
    assert_eq!(said(&failed), "add: the engine said no");
    assert_eq!(next(&asked)["op"], json!("add"));

    let ran = server
        .canvas_command_tool(CanvasCommandArgs {
            command: "board.sticky".into(),
            params: Some(json!({"x": 0, "y": 0})),
            file: None,
            project: project_arg(&project),
        })
        .await;
    assert_eq!(answer(&ran)["result"]["id"], json!("9"));
    let op = json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}});
    assert_eq!(next(&asked), op);

    let listed = server
        .canvas_commands(Parameters(CanvasCommandsArgs {
            filter: None,
            file: None,
            project: project_arg(&project),
        }))
        .await;
    assert_eq!(answer(&listed)["commands"][0]["id"], json!("a.b"));
    assert_eq!(next(&asked), json!({"op": "commands"}));

    // None of it went to the file behind the app's back.
    assert_eq!(layers_in(&board_file(&project)).len(), 3);
}

/// MCP-8: the file or folder an engine command names goes to the canvas as the agent means
/// it: `~` for the home folder, and a relative one from the agent's folder. As written it was
/// the app's folder's (`/` for Sawit.app) with the app running, and `~/…` was no folder at
/// all. What isn't a file is left as it is: SVG path data, a layer's id, a share link, a
/// number.
#[tokio::test]
async fn canvas_command_sends_the_files_it_names_as_the_agent_means_them() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    let asked = fake_app(&server.paths, Vec::new());
    let here = std::env::current_dir().unwrap();
    let sent = |command: &str, params: Value| {
        let run = server.canvas_command_tool(CanvasCommandArgs {
            command: command.into(),
            params: Some(params),
            file: None,
            project: project_arg(&project),
        });
        async {
            let ran = run.await;
            assert_ne!(ran.is_error, Some(true), "{}", said(&ran));
            next(&asked)["params"].clone()
        }
    };

    let params = sent("file.exportSvg", json!({"path": " rel/page.svg "})).await;
    assert_eq!(params["path"], json!(here.join("rel/page.svg")));
    let params = sent("file.exportLayers", json!({"dir": "out", "ids": [1]})).await;
    assert_eq!(params, json!({"dir": here.join("out"), "ids": [1]}));
    if let Some(home) = std::env::var_os("HOME") {
        let params = sent("image.place", json!({"path": "~/Desktop/a.png"})).await;
        let placed = Path::new(&home).join("Desktop").join("a.png");
        assert_eq!(params["path"], json!(placed));
    }
    for (command, params) in [
        ("vector.create", json!({"path": "M0 0 L10 0", "x": 0})),
        ("board.ink", json!({"path": "M0 0 L10 10"})),
        ("text.onPath", json!({"path": "12"})),
        ("library.add", json!({"path": "https://share.example/f/1"})),
        ("comment.next", json!({"dir": 1})),
        ("file.exportSvg", json!({"path": "/abs/page.svg"})),
    ] {
        assert_eq!(sent(command, params.clone()).await, params, "{command}");
    }
    let batch = json!({"commands": [
        {"command": "file.exportSvg", "params": {"path": "b.svg"}},
        {"command": "vector.create", "params": {"path": "M0 0"}},
        {"command": "file.exportSvg", "params": {"$step": 0}},
    ]});
    let steps = sent("command.batch", batch).await["commands"].clone();
    assert_eq!(steps[0]["params"]["path"], json!(here.join("b.svg")));
    assert_eq!(steps[1]["params"]["path"], json!("M0 0"));
    assert_eq!(steps[2]["params"], json!({"$step": 0}));
}

/// Every command [`FILE_PARAMS`] names is one canvas_command runs, and takes that parameter:
/// a command the engine renamed or dropped would leave its files read from the app's folder.
#[test]
fn the_commands_that_take_a_file_are_the_engines() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let reply = ops::run(&mut board, &json!({"op": "commands"}));
    let commands = reply["commands"].as_array().unwrap();
    for (id, key) in FILE_PARAMS {
        let command = commands.iter().find(|c| c["id"] == json!(id));
        let params = command.map(|c| c["params"].as_str().unwrap_or_default());
        let params = params.unwrap_or_else(|| panic!("{id} is no command"));
        assert!(params.contains(key), "{id} takes no {key}: {params}");
    }
}

#[test]
fn a_failed_reply_is_an_error_and_any_other_is_the_answer() {
    assert_eq!(
        op_reply(json!({"ok": false, "error": "add: no"})),
        Err("add: no".to_string())
    );
    assert_eq!(
        op_reply(json!({"ok": false})),
        Err("The canvas op failed.".to_string())
    );
    let fine = json!({"ok": true, "added": 1});
    assert_eq!(op_reply(fine.clone()), Ok(fine));
    // The old canvas's page leaves `ok` out of what it answers.
    let old = json!({"count": 2, "elements": []});
    assert_eq!(op_reply(old.clone()), Ok(old));
    assert_eq!(op_reply(Value::Null), Ok(Value::Null));
}
