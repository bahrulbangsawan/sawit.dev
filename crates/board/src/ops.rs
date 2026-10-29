//! The canvas's control ops: the `{"op": ...}` messages of the agent wire contract (spec, "Agent
//! wire contract"), run on a [`BoardDoc`] and answered as JSON.
//!
//! Every answer is `{ok: true, ...}` or `{ok: false, error}`, and an op never panics: a failure of
//! the engine, or an op that is malformed, is an `error` the agent can read. For now there are two
//! ops, `get` and `command`; the others of the contract get their own answer, "<op> is not
//! supported yet".
//!
//! A canvas holds one document, and the view saves the one on show over the canvas's file, so no
//! `command` op may leave another document open or on show. The ones that do that by name are
//! refused up front ([`HANDLED_BY_SAWIT`], as the steps of a `command.batch` too); any other
//! command that does it anyway (`file.recover`, `version.duplicate`, `branch.create`...) is undone
//! and answers an error.

use std::fmt::Display;

use figcraft_engine::{EngineError, Session};
use serde_json::{Value, json};
use thiserror::Error;

use crate::doc::{BoardDoc, BoardError};

/// The commands the `command` op refuses: they switch, open, close or write documents behind the
/// canvas's store. Sawit does these itself.
///
/// This is the cheap first check, and the ids can't be a complete list: [`Documents`] catches what
/// else a command does to the session's documents.
const HANDLED_BY_SAWIT: &[&str] = &[
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
];

/// The command that runs other commands as its steps.
const BATCH: &str = "command.batch";

/// Runs `op` (an object whose `op` is the op's name) on `board` and answers it: `{ok: true, ...}`,
/// or `{ok: false, error}`.
///
/// - `get` answers `{ok, elements}`: the top-level layers of the page on show, back to front, as
///   `{id, type: "other", name, x, y, width, height}`. The `id` is the layer's FigCraft node id as a
///   string, and the box is its bounds in page coordinates.
/// - `command` (`{command, params?}`) runs engine command `command` and answers `{ok, result}`,
///   or the engine's own `error`. It refuses the commands Sawit handles itself, as the steps of a
///   `command.batch` too ("<id> is handled by Sawit"). A command that would leave the canvas with
///   another document open or on show is undone and answers "<id> would open or switch documents;
///   the canvas keeps its one document".
pub fn run(board: &mut BoardDoc, op: &Value) -> Value {
    match op.get("op").and_then(Value::as_str) {
        Some("get") => get(board),
        Some("command") => command(board, op),
        Some(other) => failure(format!("{other} is not supported yet")),
        None => failure(r#"an op needs a name, like {"op": "get"}"#),
    }
}

/// `{ok: false, error}`.
fn failure(error: impl Display) -> Value {
    json!({"ok": false, "error": error.to_string()})
}

fn get(board: &BoardDoc) -> Value {
    match layers(board) {
        Ok(elements) => json!({"ok": true, "elements": elements}),
        Err(error) => failure(error),
    }
}

/// The top-level layers of the page on show, back to front.
fn layers(board: &BoardDoc) -> Result<Vec<Value>, BoardError> {
    let ds = board.session.ds()?;
    let doc = &ds.doc;
    let Some(page) = doc.node(ds.page) else {
        return Ok(Vec::new());
    };
    Ok(page
        .children
        .iter()
        .filter_map(|&id| {
            let (node, bounds) = (doc.node(id)?, doc.world_bounds(id)?);
            Some(json!({
                "id": id.to_string(),
                "type": "other",
                "name": node.name,
                "x": bounds.x0,
                "y": bounds.y0,
                "width": bounds.width(),
                "height": bounds.height(),
            }))
        })
        .collect())
}

fn command(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(id) = op.get("command").and_then(Value::as_str) else {
        return failure(
            r#"a command op needs a command id, like {"op": "command", "command": "view.zoomIn"}"#,
        );
    };
    let params = match op.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(params) => params.clone(),
    };
    if let Some(refused) = refused(id, &params) {
        return failure(format!("{refused} is handled by Sawit"));
    }
    let ran = if id == BATCH {
        batch(board, &params)
    } else {
        single(board, id, &params)
    };
    match ran {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => failure(error),
    }
}

