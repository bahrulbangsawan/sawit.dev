//! Lane 2 (mesurer-agents): mesurer's comment threads as Annotate rows, the markup for agents,
//! and the `mesurer` op's actions, with workspaces fed through the store's handler as mesurer
//! posts them (from the blank page the test tab shows: origin `about`, page `blank`).

use futures::channel::oneshot;
use gpui_kit::VisualTestContext;
use sawit_core::browser::BrowserOp;
use serde_json::{Map, Value, json};

use super::{engine_event, test_browser};
use crate::browser::BrowserView;
use crate::browser::engine::{EngineEvent, WorldReply};
use crate::browser::shots::MesurerShot;

type View = gpui_kit::Entity<BrowserView>;

/// Posts `message` (`{kind, payload}`) from mesurer's world; answers what the app replied.
fn post(view: &View, message: Value, cx: &mut VisualTestContext) -> Result<String, String> {
    let mut message = message;
    message["url"] = json!("about:blank");
    let (reply, mut answer) = WorldReply::channel();
    let body = message.to_string();
    engine_event(view, EngineEvent::World { body, reply }, cx);
    (answer.try_recv().ok().flatten()).unwrap_or_else(|| Err("no answer".into()))
}

/// mesurer saves a workspace whose page has `threads` and the rest of `page`.
fn save(view: &View, threads: Value, page: Value, cx: &mut VisualTestContext) {
    let mut artifacts = json!({"guides": [], "selectedGuideIds": [], "measurements": [],
        "heldDistances": [], "comments": threads});
    for (k, v) in page.as_object().unwrap() {
        artifacts[k] = v.clone();
    }
    let workspace = json!({"enabled": true, "toolMode": "none", "rulersVisible": false,
        "guideOrientation": "vertical", "xrayVisible": false, "pages": {"blank": artifacts}});
    let saved = post(
        view,
        json!({"kind": "store.workspace", "payload": workspace}),
        cx,
    );
    assert_eq!(saved, Ok("null".into()));
    cx.run_until_parked();
}

/// A thread as mesurer keeps it.
fn thread(id: &str, status: &str, messages: &[&str]) -> Value {
    let messages: Vec<Value> = (messages.iter().enumerate())
        .map(|(i, m)| json!({"id": format!("{id}-{i}"), "role": "user", "text": m, "createdAt": i}))
        .collect();
    json!({"id": id, "status": status, "createdAt": 1, "updatedAt": 2, "messages": messages,
        "target": {"selector": format!("#{id}"), "tagName": "h2", "textSnippet": "Fresh",
            "htmlPreview": "<h2>Fresh</h2>", "attributes": {"id": id}, "styles": "display: block",
            "rect": {"left": 10, "top": 20, "width": 200, "height": 40},
            "documentPoint": {"x": 12, "y": 22}}})
}

/// The rows: `(n, first message, messages, resolved)` for comment threads, by row.
fn rows(view: &View, cx: &mut VisualTestContext) -> Vec<(u32, String, usize, bool)> {
    cx.update(|_, cx| {
        (view.read(cx).annotate.read(cx).annotations(cx).into_iter())
            .map(|a| {
                let messages = a.thread().map_or(0, |t| t.messages.len());
                (a.n, a.comment.clone(), messages, a.resolved())
            })
            .collect()
    })
}

/// The ids of the threads on the page as last handed to mesurer.
fn applied_ids(view: &View, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let apply = view
            .read(cx)
            .mesurer
            .agents
            .last_apply
            .clone()
            .expect("an apply");
        (apply["snapshot"]["workspace"]["pages"]["blank"]["comments"].as_array())
            .into_iter()
            .flatten()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    })
}

fn op(view: &View, action: &str, args: Value, cx: &mut VisualTestContext) -> Result<Value, String> {
    let args: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    let action = action.to_string();
    let rx: oneshot::Receiver<Result<Value, String>> = cx
        .update(|_, cx| view.update(cx, |b, cx| b.apply(BrowserOp::Mesurer { action, args }, cx)));
    cx.run_until_parked();
    let mut rx = rx;
    (rx.try_recv().ok().flatten()).unwrap_or_else(|| Err("no answer".into()))
}

