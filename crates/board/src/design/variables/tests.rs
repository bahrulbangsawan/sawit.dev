//! The Variables table and its edits.

use serde_json::json;

use super::*;
use crate::design::lists::testing::{apply, design_file, rectangle};

#[test]
fn a_collection_gets_modes_and_variables_whose_values_differ_by_mode() {
    let mut board = design_file();
    apply(&mut board, &create_collection("Theme"));
    let doc = board.doc().unwrap();
    let theme = table(&doc).remove(0);
    assert_eq!(theme.name, "Theme");
    assert_eq!(theme.modes.len(), 1);
    apply(&mut board, &add_mode(&theme.id, "Dark"));
    apply(&mut board, &create_variable(&theme.id, "surface", "color"));
    apply(&mut board, &create_variable(&theme.id, "radius", "number"));
    let doc = board.doc().unwrap();
    let theme = table(&doc).remove(0);
    assert_eq!(
        theme.modes.iter().map(|m| m.1.as_str()).collect::<Vec<_>>(),
        ["Mode 1", "Dark"]
    );
    let surface = theme
        .variables
        .iter()
        .find(|v| v.name == "surface")
        .unwrap()
        .clone();
    assert_eq!(surface.kind, "color");
    let dark = theme.modes[1].0.clone();
    apply(
        &mut board,
        &set_value(&surface.id, "color", &dark, "#111111").unwrap(),
    );
    apply(
        &mut board,
        &set_value(&surface.id, "color", &theme.modes[0].0, "ffffff80").unwrap(),
    );
    assert!(set_value(&surface.id, "color", &dark, "blue").is_none());
    let radius = theme
        .variables
        .iter()
        .find(|v| v.name == "radius")
        .unwrap()
        .clone();
    apply(
        &mut board,
        &set_value(&radius.id, "number", &dark, "12").unwrap(),
    );
    let doc = board.doc().unwrap();
    let theme = table(&doc).remove(0);
    let surface = theme
        .variables
        .iter()
        .find(|v| v.name == "surface")
        .unwrap();
    assert_eq!(surface.values, ["#FFFFFF80", "#111111"]);
    let radius_row = theme.variables.iter().find(|v| v.name == "radius").unwrap();
    assert_eq!(radius_row.values[1], "12");

    apply(&mut board, &rename_variable(&radius.id, "corner"));
    apply(&mut board, &rename_mode(&theme.id, &dark, "Night"));
    let doc = board.doc().unwrap();
    let theme = table(&doc).remove(0);
    assert!(theme.variables.iter().any(|v| v.name == "corner"));
    assert_eq!(theme.modes[1].1, "Night");
    assert_eq!(
        fresh_name("Mode", &["Mode".into(), "Mode 2".into()]),
        "Mode 3"
    );
}

#[test]
fn a_layer_binds_to_a_variable_and_a_frame_takes_a_mode() {
    let mut board = design_file();
    apply(&mut board, &create_collection("Theme"));
    let doc = board.doc().unwrap();
    let theme = table(&doc).remove(0);
    apply(&mut board, &add_mode(&theme.id, "Dark"));
    apply(&mut board, &create_variable(&theme.id, "ink", "color"));
    let r = rectangle(&mut board, "R", 0.0);
    let doc = board.doc().unwrap();
    let choices = bindable(&doc, "color");
    assert_eq!(choices.len(), 1);
    assert!(choices[0].1.ends_with("/ink"));
    apply(
        &mut board,
        &bind(&[r], "fills/0/color", Some(&choices[0].0)),
    );
    assert_eq!(
        bound_to(&board.doc().unwrap(), r, "fills/0/color"),
        Some(choices[0].0.clone())
    );
    apply(&mut board, &bind(&[r], "fills/0/color", None));
    assert_eq!(bound_to(&board.doc().unwrap(), r, "fills/0/color"), None);

    let made = board
        .execute(
            "frame.create",
            &json!({"x": 0, "y": 300, "width": 100, "height": 100}),
        )
        .unwrap();
    let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
    let dark = table(&board.doc().unwrap())[0].modes[1].0.clone();
    apply(&mut board, &set_mode(&[frame], &theme.id, Some(&dark)));
    assert_eq!(mode_of(&board.doc().unwrap(), frame, &theme.id), Some(dark));
    apply(&mut board, &set_mode(&[frame], &theme.id, None));
    assert_eq!(mode_of(&board.doc().unwrap(), frame, &theme.id), None);
}
