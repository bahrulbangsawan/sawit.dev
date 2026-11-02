//! Command ops that would open or switch documents, and the file's versions and branches.

use std::sync::Arc;

use figcraft_engine::Session;
use serde_json::json;

use super::*;
use crate::ops::command::Documents;

/// The uids of the session's documents, in order, and the uid of the one on show.
fn documents(board: &BoardDoc) -> (Vec<u64>, u64) {
    let session = &board.session;
    (
        session.docs.iter().map(|doc| doc.uid).collect(),
        session.ds().unwrap().uid,
    )
}

/// The answer to a command that would have left the canvas with another document.
fn would_switch(id: &str) -> Value {
    json!({
        "ok": false,
        "error": format!(
            "{id} would open or switch documents; the canvas keeps its one document"
        ),
    })
}

#[test]
fn a_recovered_file_would_replace_the_canvas_and_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let other = other_board_file(dir.path());
    let mut board = board();
    let reply = send(
        &mut board,
        "board.sticky",
        json!({"x": 0, "y": 0, "text": "mine"}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

    let reply = send(&mut board, "file.recover", json!({"path": other}));

    assert_eq!(reply, would_switch("file.recover"));
    // The canvas holds its one document, on show and as it was: `get` lists its one sticky,
    // not the two of the recovered file.
    assert_eq!(board.session.docs.len(), 1);
    assert_eq!(documents(&board), documents_before);
    assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
    assert_eq!(elements(&mut board).len(), 1);
}

#[test]
fn a_batch_that_opens_a_document_is_undone_whole() {
    let dir = tempfile::tempdir().unwrap();
    let other = other_board_file(dir.path());
    let mut board = board();
    let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

    let reply = send(
        &mut board,
        "command.batch",
        json!({"commands": [
            {"command": "board.sticky", "params": {"x": 0, "y": 0}},
            {"command": "file.recover", "params": {"path": other}},
        ]}),
    );

    assert_eq!(reply, would_switch("command.batch"));
    assert_eq!(board.session.docs.len(), 1);
    assert_eq!(documents(&board), documents_before);
    // The sticky the batch made before it opened the file went with it: the canvas is the
    // very document it was, and its history has no step for the batch.
    assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
    assert!(elements(&mut board).is_empty());
    assert_eq!(board.session.undo_depth(), (0, 0));
}

#[test]
fn a_batch_that_opens_a_document_and_switches_back_is_refused_too() {
    let dir = tempfile::tempdir().unwrap();
    let other = other_board_file(dir.path());
    let mut board = board();
    let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

    // At its end the canvas's document is on show again, but a second one is open.
    let reply = send(
        &mut board,
        "command.batch",
        json!({"commands": [
            {"command": "file.recover", "params": {"path": other}},
            {"command": "doc.switch", "params": {"index": 0}},
        ]}),
    );

    assert_eq!(reply, would_switch("command.batch"));
    assert_eq!(board.session.docs.len(), 1);
    assert_eq!(documents(&board), documents_before);
    assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
}

#[test]
fn other_commands_that_open_a_document_are_refused_too() {
    let mut board = board();
    let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    // A version of the canvas, for `version.duplicate` to open.
    let reply = send(&mut board, "file.saveVersion", json!({"name": "First"}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

    for (id, params) in [
        ("version.duplicate", json!({"index": 0})),
        ("branch.create", json!({"name": "Try it"})),
    ] {
        let reply = send(&mut board, id, params);
        assert_eq!(reply, would_switch(id));
        assert_eq!(documents(&board), documents_before, "{id}");
        assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()), "{id}");
    }
    assert_eq!(elements(&mut board).len(), 1);
}

/// How many versions and branches the canvas's file has: what its next save would write of
/// them.
fn history(board: &BoardDoc) -> (usize, usize) {
    let ds = board.session.ds().unwrap();
    (ds.versions.len(), ds.branches.len())
}

#[test]
fn a_failed_command_op_leaves_no_version_or_branch_to_save() {
    let mut board = board();
    let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    assert_eq!(reply["ok"], json!(true), "{reply}");

    // `branch.create` adds its branch (two copies of the document) to the canvas's file
    // before it opens it in a tab. The guard closes the tab, and the branch goes with it.
    let reply = send(&mut board, "branch.create", json!({"name": "Try it"}));
    assert_eq!(reply, would_switch("branch.create"));
    assert_eq!(history(&board), (0, 0));

    // A batch the guard refuses: what its steps added to the file goes with it too.
    let reply = send(
        &mut board,
        "command.batch",
        json!({"commands": [
            {"command": "file.saveVersion", "params": {"name": "First"}},
            {"command": "branch.create", "params": {"name": "Try it"}},
        ]}),
    );
    assert_eq!(reply, would_switch("command.batch"));
    assert_eq!(history(&board), (0, 0));

    // So does a batch that fails on a step of its own: none of it is done.
    let reply = send(
        &mut board,
        "command.batch",
        json!({"commands": [
            {"command": "file.saveVersion", "params": {"name": "First"}},
            {"command": "no.such.command"},
        ]}),
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert_eq!(history(&board), (0, 0));

    // A command that goes through keeps what it adds.
    let reply = send(&mut board, "file.saveVersion", json!({"name": "Kept"}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(history(&board), (1, 0));
    // And a refusal after it leaves that version where it is.
    let reply = send(&mut board, "branch.create", json!({}));
    assert_eq!(reply, would_switch("branch.create"));
    assert_eq!(history(&board), (1, 0));
    assert_eq!(board.session.ds().unwrap().versions[0].name, "Kept");
}

/// A session whose one document is a board called "Canvas", made by the engine itself.
fn session_with_a_canvas() -> Session {
    let mut session = Session::empty();
    session
        .execute("file.newBoard", &json!({"name": "Canvas"}))
        .unwrap();
    session
}

#[test]
fn documents_are_told_apart_by_uid_not_by_count_and_position() {
    let mut session = session_with_a_canvas();
    let before = Documents::of(&session);
    assert_eq!(Documents::of(&session), before);

    // Close the canvas's document and open another in its place: still one document, and
    // still the first on show.
    session
        .execute("file.newBoard", &json!({"name": "Other"}))
        .unwrap();
    session
        .execute("file.close", &json!({"index": 0, "force": true}))
        .unwrap();
    assert_eq!((session.docs.len(), session.active), (1, 0));
    assert_ne!(Documents::of(&session), before);
}

#[test]
fn restoring_shows_the_canvas_wherever_it_now_is() {
    let mut session = session_with_a_canvas();
    let before = Documents::of(&session);

    // Two more documents, the canvas's moved behind them, the first of them on show.
    for name in ["One", "Two"] {
        session
            .execute("file.newBoard", &json!({"name": name}))
            .unwrap();
    }
    session
        .execute("file.moveTab", &json!({"from": 0, "to": 2}))
        .unwrap();
    session.execute("doc.switch", &json!({"index": 0})).unwrap();
    assert_eq!((session.docs.len(), session.active), (3, 0));
    assert_ne!(Documents::of(&session), before);

    before.restore(&mut session);
    assert_eq!(Documents::of(&session), before);
    assert_eq!((session.docs.len(), session.active), (1, 0));
    assert_eq!(session.doc().unwrap().name, "Canvas");
}

#[test]
fn a_failure_that_opens_nothing_keeps_the_engines_own_words() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.sawit");
    let mut board = board();
    let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

    let reply = send(&mut board, "file.recover", json!({"path": missing}));
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().expect("an error string");
    assert!(error.contains("missing.sawit"), "{error}");
    assert!(!error.contains("would open or switch"), "{error}");

    // A step that fails rolls the batch back, and the step's error is what is answered.
    let reply = send(
        &mut board,
        "command.batch",
        json!({"commands": [
            {"command": "board.sticky", "params": {"x": 0, "y": 0}},
            {"command": "no.such.command"},
        ]}),
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().expect("an error string");
    assert!(error.contains("no.such.command"), "{error}");
    assert!(!error.contains("would open or switch"), "{error}");

    assert_eq!(documents(&board), documents_before);
    assert!(Arc::ptr_eq(&doc_before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), (0, 0));
}
