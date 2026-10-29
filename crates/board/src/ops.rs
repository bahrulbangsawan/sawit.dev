//! The canvas's control ops: the `{"op": ...}` messages of the agent wire contract (spec, "Agent
//! wire contract"), run on a [`BoardDoc`] and answered as JSON.
//!
//! Every answer is `{ok: true, ...}` or `{ok: false, error}`, and an op never panics: a failure of
//! the engine, or an op that is malformed, is an `error` the agent can read. For now there are two
//! ops, `get` and `command`; the others of the contract get their own answer, "<op> is not
//! supported yet".

use std::fmt::Display;

use serde_json::{Value, json};

use crate::doc::{BoardDoc, BoardError};

/// The commands the `command` op refuses: they switch, open, close or write documents behind the
/// canvas's store. Sawit does these itself.
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

/// Runs `op` (an object whose `op` is the op's name) on `board` and answers it: `{ok: true, ...}`,
/// or `{ok: false, error}`.
///
/// - `get` answers `{ok, elements}`: the top-level layers of the page on show, back to front, as
///   `{id, type: "other", name, x, y, width, height}`. The `id` is the layer's FigCraft node id as a
///   string, and the box is its bounds in page coordinates.
/// - `command` (`{command, params?}`) runs engine command `command` and answers `{ok, result}`,
///   or the engine's own `error`. It refuses the commands Sawit handles itself.
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
    if HANDLED_BY_SAWIT.contains(&id) {
        return failure(format!("{id} is handled by Sawit"));
    }
    let params = match op.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(params) => params.clone(),
    };
    match board.execute(id, &params) {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => failure(error),
    }
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
