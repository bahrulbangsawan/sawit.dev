//! `browser_mesurer` and `browser_screenshot`'s `mesurer`: the ops they send the app, what they
//! refuse before asking it, and what their schemas tell an agent.

use std::sync::{Arc, Mutex};

use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};

use super::*;
use crate::Sawit;

fn args(value: Value) -> MesurerArgs {
    serde_json::from_value(value).unwrap()
}

fn op(value: Value) -> Result<Value, String> {
    mesurer_op(&args(value)).map(|op| serde_json::to_value(op).unwrap())
}

/// Each action becomes the app's `mesurer` op with what it takes, `state` by default.
#[test]
fn each_action_is_the_apps_op() {
    assert_eq!(
        op(json!({})),
        Ok(json!({"op": "mesurer", "action": "state"}))
    );
    assert_eq!(
        op(json!({"action": "STATE", "full": true})),
        Ok(json!({"op": "mesurer", "action": "state", "html": true}))
    );
    assert_eq!(
        op(json!({"action": "open"})),
        Ok(json!({"op": "mesurer", "action": "open"}))
    );
    assert_eq!(
        op(json!({"action": "set", "xray": true, "tool": "guides",
            "guides": [{"position": 120}],
            "layout_guides": [{"kind": "columns", "count": 12, "size": 72, "gutter": 20}]})),
        Ok(
            json!({"op": "mesurer", "action": "set", "xray": true, "tool": "guides",
            "guides": [{"position": 120.0}],
            "layout_guides": [{"kind": "columns", "count": 12, "size": 72.0, "gutter": 20.0}]})
        )
    );
    assert_eq!(
        op(json!({"action": "clear", "what": " all "})),
        Ok(json!({"op": "mesurer", "action": "clear", "what": "all"}))
    );
    // The op reads back as the app takes it.
    let sent = op(json!({"action": "set", "minimized": true})).unwrap();
    let back: BrowserOp = serde_json::from_value(sent).unwrap();
    let BrowserOp::Mesurer { action, args } = back else {
        panic!("not mesurer's op");
    };
    assert_eq!(
        (action.as_str(), args["minimized"].clone()),
        ("set", json!(true))
    );
}

/// What doesn't go with the action is refused before the app is asked.
#[test]
fn arguments_of_another_action_are_refused() {
    for (given, says) in [
        (json!({"action": "dance"}), "Unknown action “dance”"),
        (json!({"action": "set"}), "set needs one of"),
        (json!({"action": "clear"}), "clear needs what"),
        (
            json!({"xray": true}),
            "xray doesn't go with action state: it is set's.",
        ),
        (
            json!({"action": "set", "what": "all"}),
            "what doesn't go with action set: it is clear's.",
        ),
        (
            json!({"action": "open", "full": true}),
            "full doesn't go with action open: it is state's.",
        ),
        (
            json!({"action": "clear", "what": "all", "tool": "pen"}),
            "tool doesn't go with action clear",
        ),
    ] {
        let e = op(given.clone()).unwrap_err();
        assert!(e.contains(says), "{given}: {e}");
    }
}

#[test]
fn a_screenshot_takes_how_much_of_mesurer_to_show() {
    assert_eq!(screenshot_mesurer(None), Ok(None));
    assert_eq!(screenshot_mesurer(Some(" ")), Ok(None));
    assert_eq!(
        screenshot_mesurer(Some("Drawings")),
        Ok(Some("drawings".into()))
    );
    assert_eq!(screenshot_mesurer(Some("show")), Ok(Some("show".into())));
    assert!(
        screenshot_mesurer(Some("all"))
            .unwrap_err()
            .contains("hide, drawings or show")
    );
    // The app reads it from the op; an op without it hides mesurer.
    let op: BrowserOp =
        serde_json::from_value(json!({"op": "screenshot", "mesurer": "drawings"})).unwrap();
    assert_eq!(
        op,
        BrowserOp::Screenshot {
            target: Default::default(),
            mesurer: Some("drawings".into())
        }
    );
    let op: BrowserOp = serde_json::from_value(json!({"op": "screenshot"})).unwrap();
    assert_eq!(
        serde_json::to_value(op).unwrap(),
        json!({"op": "screenshot"})
    );
}

