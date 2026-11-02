//! The `replace` op with an Excalidraw scene or file.

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::doc::DocKind;

#[test]
fn replace_loads_an_excalidraw_scene_as_one_undo_step() {
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
    );
    // Two pages of its own: both are replaced.
    board
        .execute("page.add", &json!({"name": "Notes"}))
        .unwrap();
    board.execute("page.select", &json!({"index": 0})).unwrap();
    let (uid, steps) = (board.session.ds().unwrap().uid, board.session.undo_depth());

    let reply = ask(&mut board, json!({"op": "replace", "scene": scene()}));

    // The canvas shows the scene, and says what is on it as `get` does, and what was left out.
    let now = elements(&mut board);
    assert_eq!(now.len(), 9, "{now:?}");
    assert_eq!(reply["elements"], json!(now));
    assert_eq!(reply["notes"], json!(["skipped 1 embed"]));
    assert_eq!(board.doc().unwrap().pages.len(), 1);
    assert!(board.session.selection().is_empty());
    // In the canvas's own session: one document, the one it had, and still a board.
    assert_eq!(board.session.docs.len(), 1);
    assert_eq!(board.session.ds().unwrap().uid, uid);
    assert_eq!(board.kind(), DocKind::Board);
    // One step: undo brings the old board back, pages and all, and redo the scene.
    assert_eq!(board.session.undo_depth(), (steps.0 + 1, 0));
    assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
    ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
    assert_eq!(board.doc().unwrap().pages.len(), 2);
    let back = elements(&mut board);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0]["text"], json!("mine"));
    ask(&mut board, json!({"op": "command", "command": "edit.redo"}));
    assert_eq!(elements(&mut board).len(), 9);
}

#[test]
fn a_replaced_scene_keeps_what_it_joins_and_the_ids_it_had() {
    let mut board = board();
    let reply = ask(&mut board, json!({"op": "replace", "scene": scene()}));
    let now = reply["elements"].as_array().unwrap();
    let of = |kind: &str| now.iter().find(|e| e["type"] == json!(kind)).unwrap();
    // The arrow still joins the rectangle and the ellipse, which are in the new canvas.
    let arrow = now.iter().find(|e| e.get("from").is_some()).unwrap();
    assert_eq!(arrow["from"], of("rectangle")["id"]);
    assert_eq!(arrow["to"], of("ellipse")["id"]);
    // The picture and the section came through the engine's import.
    assert_eq!(of("image")["width"], json!(120.0));
    assert_eq!(of("section")["name"], json!("Group A"));
    let doc = board.doc().unwrap();
    let section = doc
        .node(of("section")["id"].as_str().unwrap().parse().unwrap())
        .unwrap();
    assert_eq!(section.children.len(), 1);
    // The ids the elements had in Excalidraw name them still.
    let rectangle = of("rectangle")["id"].as_str().unwrap().to_string();
    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": "box", "text": "Renamed"}]}),
    );
    assert_eq!(reply["updated"], json!(1), "{reply}");
    assert_eq!(listed(&mut board, &rectangle)["text"], json!("Renamed"));
}

#[test]
fn replace_loads_an_excalidraw_file() {
    let dir = tempfile::tempdir().unwrap();
    let text = include_str!("../../../tests/fixtures/excalidraw/basic.excalidraw");
    for name in ["board.excalidraw", "BOARD.Excalidraw"] {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        let mut board = board();
        add(
            &mut board,
            json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
        );

        let reply = ask(&mut board, json!({"op": "replace", "path": path}));

        assert_eq!(reply["elements"].as_array().unwrap().len(), 9, "{name}");
        assert_eq!(reply["notes"], json!(["skipped 1 embed"]));
        assert_eq!(elements(&mut board).len(), 9);
        assert_eq!(board.session.undo_labels(1), ["Replace canvas"]);
        // The file is read, never written.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn a_scene_wins_over_a_path_and_an_empty_one_clears_the_canvas() {
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
    );
    let reply = ask(
        &mut board,
        json!({"op": "replace", "scene": {"elements": []}, "path": "/nowhere/a.sawit"}),
    );
    assert_eq!(reply, json!({"ok": true, "elements": []}));
    assert!(elements(&mut board).is_empty());
    ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
    assert_eq!(elements(&mut board).len(), 1);
    // `null` is no scene: the path is what counts.
    let error = refusal(
        &mut board,
        json!({"op": "replace", "scene": null, "path": "/nowhere/a.sawit"}),
    );
    assert!(error.contains("a.sawit"), "{error}");
}

#[test]
fn replace_of_a_bad_scene_or_excalidraw_file_is_an_error_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    };
    let not_json = write("not-json.excalidraw", "{ not json");
    let no_elements = write("no-elements.excalidraw", r#"{"type": "excalidraw"}"#);
    // A file with nothing in it opens blank as the project canvas (the migration), but nobody
    // loads nothing on purpose: `replace` refuses it, as `canvas_load` always has.
    let empty = write("empty.excalidraw", "");
    let spaces = write("spaces.excalidraw", " \n");
    let bom = write("bom.excalidraw", "\u{feff}");
    let missing = dir.path().join("missing.excalidraw");
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "sticky", "x": 0, "y": 0, "text": "mine"}]),
    );
    let (before, steps) = (board.doc().unwrap(), board.session.undo_depth());

    for (op, says) in [
        (json!({"op": "replace", "scene": {}}), "`elements`"),
        (json!({"op": "replace", "scene": [1, 2]}), "`elements`"),
        (json!({"op": "replace", "scene": "scene"}), "`elements`"),
        (
            json!({"op": "replace", "path": missing}),
            "missing.excalidraw",
        ),
        (
            json!({"op": "replace", "path": not_json}),
            "isn't valid JSON",
        ),
        (json!({"op": "replace", "path": no_elements}), "`elements`"),
        (json!({"op": "replace", "path": empty}), "isn't valid JSON"),
        (json!({"op": "replace", "path": spaces}), "isn't valid JSON"),
        (json!({"op": "replace", "path": bom}), "isn't valid JSON"),
    ] {
        let error = refusal(&mut board, op.clone());
        assert!(error.starts_with("replace: "), "{op}: {error}");
        assert!(error.contains(says), "{op}: {error}");
    }
    // The file that is no scene is named, not only said to be one.
    let error = refusal(&mut board, json!({"op": "replace", "path": no_elements}));
    assert!(error.contains("no-elements.excalidraw"), "{error}");

    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), steps);
    assert_eq!(elements(&mut board).len(), 1);
}
