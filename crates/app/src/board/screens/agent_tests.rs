//! Agents' screen ops while the app runs: `begin {kind: "screen"}` makes the frame and the folder,
//! `screen_write` writes the files and answers that the screen renders, `screen_get` reads it
//! back, and a write that can't be done changes nothing.

use gpui_kit::TestAppContext;
use serde_json::{Value, json};

use crate::board::tests::{Scratch, next_frame, open_in_window};
use crate::workspace::Answer;

fn now(answer: Answer) -> Value {
    match answer {
        Answer::Now(reply) => reply,
        Answer::Later(_) => panic!("it answers at once"),
    }
}

#[gpui_kit::test]
fn agents_write_and_read_screens_in_the_app(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-screens");
    cx.update(gpui_kit::init);
    let nyawit = tmp.0.join(".nyawit");
    std::fs::create_dir_all(&nyawit).unwrap();
    let (_window, view) = open_in_window(cx, &nyawit.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let mut op = |op: Value| now(view.update(cx, |view, cx| view.agent_op(op, None, cx)));

    let begun = op(json!({"op": "begin", "name": "Sign in", "kind": "screen",
                          "frame": {"x": 0, "y": 0, "width": 390, "height": 844}}));
    assert_eq!(begun["screen"], "sign-in", "{begun}");
    let folder = nyawit.join("screens/sign-in");
    assert!(folder.is_dir(), "the folder is made with the frame");
    let work = begun["work"].clone();

    let wrote = op(
        json!({"op": "screen_write", "work": work, "html": "<h1>Hi</h1>",
                          "files": [{"path": "css/a.css", "content": "h1{}"}]}),
    );
    assert_eq!(
        wrote["written"],
        json!(["index.html", "css/a.css"]),
        "{wrote}"
    );
    assert_eq!(wrote["rendered"], true);
    assert_eq!(
        std::fs::read_to_string(folder.join("index.html")).unwrap(),
        "<h1>Hi</h1>"
    );

    let edit = json!({"op": "screen_write", "screen": "Sign in",
                      "edits": [{"find": "Hi", "replace": "Welcome"}, {"find": "nope", "replace": "x"}]});
    let refused = op(edit);
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(
        std::fs::read_to_string(folder.join("index.html")).unwrap(),
        "<h1>Hi</h1>"
    );

    let got = op(json!({"op": "screen_get", "screen": "sign-in"}));
    assert_eq!(got["html"], "<h1>Hi</h1>", "{got}");
    assert_eq!(got["files"], json!(["css/a.css", "index.html"]));
    assert_eq!(got["folder"], json!(folder));
    let unknown = op(json!({"op": "screen_write", "screen": "login", "html": "x"}));
    assert!(
        unknown["error"]
            .as_str()
            .unwrap()
            .contains("no screen is called"),
        "{unknown}"
    );
}
