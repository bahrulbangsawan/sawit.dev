//! mesurer in Annotate: mounting with no engine, the `state` op, the summary, screenshots and the
//! bridge's dispatch.

use futures::channel::oneshot;
use sawit_core::browser::BrowserOp;
use serde_json::{Value, json};

use super::{engine_event, test_browser};
use crate::browser::engine::{EngineEvent, WorldReply};
use crate::browser::mesurer::shot_hides;
use crate::browser::{BrowserView, Panel};

/// Posts `message` (`{kind, payload}`) from mesurer's world, from the blank page the test tab
/// shows; answers what the app replied.
fn post(
    view: &gpui_kit::Entity<BrowserView>,
    message: Value,
    cx: &mut gpui_kit::VisualTestContext,
) -> Result<String, String> {
    let mut message = message;
    message["url"] = json!("about:blank");
    let (reply, mut answer) = WorldReply::channel();
    let body = message.to_string();
    engine_event(view, EngineEvent::World { body, reply }, cx);
    answer
        .try_recv()
        .ok()
        .flatten()
        .unwrap_or_else(|| Err("no answer".into()))
}

/// What an op answers, once the view has run.
fn answer(
    rx: oneshot::Receiver<Result<Value, String>>,
    cx: &mut gpui_kit::VisualTestContext,
) -> Result<Value, String> {
    cx.run_until_parked();
    let mut rx = rx;
    rx.try_recv()
        .ok()
        .flatten()
        .unwrap_or_else(|| Err("no answer".into()))
}

fn mesurer_op(action: &str) -> BrowserOp {
    BrowserOp::Mesurer {
        action: action.into(),
        args: Default::default(),
    }
}

/// With no web view (a tab never drawn), opening Annotate, Design and closing again mounts
/// nothing and asks nothing of a page; `app_state` says so.
#[gpui_kit::test]
fn annotate_without_an_engine_mounts_nothing(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let mounted =
        |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| view.read(cx).mesurer.mounted);
    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(Panel::Annotate, cx)));
    cx.run_until_parked();
    assert!(!mounted(cx));
    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(Panel::Design, cx)));
    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(Panel::Annotate, cx)));
    cx.update(|_, cx| view.update(cx, |b, cx| b.toggle_panel(Panel::Annotate, cx)));
    cx.run_until_parked();
    assert!(!mounted(cx));
    let described = cx.update(|_, cx| view.read(cx).describe());
    assert_eq!(described["mesurer"], json!({"mounted": false, "tool": ""}));
    // A page's load and its end change nothing while there is no engine.
    engine_event(
        &view,
        EngineEvent::Load {
            started: true,
            url: "http://localhost:3000/".into(),
        },
        cx,
    );
    assert_eq!(cx.update(|_, cx| view.read(cx).mesurer.load_gen), 1);
    assert!(!mounted(cx));
}

/// `state` answers with no page: not mounted, the version, the page's key and origin, where its
/// workspace is kept; another action is lane 2's, which doesn't know it yet.
#[gpui_kit::test]
fn the_state_op_answers_without_a_page(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let rx = cx.update(|_, cx| view.update(cx, |b, cx| b.apply(mesurer_op("state"), cx)));
    let state = answer(rx, cx).unwrap();
    assert_eq!(state["mounted"], false);
    assert_eq!(state["version"], "0.2.7");
    assert_eq!(
        state["commit"],
        sawit_core::browser::mesurer::MESURER_COMMIT
    );
    assert_eq!(state["world"], "sawit-mesurer");
    assert_eq!(state["origin"], "about");
    assert_eq!(state["page_key"], "blank");
    assert_eq!(state["page"], Value::Null);
    assert!(
        state["store_path"]
            .as_str()
            .unwrap()
            .ends_with("cache/browser/mesurer/about.json"),
        "{state}"
    );
    assert!(
        state.get("last_clipboard").is_none(),
        "only under SAWIT_E2E_RUN"
    );
    let rx = cx.update(|_, cx| view.update(cx, |b, cx| b.apply(mesurer_op("dance"), cx)));
    assert!(
        answer(rx, cx)
            .unwrap_err()
            .contains("unknown action “dance”")
    );
}

