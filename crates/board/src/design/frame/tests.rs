//! Constraints, size limits and layout grids.

use serde_json::json;

use super::*;
use crate::design::lists::testing::{design_file, edit, node_set, read, rectangle};

fn frame_with_child(board: &mut crate::BoardDoc) -> (figcraft_doc::NodeId, figcraft_doc::NodeId) {
    let made = board
        .execute(
            "frame.create",
            &json!({"x": 0, "y": 0, "width": 400, "height": 300}),
        )
        .unwrap();
    let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
    let made = board
        .execute(
            "node.create",
            &json!({"type": "rectangle", "x": 10, "y": 10, "width": 50, "height": 50, "parent": frame}),
        )
        .unwrap();
    (frame, figcraft_engine::params::id_of(&made["id"]).unwrap())
}

#[test]
fn a_constraint_is_set_one_axis_at_a_time() {
    let mut board = design_file();
    let (_, child) = frame_with_child(&mut board);
    let sel = read(&board, &[child]);
    assert!(has_constraints(&sel));
    assert_eq!(constraint(&sel, "horizontal").as_deref(), Some("left"));
    edit(&mut board, &[child], |s| {
        set_constraint(s, "horizontal", "leftRight")
    });
    edit(&mut board, &[child], |s| {
        set_constraint(s, "vertical", "bottom")
    });
    let sel = read(&board, &[child]);
    assert_eq!(constraint(&sel, "horizontal").as_deref(), Some("leftRight"));
    assert_eq!(constraint(&sel, "vertical").as_deref(), Some("bottom"));
    assert_eq!(
        set_constraint(&sel, "vertical", "middle").params["ids"],
        json!([])
    );
    // A layer on the page has none.
    let loose = rectangle(&mut board, "Loose", 900.0);
    assert!(!has_constraints(&read(&board, &[loose])));
}

#[test]
fn layout_grids_are_added_changed_and_removed() {
    let mut board = design_file();
    let (frame, _) = frame_with_child(&mut board);
    assert_eq!(grids(&read(&board, &[frame])), Some(vec![]));
    edit(&mut board, &[frame], |s| add_grid(s, "columns"));
    let rows = grids(&read(&board, &[frame])).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "columns");
    edit(&mut board, &[frame], |s| {
        set_grid(s, 0, "count", json!(12.0))
    });
    edit(&mut board, &[frame], |s| {
        set_grid(s, 0, "gutter", json!(24.0))
    });
    edit(&mut board, &[frame], |s| {
        set_grid(s, 0, "visible", json!(false))
    });
    let row = &grids(&read(&board, &[frame])).unwrap()[0];
    assert_eq!((row.count, row.gutter, row.visible), (12.0, 24.0, false));
    assert_eq!(
        set_grid(&read(&board, &[frame]), 0, "count", json!(1.5)).params["ids"],
        json!([])
    );
    edit(&mut board, &[frame], |s| remove_grid(s, 0));
    assert_eq!(grids(&read(&board, &[frame])), Some(vec![]));
    let _ = node_set;
}
