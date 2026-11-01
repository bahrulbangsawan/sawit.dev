//! Ops sent to the canvas as the control socket sends them, and the commands the chrome chooses.

use super::*;

#[gpui_kit::test]
fn ops_run_on_the_canvas_document(cx: &mut TestAppContext) {
    let tmp = Scratch::new("ops");
    let view = open(cx, &tmp.0.join("canvas.sawit"));
    let layers = |cx: &mut TestAppContext| {
        let reply = run_op(&view, json!({"op": "get"}), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        reply["elements"].as_array().map(Vec::len)
    };
    assert_eq!(layers(cx), Some(0));
    let reply = run_op(&view, sticky_op(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(!reply["result"]["id"].is_null(), "{reply}");
    // The edit is in the canvas's own document, which `get` reads back.
    assert_eq!(layers(cx), Some(1));
    let in_document = view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        doc.node(doc.first_page().unwrap()).unwrap().children.len()
    });
    assert_eq!(in_document, 1);
    // An op the canvas doesn't have is an error reply, not a panic.
    let reply = run_op(&view, json!({"op": "teleport"}), cx);
    assert_eq!(reply["ok"], json!(false), "{reply}");
}

#[gpui_kit::test]
fn a_canvas_that_could_not_be_opened_answers_ops_with_the_reason(cx: &mut TestAppContext) {
    let tmp = Scratch::new("op-corrupt");
    let path = tmp.0.join("canvas.sawit");
    std::fs::write(&path, b"not a canvas").unwrap();
    let view = open(cx, &path);
    let reason = store::open(&path)
        .err()
        .expect("a corrupt file")
        .to_string();
    for op in [json!({"op": "get"}), sticky_op()] {
        let reply = run_op(&view, op, cx);
        assert_eq!(reply["ok"], json!(false), "{reply}");
        // It says which file, and what is wrong with it.
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("couldn't be opened"), "{error}");
        assert!(error.contains(&*path.to_string_lossy()), "{error}");
        assert!(error.contains(&reason), "{error}");
    }
    // And the file it couldn't open is as it was.
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    view.update(cx, |view, _| view.flush());
    assert_eq!(std::fs::read(&path).unwrap(), b"not a canvas");
    assert_eq!(names(&tmp.0), ["canvas.sawit"]);
}

#[gpui_kit::test]
fn an_ops_requests_reach_the_view(cx: &mut TestAppContext) {
    let tmp = Scratch::new("op-requests");
    let view = open(cx, &tmp.0.join("canvas.sawit"));
    run_op(&view, sticky_op(), cx);
    // ⇧⌘↩'s command, with typing going on in the new sticky: the engine asks the view to
    // start it, and an op's requests reach the view as an input's do.
    let next_sticky =
        json!({"op": "command", "command": "board.nextSticky", "params": {"edit": true}});
    let reply = run_op(&view, next_sticky, cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let next = figcraft_engine::params::id_of(&reply["result"]["id"]).unwrap();
    view.read_with(cx, |view, _| {
        let board = view.board().unwrap();
        let ds = board.session.ds().unwrap();
        let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
        assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
        assert!(board.session.ui_requests.is_empty());
    });
}

#[gpui_kit::test]
fn choose_carries_out_what_its_command_asks(cx: &mut TestAppContext) {
    let tmp = Scratch::new("choose");
    let view = open(cx, &tmp.0.join("canvas.sawit"));
    // A sticky, selected, then ⇧⌘↩'s command with typing to go on in the next one: the
    // engine asks the view to start it, which `choose` carries out at once.
    run_op(&view, sticky_op(), cx);
    view.update(cx, |view, cx| {
        view.choose("board.nextSticky", json!({"edit": true}), cx);
    });
    let [_, next] = layers(&view, cx)[..] else {
        panic!("layers {:?}", layers(&view, cx))
    };
    view.read_with(cx, |view, _| {
        let board = view.board().unwrap();
        let ds = board.session.ds().unwrap();
        let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
        assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
        assert!(board.session.ui_requests.is_empty());
        assert_eq!(view.toast, None);
    });
    // A command the engine refuses says why, in a toast.
    view.update(cx, |view, cx| view.choose("no.such", json!({}), cx));
    assert!(view.read_with(cx, |view, _| view.toast.is_some()));
}
