//! The app tools' tests, and the stand-ins they share with the canvas tools' tests: a server with
//! a Sawit home of its own, and an app that answers on its socket.

mod requests;
mod terminal;

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::mpsc::Receiver;

use rmcp::model::CallToolResult;
use sawit_core::app_control::{self, Request};
use sawit_core::instances::AppPaths;
use serde_json::Value;

use crate::Sawit;

// The canvas tools, with the app and without it. Each test gives its server a Sawit home of
// its own, so that no app answers on its socket but the one a test stands in, and names its
// project folder, so that nothing is written under the repository.

/// A server whose home is in `dir`.
pub(super) fn native(dir: &Path) -> Sawit {
    Sawit::new(AppPaths {
        root: dir.join("home"),
    })
}

/// The text of a tool's result.
pub(super) fn said(result: &CallToolResult) -> String {
    let texts = result.content.iter().filter_map(|c| c.as_text());
    texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
}

/// The JSON a tool that succeeded answered.
pub(super) fn answer(result: &CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "{}", said(result));
    serde_json::from_str(&said(result)).unwrap()
}

/// A stand-in for the running app that answers any command, each with the next of `answers`.
/// The requests come out of the receiver.
pub(super) fn fake_app_answering(paths: &AppPaths, answers: Vec<Value>) -> Receiver<Request> {
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
