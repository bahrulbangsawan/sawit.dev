//! Gradient and image rows: what their editors read, and their edits.

use serde_json::json;

use super::*;
use crate::design::lists::testing::edit;
use crate::design::lists::testing::{
    apply, bind, bound, design_file, node_set, read, rectangle, variables,
};

fn linear(board: &mut crate::BoardDoc) -> figcraft_doc::NodeId {
    let r = rectangle(board, "R", 0.0);
    node_set(
        board,
        &[r],
        json!({"fills": ["#112233", {"type": "linearGradient", "stops": [
            {"position": 1.0, "color": "#0000FF"},
            {"position": 0.0, "color": "#FF000080"},
        ]}]}),
    );
    r
}

#[test]
fn a_gradient_row_reads_its_kind_stops_and_angle() {
    let mut board = design_file();
    let r = linear(&mut board);
    let sel = read(&board, &[r]);
    assert!(gradient(&sel, "fills", 0).is_none(), "a solid");
    let row = gradient(&sel, "fills", 1).unwrap();
    assert_eq!(row.kind, "linear");
    assert_eq!(row.stops.len(), 2);
    assert_eq!(
        (row.stops[0].hex.as_str(), row.stops[0].position),
        ("#FF0000", 0.0)
    );
    assert!((row.stops[0].opacity - 128.0 / 255.0).abs() < 0.01);
    assert_eq!(row.stops[1].hex, "#0000FF");
}

#[test]
fn stops_are_edited_added_and_removed_and_the_other_rows_keep_their_bindings() {
    let mut board = design_file();
    let r = linear(&mut board);
    variables(
        &mut board,
        &[(
            "ink",
            "color",
            json!({"r": 0.5, "g": 0.25, "b": 0.125, "a": 1.0}),
        )],
    );
    bind(&mut board, r, "fills/0/color", "ink");

    let e = set_stop(
        &read(&board, &[r]),
        "fills",
        1,
        1,
        Some(0.75),
        Some("#00FF00"),
        Some(0.5),
    );
    assert_eq!(e.command, "fill.set");
    apply(&mut board, &e);
    let row = gradient(&read(&board, &[r]), "fills", 1).unwrap();
    assert_eq!(
        (row.stops[1].hex.as_str(), row.stops[1].position),
        ("#00FF00", 0.75)
    );
    assert!((row.stops[1].opacity - 0.5).abs() < 0.01);

    edit(&mut board, &[r], |s| add_stop(s, "fills", 1));
    let row = gradient(&read(&board, &[r]), "fills", 1).unwrap();
    assert_eq!(row.stops.len(), 3);
    assert!((row.stops[1].position - 0.375).abs() < 1e-6, "{row:?}");

    edit(&mut board, &[r], |s| remove_stop(s, "fills", 1, 1));
    // Two left: the last removal is refused.
    let refused = remove_stop(&read(&board, &[r]), "fills", 1, 1);
    assert_eq!(refused.params["ids"], json!([]));
    assert_eq!(
        gradient(&read(&board, &[r]), "fills", 1)
            .unwrap()
            .stops
            .len(),
        2,
        "two at least"
    );
    assert_eq!(bound(&board, r), ["fills/0/color"]);
}

#[test]
fn kinds_turn_into_each_other_and_the_angle_turns_the_gradient() {
    let mut board = design_file();
    let r = linear(&mut board);
    // A solid becomes a gradient of its colour, to the same colour transparent.
    edit(&mut board, &[r], |s| set_kind(s, "fills", 0, "radial"));
    let row = gradient(&read(&board, &[r]), "fills", 0).unwrap();
    assert_eq!(row.kind, "radial");
    assert_eq!(row.stops[0].hex, "#112233");
    assert_eq!((row.stops[0].opacity, row.stops[1].opacity), (1.0, 0.0));
    // And back: a solid of its first colour.
    edit(&mut board, &[r], |s| set_kind(s, "fills", 0, "solid"));
    assert!(gradient(&read(&board, &[r]), "fills", 0).is_none());
    // Another kind keeps the stops.
    edit(&mut board, &[r], |s| set_kind(s, "fills", 1, "angular"));
    let row = gradient(&read(&board, &[r]), "fills", 1).unwrap();
    assert_eq!((row.kind.as_str(), row.stops.len()), ("angular", 2));
    edit(&mut board, &[r], |s| set_kind(s, "fills", 1, "linear"));
    for degrees in [0.0, 45.0, 90.0, 270.0] {
        edit(&mut board, &[r], |s| set_angle(s, "fills", 1, degrees));
        let row = gradient(&read(&board, &[r]), "fills", 1).unwrap();
        assert!(
            (row.angle - degrees).abs() < 0.01,
            "{degrees}: {}",
            row.angle
        );
    }
    assert_eq!(
        set_kind(&read(&board, &[r]), "fills", 1, "conic").command,
        "node.set"
    );
}
