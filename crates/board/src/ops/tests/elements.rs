//! The `get`, `add`, `delete` and `clear` ops.

use std::sync::Arc;

use figcraft_doc::NodeId;
use serde_json::json;

use super::*;

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
        assert_eq!(element["type"], json!("sticky"));
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

/// The layers of the page on show, labels among them.
fn layers_on_page(board: &BoardDoc) -> usize {
    let ds = board.session.ds().unwrap();
    ds.doc.node(ds.page).unwrap().children.len()
}

#[test]
fn get_summarises_types_and_text() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "sticky", "id": "s", "x": 0, "y": 0, "text": "hi"},
            {"type": "diamond", "id": "d", "x": 400, "y": 0},
            {"type": "arrow", "start": {"id": "s"}, "end": {"id": "d"}, "label": {"text": "yes"}},
        ]),
    );

    // Three elements: the connector's label is a layer of its own, but not an element (P7-C4).
    assert_eq!(layers_on_page(&board), 4);
    let elements = elements(&mut board);
    assert_eq!(elements.len(), 3, "{elements:?}");
    let types: Vec<&str> = elements
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["sticky", "diamond", "connector"]);
    let listed_ids: Vec<&str> = elements.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(listed_ids, ids);

    assert_eq!(elements[0]["text"], json!("hi"));
    assert!(elements[1].get("text").is_none(), "{}", elements[1]);
    // The connector says what it joins, and its text is its label's.
    assert_eq!(elements[2]["from"], json!(ids[0]));
    assert_eq!(elements[2]["to"], json!(ids[1]));
    assert_eq!(elements[2]["text"], json!("yes"));
    for element in &elements {
        for key in ["x", "y", "width", "height"] {
            assert!(element[key].is_number(), "{key} of {element}");
        }
    }
}

#[test]
fn add_returns_ids_in_order() {
    let mut board = board();
    // Made in another order (shapes before text, text before what joins them), answered in
    // this one.
    let reply = ask(
        &mut board,
        json!({"op": "add", "elements": [
            {"type": "arrow", "start": {"id": "a"}, "end": {"id": "b"}},
            {"type": "text", "x": 0, "y": -50, "text": "Title"},
            {"type": "rectangle", "id": "b", "x": 300, "y": 0, "width": 100, "height": 60},
            {"type": "rectangle", "id": "a", "x": 0, "y": 0, "width": 100, "height": 60},
        ]}),
    );
    assert_eq!(reply["added"], json!(4));
    assert!(reply.get("notes").is_none(), "{reply}");
    let ids: Vec<String> = reply["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 4);
    let at = |board: &mut BoardDoc, id: &String| {
        let element = listed(board, id);
        (element["type"].clone(), element["x"].clone())
    };
    assert_eq!(at(&mut board, &ids[0]).0, json!("connector"));
    assert_eq!(at(&mut board, &ids[1]), (json!("text"), json!(0.0)));
    assert_eq!(at(&mut board, &ids[2]), (json!("rectangle"), json!(300.0)));
    assert_eq!(at(&mut board, &ids[3]), (json!("rectangle"), json!(0.0)));
    // The arrow joins the two rectangles by the ids the agent gave them.
    let connector = listed(&mut board, &ids[0]);
    assert_eq!(connector["from"], json!(ids[3]));
    assert_eq!(connector["to"], json!(ids[2]));
}

#[test]
fn add_notes_what_it_skips() {
    let mut board = board();
    let reply = ask(
        &mut board,
        json!({"op": "add", "elements": [
            {"type": "hologram", "x": 0, "y": 0},
            {"type": "rectangle", "x": 0, "y": 0},
        ]}),
    );
    // Skipped with a note, not an error, and the rest is drawn.
    assert_eq!(reply["added"], json!(1));
    assert_eq!(reply["ids"].as_array().map(Vec::len), Some(1));
    let notes = reply["notes"].as_array().expect("notes");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].as_str().unwrap().contains("hologram"), "{notes:?}");
    assert_eq!(elements(&mut board).len(), 1);

    // Nothing to add adds nothing.
    let reply = ask(&mut board, json!({"op": "add", "elements": []}));
    assert_eq!(reply, json!({"ok": true, "added": 0, "ids": []}));
}