/// A thread mesurer saves becomes a row of kind Comment, without a marker of Sawit's; replies,
/// resolving and reopening reach it; one deleted in mesurer drops it.
#[gpui_kit::test]
fn comment_rows_follow_the_threads(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    save(
        &view,
        json!([thread("c1", "open", &["Tighten this"])]),
        json!({}),
        cx,
    );
    assert_eq!(rows(&view, cx), [(1, "Tighten this".into(), 1, false)]);
    let markers = cx.update(|_, cx| {
        view.read(cx)
            .annotate
            .read(cx)
            .markers_for("about:blank", cx)
    });
    assert!(markers.is_empty(), "mesurer's pin is its marker");

    save(
        &view,
        json!([thread("c1", "open", &["Tighten this", "And bolder"])]),
        json!({}),
        cx,
    );
    assert_eq!(rows(&view, cx), [(1, "Tighten this".into(), 2, false)]);
    let md = cx
        .update(|_, cx| view.read(cx).annotations_markdown(cx))
        .unwrap();
    assert!(
        md.contains(
            "## 1. `h2#c1` — Comment\n- **Comment:** Tighten this\n  - **Reply:** And bolder\n"
        ),
        "{md}"
    );

    save(
        &view,
        json!([thread("c1", "resolved", &["Tighten this"])]),
        json!({}),
        cx,
    );
    assert_eq!(rows(&view, cx), [(1, "Tighten this".into(), 1, true)]);
    let md = cx.update(|_, cx| view.read(cx).annotations_markdown(cx));
    assert_eq!(md, None, "a resolved thread doesn't go");

    save(
        &view,
        json!([
            thread("c1", "open", &["Tighten this"]),
            thread("c2", "open", &["Too pale"])
        ]),
        json!({}),
        cx,
    );
    assert_eq!(
        rows(&view, cx),
        [
            (1, "Tighten this".into(), 1, false),
            (2, "Too pale".into(), 1, false)
        ]
    );
    save(
        &view,
        json!([thread("c2", "open", &["Too pale"])]),
        json!({}),
        cx,
    );
    assert_eq!(rows(&view, cx), [(2, "Too pale".into(), 1, false)]);
    // Cleared in mesurer's settings: no thread is left.
    let cleared = post(
        &view,
        json!({"kind": "store.clear", "payload": {"what": "workspace"}}),
        cx,
    );
    assert_eq!(cleared, Ok("null".into()));
    cx.run_until_parked();
    assert!(rows(&view, cx).is_empty());
}

/// A thread whose pin was moved to another element: its row, and what agents get, describe the
/// new element.
#[gpui_kit::test]
fn a_moved_comment_row_takes_its_new_element(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    save(
        &view,
        json!([thread("c1", "open", &["Too pale"])]),
        json!({}),
        cx,
    );
    let mut moved = thread("c1", "open", &["Too pale"]);
    moved["target"]["selector"] = json!("div#swatch");
    moved["target"]["tagName"] = json!("div");
    moved["target"]["attributes"] = json!({"id": "swatch"});
    save(&view, json!([moved]), json!({}), cx);
    let selector = cx.update(|_, cx| {
        let rows = view.read(cx).annotate.read(cx).annotations(cx);
        rows[0].element.selector.clone()
    });
    assert_eq!(selector, "div#swatch");
    let md = cx
        .update(|_, cx| view.read(cx).annotations_markdown(cx))
        .unwrap();
    assert!(md.contains("## 1. `div#swatch` — Comment"), "{md}");
}

/// Removing a comment's row hands mesurer the workspace without its thread; Undo hands it back
/// with the thread where it was. Comment rows never count as notes lost with the tab.
#[gpui_kit::test]
fn a_removed_comment_row_deletes_its_thread_until_undo(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let threads = json!([
        thread("c1", "open", &["One"]),
        thread("c2", "open", &["Two"])
    ]);
    save(&view, threads, json!({}), cx);
    assert_eq!(rows(&view, cx).len(), 2);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).unsent_notes(cx)),
        0,
        "kept in mesurer's files"
    );

    let panel = cx.update(|_, cx| view.read(cx).annotate.clone());
    cx.update(|_, cx| panel.update(cx, |p, cx| p.remove(0, cx)));
    cx.run_until_parked();
    assert_eq!(applied_ids(&view, cx), ["c2"]);
    assert_eq!(rows(&view, cx).len(), 1);
    // mesurer saves before it took the change: the removed row isn't made again.
    save(
        &view,
        json!([
            thread("c1", "open", &["One"]),
            thread("c2", "open", &["Two"])
        ]),
        json!({}),
        cx,
    );
    assert_eq!(rows(&view, cx).len(), 1);

    cx.update(|_, cx| panel.update(cx, |p, cx| p.undo(cx)));
    cx.run_until_parked();
    assert_eq!(applied_ids(&view, cx), ["c1", "c2"]);
    assert_eq!(rows(&view, cx).len(), 2);
    let state = op(&view, "state", json!({}), cx).unwrap();
    assert_eq!(state["page"]["comments"].as_array().unwrap().len(), 2);
}

