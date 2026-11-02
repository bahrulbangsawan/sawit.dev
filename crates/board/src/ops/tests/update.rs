//! The `update` op.

use std::sync::Arc;

use figcraft_color::{Color, Paint};
use figcraft_doc::NodeId;
use serde_json::json;

use super::*;

/// The first solid fill and stroke of layer `id`, as `#RRGGBB`.
fn paints(board: &BoardDoc, id: &str) -> (Option<String>, Option<String>) {
    let doc = board.doc().unwrap();
    let node = doc.node(id.parse().unwrap()).unwrap();
    let first = |paints: &[Paint]| paints.iter().find_map(Paint::solid_color);
    let (fill, stroke) = (first(&node.fills), first(&node.strokes));
    (fill.map(Color::to_css_hex), stroke.map(Color::to_css_hex))
}

#[test]
fn update_moves_and_relabels() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([{"type": "sticky", "id": "s", "x": 0, "y": 20, "text": "Old"}]),
    );

    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": ids[0], "x": 500, "text": "New"}]}),
    );

    assert_eq!(reply, json!({"ok": true, "updated": 1, "missing": []}));
    let element = listed(&mut board, &ids[0]);
    assert_eq!(element["x"], json!(500.0));
    // What the element didn't name stays.
    assert_eq!(element["y"], json!(20.0));
    assert_eq!(element["text"], json!("New"));
}

#[test]
fn update_reports_missing() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([{"type": "rectangle", "id": "box", "x": 0, "y": 0}]),
    );
    let before = board.doc().unwrap();

    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": "nope", "x": 5}]}),
    );
    assert_eq!(
        reply,
        json!({"ok": true, "updated": 0, "missing": ["nope"]})
    );
    // Nothing changed, so there is no step to undo.
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), (1, 0));

    // What is there is updated, what isn't is named, in the order it was given. The page
    // is no element, though its node id is a number like the others'.
    let page = board.doc().unwrap().first_page().unwrap().to_string();
    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [
            {"id": "gone", "x": 1}, {"id": "box", "x": 5}, {"id": page, "x": 5}, {"id": ids[0], "y": 7},
        ]}),
    );
    assert_eq!(reply["updated"], json!(2));
    assert_eq!(reply["missing"], json!(["gone", page]));
    let element = listed(&mut board, &ids[0]);
    assert_eq!(
        (element["x"].clone(), element["y"].clone()),
        (json!(5.0), json!(7.0))
    );
}

#[test]
fn update_changes_size_text_name_and_colours() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "rectangle", "id": "box", "x": 0, "y": 0, "width": 100, "height": 60,
             "label": {"text": "Old"}},
            {"type": "sticky", "id": "note", "x": 300, "y": 0, "text": "Old"},
            {"type": "text", "id": "title", "x": 0, "y": -100, "text": "Old"},
            {"type": "frame", "id": "area", "x": 0, "y": 300, "width": 200, "height": 100,
             "name": "Old"},
            {"type": "arrow", "id": "link", "start": {"id": "box"}, "end": {"id": "note"}},
        ]),
    );

    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [
            {"id": "box", "x": 20, "y": 40, "width": 140, "height": 80, "text": "Box",
             "backgroundColor": "#2f9e44", "strokeColor": "#e03131"},
            {"id": "note", "label": {"text": "Note"}, "backgroundColor": "pink"},
            {"id": "title", "text": "Title", "strokeColor": "#e03131"},
            {"id": "area", "name": "Area", "width": 250},
            {"id": "link", "text": "uses", "strokeColor": "#1971c2"},
        ]}),
    );
    assert_eq!(reply, json!({"ok": true, "updated": 5, "missing": []}));

    let shape = listed(&mut board, &ids[0]);
    assert_eq!(
        [&shape["x"], &shape["y"], &shape["width"], &shape["height"]],
        [&json!(20.0), &json!(40.0), &json!(140.0), &json!(80.0)]
    );
    assert_eq!(shape["text"], json!("Box"));
    assert_eq!(shape["color"], json!("#2F9E44"));
    // The stroke is the outline's, the layer inside the shape.
    let doc = board.doc().unwrap();
    let outline = doc.node(ids[0].parse().unwrap()).unwrap().children[0];
    assert_eq!(
        paints(&board, &outline.to_string()).1.as_deref(),
        Some("#E03131")
    );

    let sticky = listed(&mut board, &ids[1]);
    assert_eq!(sticky["text"], json!("Note"));
    assert_eq!(sticky["color"], json!("#F7B9DA"));

    // A text layer's colour is its `strokeColor` (P7-C24).
    let text = listed(&mut board, &ids[2]);
    assert_eq!(text["text"], json!("Title"));
    assert_eq!(text["color"], json!("#E03131"));
    assert_eq!(paints(&board, &ids[2]).1, None);

    let frame = listed(&mut board, &ids[3]);
    assert_eq!(frame["name"], json!("Area"));
    assert_eq!(frame["width"], json!(250.0));

    // A connector's text is a label, made on the way.
    let connector = listed(&mut board, &ids[4]);
    assert_eq!(connector["text"], json!("uses"));
    assert_eq!(connector["color"], json!("#1971C2"));
    assert_eq!(labels(&board), 1);
    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": "link", "text": ""}]}),
    );
    assert_eq!(reply["updated"], json!(1));
    assert!(listed(&mut board, &ids[4]).get("text").is_none());
    assert_eq!(labels(&board), 0);
}