/// A workspace mesurer saves is held, counted for the page on show, and shown to the Annotate
/// panel; the `state` op reads it back, comments' HTML previews left out.
#[gpui_kit::test]
fn a_saved_workspace_reaches_the_panel_and_the_state_op(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let workspace = json!({
        "enabled": true, "toolMode": "guides", "xrayVisible": true, "rulersVisible": false,
        "pages": {"blank": {
            "guides": [{"id": "g1", "orientation": "vertical", "position": 120}],
            "layoutGuides": [{"id": "l1"}],
            "comments": [{"id": "c1", "status": "open", "target": {"selector": "h1", "htmlPreview": "<h1>Hi</h1>"}}]
        }}
    });
    let replied = post(
        &view,
        json!({"kind": "store.workspace", "payload": workspace}),
        cx,
    );
    assert_eq!(replied, Ok("null".into()));
    let summary = cx.update(|_, cx| view.read(cx).annotate.read(cx).mesurer_summary().cloned());
    let summary = summary.expect("the panel has it");
    assert_eq!(
        (
            summary.tool.as_str(),
            summary.guides,
            summary.layout_guides,
            summary.comments_open,
            summary.xray
        ),
        ("guides", 1, 1, 1, true)
    );
    let rx = cx.update(|_, cx| view.update(cx, |b, cx| b.apply(mesurer_op("state"), cx)));
    let state = answer(rx, cx).unwrap();
    assert_eq!(state["summary"]["guides"], 1);
    assert_eq!(state["chrome"]["toolMode"], "guides");
    assert_eq!(state["page"]["guides"][0]["position"], 120);
    assert_eq!(
        state["page"]["comments"][0]["target"],
        json!({"selector": "h1"})
    );
    let described = cx.update(|_, cx| view.read(cx).describe());
    assert_eq!(described["mesurer"]["tool"], "guides");
    // Cleared in mesurer's settings: nothing left on the page.
    let cleared = post(
        &view,
        json!({"kind": "store.clear", "payload": {"what": "workspace"}}),
        cx,
    );
    assert_eq!(cleared, Ok("null".into()));
    let summary = cx.update(|_, cx| view.read(cx).mesurer.summary.clone());
    assert_eq!(summary.markup(), 0);
}

/// Screenshots hide mesurer only while it is mounted, and show it again after.
#[gpui_kit::test]
fn screenshots_hide_mesurer_only_while_mounted(cx: &mut gpui_kit::TestAppContext) {
    assert_eq!(shot_hides(false), None);
    assert_eq!(shot_hides(true), Some(["all", "none"]));
    let (view, cx) = test_browser(cx);
    let asked = cx.update(|_, cx| view.read(cx).mesurer_hide("all").is_some());
    assert!(!asked, "nothing is asked of a page with no mesurer");
}

/// Each kind the glue posts reaches the file that answers it: the foundation's are answered,
/// the lanes' in their own files; a message that isn't one is refused.
#[gpui_kit::test]
fn each_kind_reaches_its_lane(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let foundation = [
        json!({"kind": "ready", "payload": {"version": "0.2.7", "url": "about:blank", "mount_ms": 31.5}}),
        json!({"kind": "log", "payload": {"level": "info", "text": "hello"}}),
        json!({"kind": "store.settings", "payload": {"theme": "dark"}}),
        json!({"kind": "store.clear", "payload": {"what": "settings"}}),
        json!({"kind": "clipboard.text", "payload": {"text": ".card > h2"}}),
    ];
    for message in foundation {
        assert_eq!(
            post(&view, message.clone(), cx),
            Ok("null".into()),
            "{message}"
        );
    }
    let ready = cx.update(|_, cx| view.read(cx).mesurer.ready.clone());
    assert_eq!(ready.unwrap()["mount_ms"], 31.5);
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(copied.as_deref(), Some(".card > h2"));
    // Every lane's kinds are answered in its own tests: `mesurer_capture.rs`,
    // `mesurer_agents.rs` and `mesurer_keys.rs`.
    let bad = post(&view, json!({"kind": "nope", "payload": {}}), cx);
    assert!(bad.unwrap_err().contains("“nope”"));
}