#[test]
fn add_reports_malformed_elements() {
    let mut board = board();
    let before = board.doc().unwrap();
    for op in [
        json!({"op": "add"}),
        json!({"op": "add", "elements": null}),
        json!({"op": "add", "elements": "rectangle"}),
        json!({"op": "add", "elements": {"type": "rectangle"}}),
        json!({"op": "add", "elements": ["rectangle"]}),
        json!({"op": "add", "elements": [{"type": "rectangle", "x": "far away"}]}),
        json!({"op": "add", "elements": [{"type": "rectangle", "x": 0, "y": 0}, 5]}),
    ] {
        let error = refusal(&mut board, op.clone());
        assert!(error.contains("elements"), "{op}: {error}");
    }
    // The engine refusing an element fails the whole call, and says which one.
    let error = refusal(
        &mut board,
        json!({"op": "add", "elements": [
            {"type": "rectangle", "x": 0, "y": 0},
            {"type": "rectangle", "x": 0, "y": 0, "width": 0},
        ]}),
    );
    assert!(error.contains("elements[1]"), "{error}");
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), (0, 0));
}

/// SMOKE-3: what only a board has (shapes, stickies, lines, ink) doesn't go on a design file.
/// The refusal says so in Sawit's words, with what does go there and the commands that draw
/// the rest, instead of sending the agent to File ▸ New board, which the canvas hides (and no
/// op makes a design file a board: `replace` keeps the canvas what it is). Text, frames and
/// images still come in.
#[test]
fn add_to_a_design_file_is_refused() {
    let mut design = design();
    let before = design.doc().unwrap();
    let error = refusal(
        &mut design,
        json!({"op": "add", "elements": [{"type": "rectangle", "x": 0, "y": 0}]}),
    );
    assert_eq!(
        error,
        "add: elements[0] (rectangle): command `board.shape` is not available right now: this \
             is for boards, and this canvas is a design file. On a design file add draws text, \
             frames and images; draw shapes and lines with the command op: shape.rectangle, \
             shape.ellipse, shape.line or shape.arrow"
    );
    for kind in ["ellipse", "diamond", "sticky", "arrow", "line", "freedraw"] {
        let element = json!({"type": kind, "x": 0, "y": 0, "points": [[0, 0], [10, 10]]});
        let error = refusal(&mut design, json!({"op": "add", "elements": [element]}));
        assert!(
            error.contains("this canvas is a design file. On a design file add draws"),
            "{kind}: {error}"
        );
        assert!(!error.contains("New board"), "{kind}: {error}");
    }
    assert!(Arc::ptr_eq(&before, &design.doc().unwrap()));
    for kind in ["text", "frame"] {
        let element = json!({"type": kind, "x": 0, "y": 0, "width": 100, "text": "kept"});
        ask(&mut design, json!({"op": "add", "elements": [element]}));
    }
    assert_eq!(elements(&mut design).len(), 2);
}

#[test]
fn the_users_selection_survives_update_and_delete() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0},
            {"type": "rectangle", "id": "b", "x": 300, "y": 0},
            {"type": "rectangle", "id": "c", "x": 600, "y": 0},
        ]),
    );
    let nodes: Vec<NodeId> = ids.iter().map(|id| id.parse().unwrap()).collect();
    board.session.set_selection(vec![nodes[0], nodes[2]]);

    ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": "b", "x": 5, "text": "B", "backgroundColor": "red"}]}),
    );
    assert_eq!(board.session.selection(), [nodes[0], nodes[2]]);

    // What is deleted leaves the selection, and what isn't deleted stays in it.
    ask(&mut board, json!({"op": "delete", "ids": ["b", "c"]}));
    assert_eq!(board.session.selection(), [nodes[0]]);
}

