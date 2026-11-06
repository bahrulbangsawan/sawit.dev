//! Dev Mode's Inspect panel.

use super::*;
use crate::design::lists::testing::{apply, design_file, rectangle};

#[test]
fn a_layer_is_inspected_and_coded_and_takes_annotations() {
    let mut board = design_file();
    let r = rectangle(&mut board, "Card", 0.0);
    let doc = board.doc().unwrap();
    let sections = inspect(&doc, r);
    assert!(!sections.is_empty());
    assert!(
        sections
            .iter()
            .any(|s| s.rows.iter().any(|r| r.value.contains("100"))),
        "{sections:?}"
    );
    let css = code(&doc, r, "css");
    assert!(css.contains("100"), "{css}");
    let react = code(&doc, r, "react");
    assert_ne!(react, css);
    assert_eq!(languages().len(), 10);

    apply(&mut board, &add_annotation(r, "Use the brand radius"));
    let doc = board.doc().unwrap();
    assert_eq!(annotations(&doc, r), ["Use the brand radius"]);
    apply(&mut board, &remove_annotation(r, 0));
    assert!(annotations(&board.doc().unwrap(), r).is_empty());
    // A frame at the top of the page takes a status.
    let made = board
        .execute(
            "frame.create",
            &serde_json::json!({"x": 400, "y": 0, "width": 100, "height": 100}),
        )
        .unwrap();
    let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
    apply(&mut board, &set_ready(frame, true));
    assert!(ready(&board.doc().unwrap(), frame));
    apply(&mut board, &set_ready(frame, false));
    assert!(!ready(&board.doc().unwrap(), frame));
}