/// Why a command op has no result.
#[derive(Debug, Error)]
enum Failed {
    /// The engine refused the command, or it failed.
    #[error(transparent)]
    Engine(#[from] BoardError),
    /// The command would have left the canvas with another document open or on show; the canvas is
    /// as it was.
    #[error("{0} would open or switch documents; the canvas keeps its one document")]
    Switched(String),
}

/// The command in [`HANDLED_BY_SAWIT`] that running `id` with `params` would reach, if any: `id`
/// itself, or a step of a `command.batch`, however deeply batches are nested.
fn refused(id: &str, params: &Value) -> Option<&'static str> {
    if let Some(handled) = HANDLED_BY_SAWIT.iter().find(|handled| **handled == id) {
        return Some(*handled);
    }
    if id != BATCH {
        return None;
    }
    let steps = params.get("commands")?.as_array()?;
    steps.iter().find_map(|step| {
        let id = step.get("command")?.as_str()?;
        refused(id, step.get("params").unwrap_or(&Value::Null))
    })
}

/// The documents a session holds, as a command op found them: their uids in order, and the uid of
/// the one on show.
///
/// A uid names a document for good, because the engine gives every document it makes a new one. So
/// closing the canvas's document and opening another in its place, which leaves the count and the
/// index on show as they were, still reads as the change it is.
#[derive(Debug, PartialEq)]
struct Documents {
    uids: Vec<u64>,
    shown: Option<u64>,
}

impl Documents {
    fn of(session: &Session) -> Self {
        Self {
            uids: session.docs.iter().map(|doc| doc.uid).collect(),
            shown: session.ds().ok().map(|doc| doc.uid),
        }
    }

    /// Closes the documents opened since, and shows the one that was on show again.
    fn restore(&self, session: &mut Session) {
        session.docs.retain(|doc| self.uids.contains(&doc.uid));
        session.active = self
            .shown
            .and_then(|uid| session.docs.iter().position(|doc| doc.uid == uid))
            .unwrap_or(0);
    }
}

/// Runs command `id`, other than a batch. If it left other documents open, or another on show,
/// those are closed again and the canvas's document is shown: nothing but `file.close` removes the
/// canvas's own document outside a batch, and Sawit refuses that one.
///
/// Not run in a group of its own: `edit.undo` and `edit.redo` aren't undo steps, and a group would
/// record them as new ones and drop what can be redone.
fn single(board: &mut BoardDoc, id: &str, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let result = board.execute(id, params);
    if Documents::of(&board.session) != before {
        before.restore(&mut board.session);
        return Err(Failed::Switched(id.into()));
    }
    Ok(result?)
}