#[test]
fn delete_removes_connector_label() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0},
            {"type": "rectangle", "id": "b", "x": 400, "y": 0},
            {"type": "arrow", "id": "ab", "start": {"id": "a"}, "end": {"id": "b"},
             "label": {"text": "yes"}},
        ]),
    );
    assert_eq!(labels(&board), 1);
    assert_eq!(layers_on_page(&board), 4);

    let reply = ask(&mut board, json!({"op": "delete", "ids": [ids[2]]}));

    assert_eq!(reply, json!({"ok": true, "deleted": 1, "missing": []}));
    // The label went with its connector, and the shapes it joined stay.
    assert_eq!(labels(&board), 0);
    assert_eq!(layers_on_page(&board), 2);
    let types: Vec<String> = elements(&mut board)
        .iter()
        .map(|e| e["type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(types, ["rectangle", "rectangle"]);
    // One step takes it back, label and all.
    ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
    assert_eq!((labels(&board), layers_on_page(&board)), (1, 4));
}

#[test]
fn delete_reports_missing_and_counts_what_it_found() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0},
            {"type": "rectangle", "id": "b", "x": 300, "y": 0},
            {"type": "rectangle", "id": "c", "x": 600, "y": 0},
        ]),
    );
    let page = board.doc().unwrap().first_page().unwrap().to_string();
    let number: u64 = ids[2].parse().unwrap();

    // By an agent's id, by a node id, twice the same layer, a layer that isn't there, and the
    // page, which is no element.
    let reply = ask(
        &mut board,
        json!({"op": "delete", "ids": ["a", ids[0], "nope", page, number, "b"]}),
    );

    assert_eq!(reply["deleted"], json!(3));
    assert_eq!(reply["missing"], json!(["nope", page]));
    assert!(elements(&mut board).is_empty());
    assert_eq!(board.doc().unwrap().pages.len(), 1);

    // Nothing found, nothing done.
    let before = board.doc().unwrap();
    let reply = ask(&mut board, json!({"op": "delete", "ids": ["x", "y"]}));
    assert_eq!(
        reply,
        json!({"ok": true, "deleted": 0, "missing": ["x", "y"]})
    );
    let reply = ask(&mut board, json!({"op": "delete", "ids": []}));
    assert_eq!(reply, json!({"ok": true, "deleted": 0, "missing": []}));
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
}

#[test]
fn delete_never_falls_back_to_the_selection() {
    // The engine's delete takes the selection when it is given no layers.
    let mut board = board();
    let ids = add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
    board.session.set_selection(vec![ids[0].parse().unwrap()]);
    for ids in [json!([]), json!(["nope"])] {
        ask(&mut board, json!({"op": "delete", "ids": ids}));
    }
    assert_eq!(elements(&mut board).len(), 1);
}

#[test]
fn delete_reports_malformed_input() {
    let mut board = board();
    add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
    for op in [
        json!({"op": "delete"}),
        json!({"op": "delete", "ids": "a"}),
        json!({"op": "delete", "ids": ["a", {"id": "b"}]}),
        json!({"op": "delete", "ids": [null]}),
    ] {
        let error = refusal(&mut board, op.clone());
        assert!(error.contains("ids"), "{op}: {error}");
    }
    assert_eq!(elements(&mut board).len(), 1);
}

#[test]
fn clear_is_one_undo_step() {
    let mut board = board();
    add(
        &mut board,
        json!([
            {"type": "sticky", "id": "s", "x": 0, "y": 0, "text": "hi"},
            {"type": "ellipse", "id": "e", "x": 400, "y": 0},
            {"type": "arrow", "start": {"id": "s"}, "end": {"id": "e"}, "label": {"text": "yes"}},
            {"type": "freedraw", "x": 0, "y": 500, "points": [[0, 0], [30, 40], [60, 0]]},
        ]),
    );
    let (layers, steps) = (layers_on_page(&board), board.session.undo_depth().0);
    assert_eq!((elements(&mut board).len(), layers), (4, 5));

    let reply = ask(&mut board, json!({"op": "clear"}));

    assert_eq!(reply, json!({"ok": true}));
    assert!(elements(&mut board).is_empty());
    // Every layer is gone, the connector's label too, in one step.
    assert_eq!(layers_on_page(&board), 0);
    assert_eq!(board.session.undo_depth(), (steps + 1, 0));
    ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
    assert_eq!(layers_on_page(&board), layers);
    assert_eq!(elements(&mut board).len(), 4);
    assert_eq!(board.session.undo_depth(), (steps, 1));
}

#[test]
fn clear_of_an_empty_page_changes_nothing_and_only_that_page_is_cleared() {
    let mut board = board();
    let before = board.doc().unwrap();
    // The selection is empty too, which is what the engine's delete would fall back to.
    assert_eq!(ask(&mut board, json!({"op": "clear"})), json!({"ok": true}));
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), (0, 0));

    add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
    board.execute("page.add", &json!({})).unwrap();
    add(&mut board, json!([{"type": "rectangle", "x": 0, "y": 0}]));
    ask(&mut board, json!({"op": "clear"}));
    assert!(elements(&mut board).is_empty());
    board.execute("page.select", &json!({"index": 0})).unwrap();
    assert_eq!(elements(&mut board).len(), 1);
}