/// "Copy for agent" notes how many threads went and counts their rows as copied; "Resolve all"
/// says how many it resolved.
#[gpui_kit::test]
fn copy_for_agent_and_resolve_all_are_noted(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    save(
        &view,
        json!([
            thread("c1", "open", &["One"]),
            thread("c2", "resolved", &["Two"])
        ]),
        json!({}),
        cx,
    );
    let copied = post(
        &view,
        json!({"kind": "comments.copied", "payload": {"text": "URL: about:blank\n\n1. One"}}),
        cx,
    );
    assert_eq!(copied, Ok("null".into()));
    let (note, c1, c2) = cx.update(|_, cx| {
        let panel = view.read(cx).annotate.read(cx);
        (
            panel.note(),
            panel.thread_copied("c1"),
            panel.thread_copied("c2"),
        )
    });
    assert_eq!(note.as_deref(), Some("Copied 1 comment for an agent"));
    assert!(c1 && !c2, "the open one went");
    let resolved = post(
        &view,
        json!({"kind": "comments.resolved", "payload": {}}),
        cx,
    );
    assert_eq!(resolved, Ok("null".into()));
    let note = cx.update(|_, cx| view.read(cx).annotate.read(cx).note());
    assert_eq!(note.as_deref(), Some("Resolved 1 comment"));
}

/// `set` and `clear` change the page's workspace once a mount has read it, and hand it to
/// mesurer; `open` and `close` are Annotate's; other actions and arguments are refused.
#[gpui_kit::test]
fn the_ops_actions_change_the_workspace(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let grid =
        json!({"layout_guides": [{"kind": "columns", "count": 12, "size": 72, "gutter": 20}]});
    let e = op(&view, "set", grid.clone(), cx).unwrap_err();
    assert!(e.contains("open it first"), "{e}");

    save(&view, json!([]), json!({}), cx);
    let set = op(&view, "set", grid, cx).unwrap();
    assert_eq!(set["set"], json!(["1 layout guides"]));
    assert_eq!(set["summary"]["layout_guides"], 1);
    assert_eq!(set["applied"], false, "mesurer isn't on the page");
    let apply = cx
        .update(|_, cx| view.read(cx).mesurer.agents.last_apply.clone())
        .unwrap();
    assert_eq!(apply["source"], json!({"workspace": true}));
    let page = &apply["snapshot"]["workspace"]["pages"]["blank"];
    assert_eq!(page["layoutGuides"][0]["count"], 12.0);
    let state = op(&view, "state", json!({}), cx).unwrap();
    assert_eq!(state["page"]["layoutGuides"][0]["gutter"], 20.0);
    assert_eq!(state["summary"]["layout_guides"], 1);

    let set = op(&view, "set", json!({"xray": true, "tool": "guides"}), cx).unwrap();
    assert_eq!(set["chrome"]["xrayVisible"], true);
    assert_eq!(set["chrome"]["toolMode"], "guides");
    let cleared = op(&view, "clear", json!({"what": "layout_guides"}), cx).unwrap();
    assert_eq!(cleared["cleared"]["layout_guides"], 1);
    assert_eq!(cleared["summary"]["layout_guides"], 0);
    for (action, args, says) in [
        ("clear", json!({}), "clear needs what"),
        (
            "clear",
            json!({"what": "all", "x": 1}),
            "takes what, not “x”",
        ),
        ("set", json!({"comments": []}), "doesn't take “comments”"),
        ("dance", json!({}), "unknown action “dance”"),
    ] {
        let e = op(&view, action, args, cx).unwrap_err();
        assert!(e.contains(says), "{action}: {e}");
    }

    let opened = op(&view, "open", json!({}), cx).unwrap();
    assert_eq!(opened["mounted"], false, "no web view in a test");
    let panel = cx.update(|_, cx| view.read(cx).describe()["panel"].clone());
    assert_eq!(panel, "annotate");
    let closed = op(&view, "close", json!({}), cx).unwrap();
    assert_eq!(closed, json!({"mounted": false, "panel": null}));
}

