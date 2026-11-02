//! The browser tools against a stand-in for the running app: what they refuse, how they wait,
//! where their files go, and what they tell an agent.

use serde_json::{Value, json};

use super::*;

#[test]
fn resize_args_need_a_device_or_size() {
    let a = ResizeArgs {
        device: None,
        width: None,
        height: None,
        project: None,
    };
    let e = resize_mode(a.device.as_deref(), a.width, a.height).unwrap_err();
    assert!(!e.is_empty());
    assert_eq!(Err(e), sawit_core::browser::resize_mode(None, None, None));
}

#[test]
fn navigation_ops() {
    assert_eq!(navigate_op(None, Some("back")), Ok(Some(BrowserOp::Back)));
    assert_eq!(
        navigate_op(Some("localhost:3000".into()), None),
        Ok(Some(BrowserOp::Navigate {
            url: "localhost:3000".into()
        }))
    );
    assert_eq!(navigate_op(Some("  ".into()), None), Ok(None));
    assert!(
        navigate_op(None, Some("up"))
            .unwrap_err()
            .contains("reload")
    );
}

#[test]
fn targets() {
    assert!(required(target(None, Some(" ".into()))).is_err());
    assert_eq!(
        required(target(Some("e4".into()), None))
            .unwrap()
            .reference
            .as_deref(),
        Some("e4")
    );
}

#[test]
fn timeouts_are_capped() {
    assert_eq!(timeout(Some(90_000), DEFAULT_LOAD_TIMEOUT), MAX_TIMEOUT);
    assert_eq!(timeout(None, DEFAULT_LOAD_TIMEOUT), DEFAULT_LOAD_TIMEOUT);
}

/// A stand-in for the running app, listening where the app does: it answers every request
/// with `answer`.
fn fake_app(paths: &sawit_core::instances::AppPaths, answer: Value) {
    use std::io::{Read as _, Write as _};
    std::fs::create_dir_all(&paths.root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut bytes = Vec::new();
            // `is_running` only connects, and sends nothing.
            if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                continue;
            }
            let line = format!("{}\n", sawit_core::app_control::reply_ok(&answer));
            let _ = stream.write_all(line.as_bytes());
        }
    });
}

/// MCP-7: `output_path` is read as every tool that writes a file reads one: trimmed, `~` for
/// the home folder, and absolute. A space at its end became part of the file's name, and a
/// relative path was written in whatever folder sawit-mcp ran in.
#[tokio::test]
async fn a_screenshot_path_is_read_as_every_tool_reads_one() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sawit_core::instances::AppPaths {
        root: dir.path().join("home"),
    };
    let server = Sawit::new(paths.clone());
    let shot = |output: String| {
        Parameters(ScreenshotArgs {
            reference: None,
            selector: None,
            output_path: Some(output),
            mesurer: None,
            project: None,
        })
    };
    let said = |result: &CallToolResult| {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    };

    // Refused before the app is asked: none runs yet, and nothing is written.
    let refused = server
        .browser_screenshot(shot("shots/page.png".into()))
        .await;
    assert_eq!(refused.is_error, Some(true));
    assert!(
        said(&refused).contains("must be absolute"),
        "{}",
        said(&refused)
    );

    fake_app(
        &paths,
        json!({"png_base64": "iVBORw0KGgo=", "width": 1, "height": 1}),
    );
    let png = dir.path().join("page.png");
    let taken = server
        .browser_screenshot(shot(format!(" {}  ", png.display())))
        .await;
    assert_ne!(taken.is_error, Some(true), "{}", said(&taken));
    assert_eq!(std::fs::read(&png).unwrap(), b"\x89PNG\r\n\x1a\n");
    assert!(said(&taken).ends_with(&format!("written to {}", png.display())));
}

/// MCP-10: with no url, browser_navigate opens the dev server of the project: the one named,
/// else the folder the agent runs in (as the canvas and app tools take it), else the project
/// of the front window, which the app shows the page in. It looked under `/`, so another
/// project's server opened, or with two running anywhere on the Mac none did.
#[tokio::test]
async fn the_dev_server_opened_is_the_projects() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sawit_core::instances::AppPaths {
        root: dir.path().join("home"),
    };
    let server = Sawit::new(paths.clone());
    let shop = dir.path().join("shop");
    std::fs::create_dir_all(&shop).unwrap();
    let shop = shop.canonicalize().unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&shop, &link).unwrap();
    let agent = dir.path().join("agent");

    let named = Some(link.display().to_string());
    let folder = server.dev_server_folder(&named, Some(agent.clone())).await;
    assert_eq!(folder, Ok(shop.clone()));
    let folder = server.dev_server_folder(&None, Some(agent.clone())).await;
    assert_eq!(folder, Ok(agent));

    // An agent run from no project: the front window's.
    let front = json!({"windows": [
        {"project": "/code/back", "frontmost": false},
        {"project": shop, "frontmost": true},
    ]});
    fake_app_answering(&paths, vec![Ok(front)]);
    let folder = server.dev_server_folder(&None, None).await;
    assert_eq!(folder, Ok(shop));
}

/// A stand-in for the running app that answers its requests with `answers` in turn, the last
/// one again once they are used up.
fn fake_app_answering(
    paths: &sawit_core::instances::AppPaths,
    answers: Vec<Result<Value, String>>,
) {
    use std::io::{Read as _, Write as _};
    std::fs::create_dir_all(&paths.root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
    std::thread::spawn(move || {
        let mut answers = answers.into_iter().peekable();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut bytes = Vec::new();
            if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                continue;
            }
            let answer = match answers.next() {
                Some(answer) if answers.peek().is_none() => {
                    answers = vec![answer.clone()].into_iter().peekable();
                    answer
                }
                Some(answer) => answer,
                None => break,
            };
            let line = match answer {
                Ok(value) => sawit_core::app_control::reply_ok(&value),
                Err(error) => sawit_core::app_control::reply_error(&error),
            };
            let _ = stream.write_all(format!("{line}\n").as_bytes());
        }
    });
}

