//! The `command` and `commands` ops: engine commands, undo, batches, and Sawit's own commands.

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::ops::command::OPENS_DOCUMENTS;
use crate::ops::refusal::{HANDLED_BY_SAWIT, refused};
use crate::policy;

/// The answer to a command that Sawit handles itself.
fn handled_by_sawit(id: &str) -> Value {
    json!({"ok": false, "error": format!("{id} is handled by Sawit")})
}

#[test]
fn command_runs_engine_commands() {
    let mut board = board();
    let reply = run(
        &mut board,
        &json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(!reply["result"]["id"].is_null(), "{reply}");
    // It ran on the board.
    assert_eq!(elements(&mut board).len(), 1);
}

#[test]
fn command_params_are_optional() {
    let mut board = board();
    let zoom = |board: &BoardDoc| board.session.ds().unwrap().view.zoom;
    let before = zoom(&board);
    let reply = run(
        &mut board,
        &json!({"op": "command", "command": "view.zoomIn"}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let once = zoom(&board);
    assert!(once > before);
    // `null` is no params either.
    let reply = run(
        &mut board,
        &json!({"op": "command", "command": "view.zoomIn", "params": null}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(zoom(&board) > once);
}

#[test]
fn command_errors_are_reported() {
    let mut board = board();
    let reply = run(
        &mut board,
        &json!({"op": "command", "command": "no.such.command"}),
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().expect("an error string");
    assert!(error.contains("no.such.command"), "{error}");
    assert!(reply.get("result").is_none());

    // The engine refusing a command's params is reported too, with its own words, and leaves
    // the board alone.
    let before = board.doc().unwrap();
    for params in [
        json!({"x": 0, "y": 0, "color": "not a colour"}),
        json!({"x": 0, "y": 0, "no_such_param": 1}),
    ] {
        let reply = run(
            &mut board,
            &json!({"op": "command", "command": "board.sticky", "params": params}),
        );
        assert_eq!(reply["ok"], json!(false), "{params}: {reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("board.sticky"), "{error}");
    }
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert!(elements(&mut board).is_empty());
}

#[test]
fn a_command_op_without_a_command_id_is_reported() {
    let mut board = board();
    for op in [
        json!({"op": "command"}),
        json!({"op": "command", "command": 5}),
        json!({"op": "command", "params": {"x": 0, "y": 0}}),
    ] {
        let reply = run(&mut board, &op);
        assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("command"), "{error}");
    }
    assert!(elements(&mut board).is_empty());
}

#[test]
fn commands_that_switch_or_write_documents_are_refused() {
    let mut board = board();
    board
        .execute("board.sticky", &json!({"x": 0, "y": 0}))
        .unwrap();
    let before = board.doc().unwrap();
    let revision = board.revision();
    for id in [
        "file.new",
        "file.newBoard",
        "file.newFromTemplate",
        "file.open",
        "file.openRecent",
        "file.close",
        "file.duplicate",
        "file.save",
        "file.saveAs",
        "window.new",
        "doc.nextTab",
        "doc.previousTab",
    ] {
        let reply = run(&mut board, &json!({"op": "command", "command": id}));
        assert_eq!(
            reply,
            json!({"ok": false, "error": format!("{id} is handled by Sawit")})
        );
    }
    // None of them reached the engine: the board is the one it was.
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.revision(), revision);
    assert_eq!(elements(&mut board).len(), 1);
}

#[test]
fn refused_commands_are_ones_the_policy_hides_or_hands_to_sawit() {
    for id in HANDLED_BY_SAWIT {
        assert!(
            policy::is_hidden(id) || policy::is_ui(id),
            "{id} is neither hidden nor Sawit's own in the command policy"
        );
    }
}

#[test]
fn commands_that_sawit_handles_are_refused_as_batch_steps_too() {
    let mut board = board();
    let before = board.doc().unwrap();
    for id in HANDLED_BY_SAWIT {
        let reply = send(
            &mut board,
            "command.batch",
            json!({"commands": [
                {"command": "board.sticky", "params": {"x": 0, "y": 0}},
                {"command": id},
            ]}),
        );
        assert_eq!(reply, handled_by_sawit(id));
    }
    // Nothing ran, not even the sticky that came before the refused step.
    assert_eq!(board.session.docs.len(), 1);
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), (0, 0));
    assert!(elements(&mut board).is_empty());
}

#[test]
fn a_save_inside_nested_batches_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("copy.sawit");
    let mut board = board();
    let before = board.doc().unwrap();

    let reply = send(
        &mut board,
        "command.batch",
        json!({"commands": [
            {"command": "board.sticky", "params": {"x": 0, "y": 0}},
            {"command": "command.batch", "params": {"commands": [
                {"command": "file.saveAs", "params": {"path": target}},
            ]}},
        ]}),
    );

    assert_eq!(reply, handled_by_sawit("file.saveAs"));
    assert!(!target.exists(), "the save wrote {}", target.display());
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    // The canvas's document was never given a path, so file.revert has nothing to read.
    assert_eq!(board.session.ds().unwrap().path, None);
}

#[test]
fn command_ops_undo_and_redo_what_an_op_did() {
    let mut board = board();
    let reply = send(&mut board, "board.sticky", json!({"x": 0, "y": 0}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(elements(&mut board).len(), 1);

    let reply = send(&mut board, "edit.undo", json!({}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(elements(&mut board).is_empty());

    // Undoing isn't an edit of its own: the step can be redone.
    let reply = send(&mut board, "edit.redo", json!({}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(elements(&mut board).len(), 1);
}

#[test]
fn a_batch_is_one_undo_step_with_its_own_label() {
    let mut board = board();
    let stickies = |label: Option<&str>| {
        let steps = json!([
            {"command": "board.sticky", "params": {"x": 0, "y": 0}},
            {"command": "board.sticky", "params": {"x": 300, "y": 0}},
        ]);
        match label {
            Some(label) => json!({"label": label, "commands": steps}),
            None => json!({"commands": steps}),
        }
    };

    let reply = send(&mut board, "command.batch", stickies(None));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(reply["result"].as_array().map(Vec::len), Some(2));
    assert_eq!(board.session.undo_depth(), (1, 0));
    assert_eq!(board.session.undo_labels(5), ["Batch"]);

    let reply = send(&mut board, "command.batch", stickies(Some("Two more")));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(board.session.undo_depth(), (2, 0));
    assert_eq!(board.session.undo_labels(5), ["Two more", "Batch"]);
    assert_eq!(elements(&mut board).len(), 4);

    // One undo takes back one batch.
    let reply = send(&mut board, "edit.undo", json!({}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(elements(&mut board).len(), 2);
}

#[test]
fn a_batch_that_opens_nothing_runs_as_the_engine_alone_would_run_it() {
    let sticky = json!({"command": "board.sticky", "params": {"x": 0, "y": 0}});
    let (undo, redo) = (
        json!({"command": "edit.undo"}),
        json!({"command": "edit.redo"}),
    );
    let nested = json!({"command": "command.batch", "params": {
        "label": "Inner",
        "commands": [sticky, sticky],
    }});
    let broken = json!({"command": "no.such.command"});
    let batches = [
        json!({"commands": []}),
        json!({"commands": [sticky, sticky]}),
        json!({"label": "Named", "commands": [sticky]}),
        json!({"commands": [sticky, undo]}),
        json!({"commands": [sticky, undo, redo]}),
        json!({"commands": [nested, sticky]}),
        json!({"commands": [sticky, broken]}),
        json!({"onError": "continue", "commands": [sticky, broken, sticky]}),
    ];

    for batch in batches {
        let (mut through_the_op, mut engine_alone) = (board(), board());
        let reply = send(&mut through_the_op, "command.batch", batch.clone());
        let expected = match engine_alone.execute("command.batch", &batch) {
            Ok(result) => json!({"ok": true, "result": result}),
            Err(error) => json!({"ok": false, "error": error.to_string()}),
        };
        assert_eq!(reply, expected, "{batch}");

        // The same undo steps (and what can be redone), history of commands and layers.
        let state = |board: &mut BoardDoc| {
            (
                board.session.undo_depth(),
                board.session.undo_labels(10),
                board.session.journal.clone(),
                elements(board).len(),
            )
        };
        assert_eq!(
            state(&mut through_the_op),
            state(&mut engine_alone),
            "{batch}"
        );
    }
}

/// SMOKE-3: a board's command run on a design file by the `command` op says what the canvas
/// is, in Sawit's words, too.
#[test]
fn a_boards_command_on_a_design_file_says_what_the_canvas_is() {
    let mut design = design();
    let reply = send(&mut design, "board.sticky", json!({"x": 0, "y": 0}));
    assert_eq!(
        reply["error"],
        json!(
            "command `board.sticky` is not available right now: this is for boards, and this \
                 canvas is a design file"
        ),
        "{reply}"
    );
}

/// Every id `commands` lists, with the whole reply checked.
fn listed_commands(board: &mut BoardDoc) -> Vec<Value> {
    let reply = ask(board, json!({"op": "commands"}));
    reply["commands"].as_array().expect("commands").clone()
}

#[test]
fn commands_hide_hidden_ids() {
    let mut board = board();
    let commands = listed_commands(&mut board);
    let ids: Vec<&str> = commands.iter().map(|c| c["id"].as_str().unwrap()).collect();

    // Nothing the canvas hides: no file.new, nor any other hidden id, nor a plugin's.
    assert!(!ids.contains(&"file.new"));
    assert!(!ids.contains(&"plugins.manage"));
    for id in &ids {
        assert!(!policy::is_hidden(id), "{id} is hidden");
    }
    // Saving is Sawit's: `command` refuses it, so it isn't offered (P7-C11).
    assert!(!ids.contains(&"file.save"));
    assert!(!ids.contains(&"file.saveAs"));

    // What an agent can do is there, as {id, label, params}.
    let sticky = commands
        .iter()
        .find(|c| c["id"] == json!("board.sticky"))
        .unwrap();
    assert_eq!(sticky["label"], json!("Sticky note"));
    assert!(
        sticky["params"].as_str().unwrap().contains("text?"),
        "{sticky}"
    );
    for command in &commands {
        let keys: Vec<&String> = command.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["id", "label", "params"], "{command}");
        assert!(command.as_object().unwrap().values().all(Value::is_string));
    }
    assert!(ids.len() > 200, "{} commands", ids.len());
}

#[test]
fn commands_lists_only_what_the_command_op_runs() {
    let mut board = board();
    let commands = listed_commands(&mut board);
    let ids: Vec<&str> = commands.iter().map(|c| c["id"].as_str().unwrap()).collect();

    // The one rule: nothing is offered that `command` refuses, up front or by the guard that
    // undoes a command that opens a document.
    for id in &ids {
        assert_eq!(
            refused(id, &Value::Null),
            None,
            "{id} is listed but refused"
        );
        assert!(!OPENS_DOCUMENTS.contains(id), "{id} opens a document");
    }
    for id in HANDLED_BY_SAWIT.iter().chain(OPENS_DOCUMENTS) {
        assert!(!ids.contains(id), "{id} is listed");
    }
    // Every other command of the engine is, except what the policy hides.
    let engine = figcraft_engine::cmd::all_commands();
    let unlisted: Vec<&str> = engine
        .iter()
        .map(|c| c.id)
        .filter(|id| !ids.contains(id))
        .collect();
    for id in &unlisted {
        assert!(
            policy::is_hidden(id) || HANDLED_BY_SAWIT.contains(id) || OPENS_DOCUMENTS.contains(id),
            "{id} is not listed, and nothing says why"
        );
    }
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "an id is listed twice");
}

#[test]
fn commands_that_open_documents_are_real_engine_ids() {
    // A rename in the engine would leave a stale id here, and the command in the list.
    for id in OPENS_DOCUMENTS {
        assert!(figcraft_engine::cmd::find_command(id).is_some(), "{id}");
    }
}