#[test]
fn update_colours_by_the_rules_apply_uses() {
    // One helper colours what `add` draws and what `update` changes (P7-C12), so naming the
    // colours when drawing and naming them afterwards give the same document.
    let bare = json!([
        {"type": "rectangle", "x": 0, "y": 0},
        {"type": "rectangle", "x": 300, "y": 0},
        {"type": "sticky", "x": 600, "y": 0},
        {"type": "text", "x": 0, "y": 300, "text": "t"},
        {"type": "arrow", "x": 0, "y": 400, "width": 100, "height": 0},
        {"type": "freedraw", "x": 0, "y": 500, "points": [[0, 0], [30, 40], [60, 0]]},
        {"type": "frame", "x": 0, "y": 600, "width": 200, "height": 100},
    ]);
    let colours = [
        json!({"backgroundColor": "#74c0fc", "strokeColor": "#e03131"}),
        json!({"backgroundColor": "transparent", "strokeColor": "#2f9e44"}),
        json!({"backgroundColor": "violet"}),
        json!({"backgroundColor": "#ff0000", "strokeColor": "#1971c2"}),
        // A connector has no fill: the background would recolour its line, so it is ignored.
        json!({"backgroundColor": "#ffc9c9", "strokeColor": "#9c36b5"}),
        json!({"backgroundColor": "#ffc9c9", "strokeColor": "#e8590c"}),
        json!({"backgroundColor": "#d3f9d8", "strokeColor": "#2f9e44"}),
    ];

    let mut drawn = board();
    let mut elements = bare.clone();
    for (element, colours) in elements.as_array_mut().unwrap().iter_mut().zip(&colours) {
        element
            .as_object_mut()
            .unwrap()
            .extend(colours.as_object().unwrap().clone());
    }
    add(&mut drawn, elements);

    let mut changed = board();
    let ids = add(&mut changed, bare);
    let patches: Vec<Value> = ids
        .iter()
        .zip(&colours)
        .map(|(id, colours)| {
            let mut patch = colours.clone();
            patch["id"] = json!(id);
            patch
        })
        .collect();
    ask(&mut changed, json!({"op": "update", "elements": patches}));

    let (left, right) = (drawn.doc().unwrap(), changed.doc().unwrap());
    for (a, b) in left.nodes().zip(right.nodes()) {
        assert_eq!(a, b, "layer {} ({}) differs", a.id, a.name);
    }
    assert!(
        *left == *right,
        "the layers are alike but the documents differ"
    );
    // And the colours are not nothing: the connector's line is its stroke, not its background.
    assert_eq!(paints(&changed, &ids[4]).1.as_deref(), Some("#9C36B5"));
    assert_eq!(paints(&changed, &ids[1]).0, None);
}

#[test]
fn update_places_layers_in_page_coordinates() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "frame", "id": "area", "x": 100, "y": 100, "width": 400, "height": 300},
            {"type": "rectangle", "id": "inner", "x": 150, "y": 160, "width": 50, "height": 50,
             "frameId": "area"},
        ]),
    );
    let inner: NodeId = ids[1].parse().unwrap();
    let doc = board.doc().unwrap();
    assert_eq!(
        doc.node(inner).unwrap().parent,
        Some(ids[0].parse().unwrap())
    );
    // It is inside the frame, so `get` lists the frame alone.
    assert_eq!(elements(&mut board).len(), 1);

    ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": "inner", "x": 300, "width": 80}]}),
    );

    // Where `add` put it is where `update` puts it: the page's coordinates, not the frame's.
    let bounds = board.doc().unwrap().world_bounds(inner).unwrap();
    assert_eq!(
        [bounds.x0, bounds.y0, bounds.width(), bounds.height()],
        [300.0, 160.0, 80.0, 50.0]
    );
}