/// MCP-9: browser_wait_for waits out only what waiting changes: a page that has no agent yet
/// while it loads. Any other answer comes back at once: with no app running, or a selector
/// that isn't one, it waited its whole timeout and then said it had timed out.
#[tokio::test]
async fn waiting_for_a_page_answers_what_waiting_wont_change_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sawit_core::instances::AppPaths {
        root: dir.path().join("home"),
    };
    let server = Sawit::new(paths.clone());
    let wait = |selector: &str| {
        Parameters(WaitArgs {
            text: None,
            text_gone: None,
            selector: Some(selector.into()),
            time_ms: None,
            timeout_ms: Some(3_000),
            project: None,
        })
    };
    let said = |result: &CallToolResult| {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    };

    let started = Instant::now();
    let no_app = server.browser_wait_for(wait("main")).await;
    assert_eq!(no_app.is_error, Some(true));
    assert!(
        said(&no_app).contains("Sawit isn't running"),
        "{}",
        said(&no_app)
    );
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "{:?}",
        started.elapsed()
    );

    let not_a_selector = "“[[[” isn't a valid CSS selector.".to_string();
    fake_app_answering(&paths, vec![Err(not_a_selector.clone())]);
    let started = Instant::now();
    let refused = server.browser_wait_for(wait("[[[")).await;
    assert_eq!(said(&refused), not_a_selector);
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "{:?}",
        started.elapsed()
    );
}

/// A page that is loading has no agent to answer yet: browser_wait_for keeps asking.
#[tokio::test]
async fn waiting_for_a_page_waits_out_its_load() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sawit_core::instances::AppPaths {
        root: dir.path().join("home"),
    };
    let server = Sawit::new(paths.clone());
    let loading = Err(sawit_core::browser::NOT_READY.to_string());
    let found = Ok(json!({"found": true, "url": "http://localhost:5173/"}));
    fake_app_answering(&paths, vec![loading.clone(), loading, found]);
    let done = server
        .browser_wait_for(Parameters(WaitArgs {
            text: Some("Welcome".into()),
            text_gone: None,
            selector: None,
            time_ms: None,
            timeout_ms: Some(5_000),
            project: None,
        }))
        .await;
    assert_ne!(done.is_error, Some(true));
}

/// A screenshot never replaces a canvas file: the tool says so before it asks the app (none
/// runs here), and the board is as it was. Another path gets the picture.
#[tokio::test]
async fn a_screenshot_never_replaces_a_canvas_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = sawit_core::instances::AppPaths {
        root: dir.path().join("home"),
    };
    let server = Sawit::new(paths.clone());
    let board = dir.path().join(".nyawit").join("canvas.sawit");
    std::fs::create_dir_all(board.parent().unwrap()).unwrap();
    std::fs::write(&board, b"the board").unwrap();
    let shot = |output: &std::path::Path| {
        Parameters(ScreenshotArgs {
            reference: None,
            selector: None,
            output_path: Some(output.display().to_string()),
            mesurer: None,
            project: None,
        })
    };
    let said = |result: &CallToolResult| {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    };

    let refused = server.browser_screenshot(shot(&board)).await;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(
        said(&refused),
        format!(
            "browser_screenshot would overwrite a canvas file: {}",
            board.display()
        )
    );
    assert_eq!(std::fs::read(&board).unwrap(), b"the board");

    // The first bytes of every PNG.
    fake_app(
        &paths,
        json!({"png_base64": "iVBORw0KGgo=", "width": 1, "height": 1}),
    );
    let png = dir.path().join("page.png");
    let taken = server.browser_screenshot(shot(&png)).await;
    assert_ne!(taken.is_error, Some(true), "{}", said(&taken));
    assert_eq!(std::fs::read(&png).unwrap(), b"\x89PNG\r\n\x1a\n");
}

/// What an agent is told of the browser is what it does. browser_press_key's ⇧⌘. and Esc go
/// to Sawit, not the page, and ⇧⌘. is the only way an agent opens Annotate; browser_console
/// and browser_network empty their whole log when asked to, so they say what that drops and
/// aren't announced read-only; and app_state's `browser` is the active project's, which may
/// not be the agent's own.
#[test]
fn the_browser_tools_say_what_they_change() {
    let tools = Sawit::router().list_all();
    let sent = |name: &str| {
        let tool = tools.iter().find(|tool| tool.name == name);
        let tool = tool.unwrap_or_else(|| panic!("no tool {name}"));
        serde_json::to_value(tool).unwrap()
    };
    let press = sent("browser_press_key").to_string();
    for said in [
        "Meta+Shift+. opens or closes the Annotate panel",
        "Escape while Sawit's picker is on",
        "Meta+Shift+. is Sawit's",
    ] {
        assert!(press.contains(said), "no {said:?} in {press}");
    }
    for (name, drops) in [
        (
            "browser_console",
            "the console errors the user's Annotate notes would carry",
        ),
        (
            "browser_network",
            "the failed-request count browser_navigate answers",
        ),
    ] {
        let tool = sent(name);
        let text = tool.to_string();
        assert!(text.contains("all of it") && text.contains(drops), "{text}");
        assert_eq!(tool["annotations"]["readOnlyHint"], false, "{name}");
        assert_eq!(tool["annotations"]["destructiveHint"], true, "{name}");
    }
    let state = sent("app_state").to_string();
    assert!(
        state.contains("`browser`: the browser tab of the window's active project")
            && state.contains("may not be the browser your browser_* tools use"),
        "{state}"
    );
}
