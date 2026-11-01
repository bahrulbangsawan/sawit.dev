//! Keys in vector edit mode: its tools, and ⌘X on its points and segments.

use figcraft_engine::vectoredit::VTool;

use super::*;

/// The tool vector edit mode has chosen.
fn vector_tool(board: &BoardDoc) -> VTool {
    board
        .session
        .ds()
        .unwrap()
        .vector_edit
        .as_ref()
        .unwrap()
        .tool
}

/// A new open path through three points in a row at height `y`: A (100, y), B (200, y) and
/// C (300, y).
fn three_points(board: &mut BoardDoc, y: i32) -> NodeId {
    let made = board
        .execute(
            "vector.create",
            &json!({"points": [[100, y], [200, y], [300, y]], "closed": false}),
        )
        .unwrap();
    figcraft_engine::params::id_of(&made["id"]).unwrap()
}

/// The path A-B-C ([`three_points`]) edited as a vector, with B selected, as in FigCraft's
/// tests of the cut in vector edit mode (ui-egui vector_keys_tests.rs).
fn abc(board: &mut BoardDoc) -> NodeId {
    let path = three_points(board, 100);
    board
        .execute("vector.edit.begin", &json!({"id": path}))
        .unwrap();
    board
        .execute("vector.edit.select", &json!({"vertices": [1]}))
        .unwrap();
    path
}

#[test]
fn vector_edit_keys_reach_vector_edit_mode() {
    let mut board = board();
    let made = board
        .execute(
            "shape.rectangle",
            &json!({"x": 0, "y": 0, "width": 100, "height": 100}),
        )
        .unwrap();
    let rect = figcraft_engine::params::id_of(&made["id"]).unwrap();
    select(&mut board, rect);
    let main_tool = tool(&board);
    // Enter on a shape edits it as a vector.
    assert_eq!(
        route_key(&mut board, "Enter", NONE),
        Ok(KeyOutcome::Handled)
    );
    assert!(board.session.ds().unwrap().vector_edit.is_some());
    // GPUI's lower-case letters pick vector edit mode's tools, not the canvas's (X is the
    // connector, Q the sticky, ⇧E the Design/Prototype switch).
    for (key, mods, picked) in [
        ("x", NONE, VTool::Cut),
        ("q", NONE, VTool::Lasso),
        ("b", NONE, VTool::Paint),
        ("p", NONE, VTool::Pen),
        ("e", SHIFT, VTool::Eraser),
        ("w", SHIFT, VTool::Width),
        ("v", NONE, VTool::Move),
    ] {
        assert_eq!(
            route_key(&mut board, key, mods),
            Ok(KeyOutcome::Handled),
            "{key}"
        );
        assert_eq!(vector_tool(&board), picked, "{key}");
    }
    assert_eq!(tool(&board), main_tool);
    // ⌘A selects the shape's four points, not every layer.
    assert_eq!(route_key(&mut board, "a", CMD), Ok(KeyOutcome::Handled));
    let edit = board.session.ds().unwrap().vector_edit.clone().unwrap();
    assert_eq!(edit.vertices.len(), 4);
    assert_eq!(board.session.selection(), [rect]);
}

#[test]
fn cmd_x_in_vector_edit_mode_takes_the_selected_points_not_the_layer() {
    let mut board = board();
    let path = abc(&mut board);
    assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
    // B is gone and a heal joins A and C: the layer stays, still edited.
    assert!(board.doc().unwrap().node(path).is_some());
    let net = network(&board, path);
    assert_eq!(net.vertices.len(), 2, "{net:?}");
    assert_eq!(net.segments.len(), 1, "{net:?}");
    assert!(board.session.ds().unwrap().vector_edit.is_some());
    // Nothing was copied, so the view has nothing to put on the system clipboard.
    assert!(board.session.clipboard.nodes.is_empty());
}

#[test]
fn cmd_x_in_vector_edit_mode_with_nothing_selected_keeps_the_layer() {
    let mut board = board();
    let path = abc(&mut board);
    board
        .execute("vector.edit.select", &json!({"vertices": []}))
        .unwrap();
    let doc = board.doc().unwrap();
    assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
    assert_eq!(network(&board, path).vertices.len(), 3);
    assert_eq!(board.doc().unwrap(), doc);
    assert!(board.session.clipboard.nodes.is_empty());
}

#[test]
fn cmd_x_in_vector_edit_mode_takes_points_selected_in_another_edited_layer() {
    let mut board = board();
    let (a, b) = (three_points(&mut board, 100), three_points(&mut board, 200));
    vectoredit::begin_many(&mut board.session, &[a, b]).unwrap();
    // Points selected in the second layer, then the first made active: only the layer that
    // isn't active has a selection.
    board
        .execute("vector.edit.select", &json!({"id": b, "vertices": [1]}))
        .unwrap();
    board
        .execute("vector.edit.select", &json!({"id": a, "add": true}))
        .unwrap();
    assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
    let doc = board.doc().unwrap();
    assert!(doc.node(a).is_some() && doc.node(b).is_some());
    assert_eq!(network(&board, a).vertices.len(), 3);
    assert_eq!(network(&board, b).vertices.len(), 2);
}

#[test]
fn cmd_x_in_vector_edit_mode_takes_the_selected_segments() {
    let mut board = board();
    let path = abc(&mut board);
    // A-B selected, and no point: the segment goes, and A with it, which nothing joins now.
    board
        .execute("vector.edit.select", &json!({"segments": [0]}))
        .unwrap();
    assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
    assert!(board.doc().unwrap().node(path).is_some());
    let net = network(&board, path);
    assert_eq!(net.segments.len(), 1, "{net:?}");
    assert_eq!(net.vertices.len(), 2, "{net:?}");
    assert!(board.session.clipboard.nodes.is_empty());
}

#[test]
fn cmd_x_outside_vector_edit_mode_cuts_the_layer_in_the_view() {
    let mut board = board();
    let path = three_points(&mut board, 100);
    select(&mut board, path);
    let doc = board.doc().unwrap();
    // The selected path itself is cut, by the view, with the system clipboard.
    assert_eq!(
        route_key(&mut board, "x", CMD),
        Ok(KeyOutcome::Ui("edit.cut"))
    );
    assert_eq!(board.doc().unwrap(), doc);
}