/// Runs a `command.batch` in a group that fails when its steps left other documents open, or
/// another on show: the engine then puts back the documents, the one on show and the canvas's own
/// document, so nothing of the batch stays.
///
/// The group takes the label the batch's own group does (`label`, else "Batch"), so a batch that
/// stays is the one undo step it always was.
fn batch(board: &mut BoardDoc, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let label = figcraft_engine::params::string(params, "label").unwrap_or_else(|| "Batch".into());
    let mut switched = false;
    let result = board.session.grouped(&label, |session| {
        let result = session.execute(BATCH, params)?;
        if Documents::of(session) != before {
            switched = true;
            return Err(EngineError::Disabled(
                BATCH.into(),
                "it would open or switch documents".into(),
            ));
        }
        Ok(result)
    });
    if switched {
        return Err(Failed::Switched(BATCH.into()));
    }
    Ok(result.map_err(BoardError::from)?)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::policy;

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// The elements `get` lists, after checking that it answered.
    fn elements(board: &mut BoardDoc) -> Vec<Value> {
        let reply = run(board, &json!({"op": "get"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        reply["elements"].as_array().unwrap().clone()
    }

    /// Sends command op `id` with `params`.
    fn send(board: &mut BoardDoc, id: &str, params: Value) -> Value {
        run(
            board,
            &json!({"op": "command", "command": id, "params": params}),
        )
    }

    /// The uids of the session's documents, in order, and the uid of the one on show.
    fn documents(board: &BoardDoc) -> (Vec<u64>, u64) {
        let session = &board.session;
        (
            session.docs.iter().map(|doc| doc.uid).collect(),
            session.ds().unwrap().uid,
        )
    }

    /// A second board in `dir`, the kind of file `file.recover` opens. It has two stickies, so
    /// it can't be taken for a canvas that has one.
    fn other_board_file(dir: &std::path::Path) -> String {
        let mut other = BoardDoc::new_board("Other").unwrap();
        for x in [40, 400] {
            other
                .execute("board.sticky", &json!({"x": x, "y": 40, "text": "other"}))
                .unwrap();
        }
        let path = dir.join("other.figcraft");
        std::fs::write(&path, other.to_bytes().unwrap()).unwrap();
        path.to_string_lossy().into_owned()
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

    /// The answer to a command that Sawit handles itself.
    fn handled_by_sawit(id: &str) -> Value {
        json!({"ok": false, "error": format!("{id} is handled by Sawit")})
    }

    #[test]
    fn get_lists_top_level_layers() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "one"}))
            .unwrap();
        board
            .execute("board.sticky", &json!({"x": 300, "y": 200, "text": "two"}))
            .unwrap();
        // Each sticky has a text layer of its own inside it, which `get` doesn't list.
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let top_level = doc.node(page).unwrap().children.clone();
        assert_eq!(top_level.len(), 2);
        assert!(
            top_level
                .iter()
                .all(|id| !doc.node(*id).unwrap().children.is_empty())
        );

        let elements = elements(&mut board);
        assert_eq!(elements.len(), 2, "{elements:?}");
        for element in &elements {
            let id = element["id"].as_str().expect("an id string");
            assert!(id.parse::<u64>().is_ok(), "{id} is not a number");
            assert_eq!(element["type"], json!("other"));
        }
        let ids: Vec<String> = top_level.iter().map(ToString::to_string).collect();
        assert_eq!(elements[0]["id"], json!(ids[0]));
        assert_eq!(elements[1]["id"], json!(ids[1]));
    }

    #[test]
    fn get_gives_each_layer_its_name_and_page_bounds() {
        let mut board = board();
        board
            .execute("board.sticky", &json!({"x": 300, "y": 200, "text": "two"}))
            .unwrap();
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let id = doc.node(page).unwrap().children[0];
        let bounds = doc.world_bounds(id).unwrap();

        let elements = elements(&mut board);
        let element = &elements[0];
        assert_eq!(element["id"], json!(id.to_string()));
        assert_eq!(element["name"], json!(doc.node(id).unwrap().name));
        assert_eq!(element["x"], json!(bounds.x0));
        assert_eq!(element["y"], json!(bounds.y0));
        assert_eq!(element["width"], json!(bounds.width()));
        assert_eq!(element["height"], json!(bounds.height()));
        // Where the sticky was put, and a size it has.
        assert!((bounds.x0 - 300.0).abs() < 1e-6 && (bounds.y0 - 200.0).abs() < 1e-6);
        assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
    }

    #[test]
    fn get_lists_only_the_page_on_show() {
        let mut board = board();
        assert!(elements(&mut board).is_empty());
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        assert_eq!(elements(&mut board).len(), 1);
        // A new page is empty and on show; the sticky is on the first page.
        board.execute("page.add", &json!({})).unwrap();
        assert!(elements(&mut board).is_empty());
        board.execute("page.select", &json!({"index": 0})).unwrap();
        assert_eq!(elements(&mut board).len(), 1);
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
        let target = dir.path().join("copy.figcraft");
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
        let missing = dir.path().join("missing.figcraft");
        let mut board = board();
        let (documents_before, doc_before) = (documents(&board), board.doc().unwrap());

        let reply = send(&mut board, "file.recover", json!({"path": missing}));
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(error.contains("missing.figcraft"), "{error}");
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

    #[test]
    fn without_a_document_ops_answer_with_an_error() {
        let mut board = BoardDoc {
            session: figcraft_engine::Session::empty(),
        };
        for op in [
            json!({"op": "get"}),
            json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}}),
        ] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            assert!(reply["error"].as_str().is_some_and(|e| !e.is_empty()));
        }
    }

    #[test]
    fn unknown_op_is_reported() {
        let mut board = board();
        let reply = run(
            &mut board,
            &json!({"op": "mermaid", "source": "graph TD; A-->B"}),
        );
        assert_eq!(
            reply,
            json!({"ok": false, "error": "mermaid is not supported yet"})
        );
        // An op with no name, or that isn't an op at all.
        for op in [json!({}), json!("get"), json!({"op": 3}), json!(null)] {
            let reply = run(&mut board, &op);
            assert_eq!(reply["ok"], json!(false), "{op}: {reply}");
            assert!(reply["error"].as_str().is_some_and(|e| e.contains("op")));
        }
    }
}
