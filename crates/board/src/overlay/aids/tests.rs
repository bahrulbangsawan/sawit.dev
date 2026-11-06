//! The View menu's aids.

use figcraft_engine::Session;
use serde_json::json;

use super::*;

fn session() -> Session {
    let mut s = Session::with_document(figcraft_doc::Document::new("Design"));
    s.execute(
        "frame.create",
        &json!({"x": 0, "y": 0, "width": 400, "height": 300}),
    )
    .unwrap();
    s
}

fn drawn(s: &Session) -> Vec<Shape> {
    let mut out = Vec::new();
    aids(&mut out, s.ds().unwrap());
    out
}

#[test]
fn a_frames_columns_are_drawn_while_layout_grids_are_on() {
    let mut s = session();
    let none = drawn(&s).len();
    s.execute(
        "grid.add",
        &json!({"grid": {"kind": "columns", "count": 4}}),
    )
    .unwrap();
    let with = drawn(&s);
    let bands = with
        .iter()
        .filter(|s| matches!(s, Shape::Fill { .. }))
        .count();
    assert_eq!(bands, 4, "{with:?}");
    assert_eq!(with.len(), none + 4);
    s.execute("view.layoutGrids", &json!({"on": false}))
        .unwrap();
    assert_eq!(drawn(&s).len(), none);
}

#[test]
fn rulers_frame_outlines_and_the_pixel_grid_follow_their_toggles() {
    let mut s = session();
    let before = drawn(&s).len();
    s.execute("view.rulers", &json!({"on": true})).unwrap();
    let rulers = drawn(&s);
    assert!(
        rulers.iter().any(|s| matches!(s, Shape::Label { .. })),
        "numbered"
    );
    s.execute("view.rulers", &json!({"on": false})).unwrap();
    s.execute("view.frameOutlines", &json!({"on": true}))
        .unwrap();
    assert_eq!(drawn(&s).len(), before + 1, "the frame's outline");
    assert_eq!(tick_step(1.0), 100.0);
    assert_eq!(tick_step(10.0), 10.0);
}