#[test]
fn update_finds_layers_by_agent_id_and_by_node_id() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0},
            {"type": "rectangle", "id": "b", "x": 0, "y": 200},
            {"type": "rectangle", "id": "c", "x": 0, "y": 400},
        ]),
    );
    let number: u64 = ids[2].parse().unwrap();

    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [
            {"id": "a", "x": 11}, {"id": ids[1], "x": 22}, {"id": number, "x": 33},
        ]}),
    );
    assert_eq!(reply, json!({"ok": true, "updated": 3, "missing": []}));
    let xs: Vec<Value> = ids
        .iter()
        .map(|id| listed(&mut board, id)["x"].clone())
        .collect();
    assert_eq!(xs, [json!(11.0), json!(22.0), json!(33.0)]);

    // An agent's id wins over a node id that looks the same (a node is called "1" too).
    add(
        &mut board,
        json!([{"type": "rectangle", "id": ids[0], "x": 0, "y": 600}]),
    );
    ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": ids[0], "x": 99}]}),
    );
    assert_eq!(listed(&mut board, &ids[0])["x"], json!(11.0));
    let theirs = elements(&mut board)
        .into_iter()
        .find(|e| e["y"] == json!(600.0))
        .unwrap();
    assert_eq!(theirs["x"], json!(99.0));
}

#[test]
fn update_is_one_undo_step_and_fails_whole() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "rectangle", "id": "a", "x": 0, "y": 0},
            {"type": "rectangle", "id": "b", "x": 0, "y": 200},
        ]),
    );
    ask(
        &mut board,
        json!({"op": "update", "elements": [{"id": "a", "x": 50, "text": "A"}, {"id": "b", "x": 60}]}),
    );
    // Two elements, three changes, one step.
    assert_eq!(board.session.undo_depth(), (2, 0));
    assert_eq!(board.session.undo_labels(1), ["Update elements"]);
    ask(&mut board, json!({"op": "command", "command": "edit.undo"}));
    assert_eq!(listed(&mut board, &ids[0])["x"], json!(0.0));
    assert_eq!(listed(&mut board, &ids[1])["x"], json!(0.0));
    assert!(listed(&mut board, &ids[0]).get("text").is_none());

    // What the engine refuses fails the call whole, naming the element: the first element's
    // change is not kept.
    let before = board.doc().unwrap();
    let steps = board.session.undo_depth();
    let error = refusal(
        &mut board,
        json!({"op": "update", "elements": [
            {"id": "a", "x": 50}, {"id": "b", "backgroundColor": "not a colour"},
        ]}),
    );
    assert!(
        error.contains("elements[1]") && error.contains("b"),
        "{error}"
    );
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
    assert_eq!(board.session.undo_depth(), steps);
}

#[test]
fn update_reports_malformed_input() {
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "rectangle", "id": "a", "x": 0, "y": 0}]),
    );
    let before = board.doc().unwrap();
    for op in [
        json!({"op": "update"}),
        json!({"op": "update", "elements": "a"}),
        json!({"op": "update", "elements": [{"x": 5}]}),
        json!({"op": "update", "elements": [{"id": ""}]}),
        json!({"op": "update", "elements": [{"id": null, "x": 5}]}),
        json!({"op": "update", "elements": ["a"]}),
        json!({"op": "update", "elements": [{"id": "a", "x": "far"}]}),
        json!({"op": "update", "elements": [{"id": "a", "text": 5}]}),
    ] {
        let error = refusal(&mut board, op.clone());
        assert!(error.contains("elements"), "{op}: {error}");
    }
    assert!(Arc::ptr_eq(&before, &board.doc().unwrap()));
}

#[test]
fn update_notes_text_that_a_layer_cannot_take() {
    let mut board = board();
    let ids = add(
        &mut board,
        json!([
            {"type": "freedraw", "x": 0, "y": 0, "points": [[0, 0], [30, 40], [60, 0]]},
            {"type": "rectangle", "x": 200, "y": 0},
        ]),
    );
    let reply = ask(
        &mut board,
        json!({"op": "update", "elements": [
            {"id": ids[0], "text": "ink can't say it", "x": 5}, {"id": ids[1], "text": "a shape can"},
        ]}),
    );
    // It is not an error: the rest of the element, and the others, are updated.
    assert_eq!(reply["updated"], json!(2));
    let notes = reply["notes"].as_array().expect("notes");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].as_str().unwrap().contains("elements[0]"),
        "{notes:?}"
    );
    assert_eq!(listed(&mut board, &ids[0])["x"], json!(5.0));
    assert_eq!(listed(&mut board, &ids[1])["text"], json!("a shape can"));
}