/// `browser_annotations` with `clear` deletes the threads it returned, and keeps resolved ones,
/// which it didn't return, with their rows.
#[gpui_kit::test]
fn annotations_clear_deletes_the_threads_it_read(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    save(
        &view,
        json!([
            thread("c1", "open", &["Read me"]),
            thread("c2", "resolved", &["Done"])
        ]),
        json!({}),
        cx,
    );
    let answer = cx.update(|_, cx| view.update(cx, |b, cx| b.annotations_op(true, cx)));
    cx.run_until_parked();
    assert_eq!(answer["count"], 1);
    assert_eq!(answer["comments_deleted"], 1);
    assert!(answer["markdown"].as_str().unwrap().contains("Read me"));
    assert!(!answer["markdown"].as_str().unwrap().contains("Done"));
    assert_eq!(applied_ids(&view, cx), ["c2"]);
    assert_eq!(rows(&view, cx), [(2, "Done".into(), 1, true)]);
}

/// The page's markup goes with the notes, without a note on the page, in its own section.
#[gpui_kit::test]
fn the_page_markup_goes_with_the_notes(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).annotations_markdown(cx)),
        None
    );
    let page = json!({
        "guides": [{"id": "g1", "orientation": "vertical", "position": 120}],
        "arrows": [{"id": "a1", "start": {"x": 1, "y": 2}, "end": {"x": 30, "y": 40}, "color": "#f00", "width": 2}],
        "textAnnotations": [{"id": "t1", "x": 5, "y": 6, "text": "Bigger"}],
    });
    save(&view, json!([]), page, cx);
    let md = cx
        .update(|_, cx| view.read(cx).annotations_markdown(cx))
        .unwrap();
    assert!(md.contains("## Page markup (Mesurer)\n"), "{md}");
    assert!(md.contains("  - `Bigger` at (5, 6)\n"), "{md}");
    assert!(md.contains("- **Guides (1):** vertical at x 120\n"), "{md}");
}

/// A mount reads the files again (another view's saves are in them) unless the browser's copy
/// has saves the files don't have yet, which then win over what was read.
#[gpui_kit::test]
fn a_mount_reads_the_files_unless_saves_are_waiting(cx: &mut gpui_kit::TestAppContext) {
    let (view, cx) = test_browser(cx);
    let store = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let store = &view.read(cx).mesurer.store;
            (store.snapshot("about"), store.workspace("about").cloned())
        })
    };
    let read = |tool: &str| json!({"settings": {"theme": "dark"}, "workspace": {"pages": {}, "toolMode": tool}});
    cx.update(|_, cx| view.update(cx, |b, _| b.mesurer.store.loaded("about", &read("pen"))));
    let (snapshot, held) = store(cx);
    assert_eq!(snapshot, None, "nothing waits: the files are read");
    assert_eq!(held.unwrap()["toolMode"], "pen");
    // Another view wrote the file meanwhile: the next mount takes what it wrote.
    cx.update(|_, cx| view.update(cx, |b, _| b.mesurer.store.loaded("about", &read("text"))));
    assert_eq!(store(cx).1.unwrap()["toolMode"], "text");
    assert_eq!(
        cx.update(|_, cx| view.read(cx).mesurer.store.settings()),
        json!({"theme": "dark"})
    );

    save(&view, json!([]), json!({}), cx);
    let (snapshot, _) = store(cx);
    let snapshot = snapshot.expect("a save waits: the copy is newer");
    assert_eq!(snapshot["workspace"]["toolMode"], "none");
    assert_eq!(snapshot["settings"], json!({"theme": "dark"}));
    cx.update(|_, cx| view.update(cx, |b, _| b.mesurer.store.loaded("about", &read("pen"))));
    assert_eq!(
        store(cx).1.unwrap()["toolMode"],
        "none",
        "the waiting save wins"
    );
}

#[test]
fn screenshots_take_how_much_of_mesurer_to_show() {
    assert_eq!(MesurerShot::named(None), Ok(MesurerShot::Hide));
    assert_eq!(
        MesurerShot::named(Some(" Drawings ")),
        Ok(MesurerShot::Drawings)
    );
    assert_eq!(MesurerShot::named(Some("show")), Ok(MesurerShot::Show));
    assert!(
        MesurerShot::named(Some("half"))
            .unwrap_err()
            .contains("hide, drawings or show")
    );
    assert_eq!(MesurerShot::Hide.hides(true), Some(["all", "none"]));
    assert_eq!(MesurerShot::Drawings.hides(true), Some(["chrome", "none"]));
    assert_eq!(MesurerShot::Show.hides(true), None);
    assert_eq!(MesurerShot::Drawings.hides(false), None, "nothing to hide");
}