/// The schemas name every action, argument and choice an agent has, and that comments are the
/// user's to write; the tool can destroy (clear), and the screenshot's choice is offered.
#[test]
fn the_schemas_tell_an_agent_what_it_can_do() {
    let tools = Sawit::router().list_all();
    let sent = |name: &str| {
        let tool = tools.iter().find(|tool| tool.name == name);
        serde_json::to_value(tool.unwrap_or_else(|| panic!("no tool {name}"))).unwrap()
    };
    let tool = sent("browser_mesurer");
    let text = tool.to_string();
    for said in [
        "never writes a comment",
        "layout_guides",
        "drawings, measurements, guides, layout_guides, comments or all",
        "none, select (inspect), selection, guides, arrows, pen, text or comments",
        "columns (default), rows or grid",
        "stretch (default), min, center or max",
    ] {
        assert!(text.contains(said), "no {said:?} in {text}");
    }
    let props = &tool["inputSchema"]["properties"];
    for name in [
        "action",
        "full",
        "xray",
        "rulers",
        "tool",
        "minimized",
        "guides",
        "layout_guides",
        "what",
        "project",
    ] {
        assert!(props.get(name).is_some(), "no {name} in {props}");
    }
    assert_eq!(tool["annotations"]["destructiveHint"], true);
    let shot = sent("browser_screenshot");
    assert!(
        shot["inputSchema"]["properties"]["mesurer"]["description"]
            .as_str()
            .unwrap()
            .contains("drawings keeps their drawings")
    );
    let annotations = sent("browser_annotations").to_string();
    assert!(
        annotations.contains("deletes the comment threads it returned"),
        "{annotations}"
    );
}

/// The tool sends its op to the running app and answers what the app said.
#[tokio::test]
async fn the_tool_asks_the_app() {
    use std::io::{Read as _, Write as _};
    let dir = tempfile::tempdir().unwrap();
    let paths = sawit_core::instances::AppPaths {
        root: dir.path().join("home"),
    };
    std::fs::create_dir_all(&paths.root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
    let heard = Arc::new(Mutex::new(Vec::<String>::new()));
    let kept = heard.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut bytes = Vec::new();
            if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                continue;
            }
            kept.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&bytes).into_owned());
            let answer = json!({"set": ["1 layout guides"], "applied": true});
            let line = sawit_core::app_control::reply_ok(&answer);
            let _ = stream.write_all(format!("{line}\n").as_bytes());
        }
    });
    let server = Sawit::new(paths);
    let result = server
        .browser_mesurer(Parameters(args(json!({"action": "set",
            "layout_guides": [{"kind": "columns", "count": 12, "gutter": 20}]}))))
        .await;
    let text: Vec<String> = (result.content.iter())
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect();
    assert_ne!(result.is_error, Some(true), "{text:?}");
    assert!(text.join("").contains("\"applied\": true"), "{text:?}");
    let heard = heard.lock().unwrap().join("\n");
    assert!(
        heard.contains("\"op\":\"mesurer\"") && heard.contains("\"action\":\"set\""),
        "{heard}"
    );
    assert!(
        heard.contains("\"layout_guides\":[{\"count\":12,\"gutter\":20.0,\"kind\":\"columns\"}]")
            || heard.contains(
                "\"layout_guides\":[{\"kind\":\"columns\",\"count\":12,\"gutter\":20.0}]"
            ),
        "{heard}"
    );

    let refused = server
        .browser_mesurer(Parameters(args(json!({"action": "clear"}))))
        .await;
    assert_eq!(refused.is_error, Some(true));
}
