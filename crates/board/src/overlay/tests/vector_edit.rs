//! Vector edit mode's segments, points and Bézier handles.

use figcraft_engine::vectoredit::{self, VHit};

use super::*;

/// A round handle drawn at `at` (but for rounding): its size across and its fill.
fn round_at(shapes: &[Shape], at: (f64, f64)) -> Vec<(f64, Rgba)> {
    let round = |s: &Shape| match s {
        Shape::Handle {
            center,
            size,
            round: true,
            fill,
            ..
        } if near(*center, at) => Some((*size, *fill)),
        _ => None,
    };
    shapes.iter().filter_map(round).collect()
}

/// Item 11: vector edit mode, in place of the selection's box.
#[test]
fn vector_edit_mode_shows_points_and_segments_and_the_selection_steps_aside() {
    let mut f = fixture();
    select(&mut f.board, &[f.rect]);
    f.board
        .execute("vector.edit.begin", &json!({"id": f.rect}))
        .unwrap();
    let shapes = overlay(&f.board.session);
    // No box, no corner handles, no size, no corner radius handles.
    assert!(box_handles(&shapes).is_empty(), "{shapes:?}");
    assert!(readouts(&shapes).is_empty(), "{shapes:?}");
    // Its 4 segments, 1.5 px, and a white point 7 px across on each of its corners.
    let segments = outlines(&shapes)
        .into_iter()
        .filter(|s| {
            matches!(
                s,
                Shape::Outline {
                    width: 1.5,
                    closed: false,
                    ..
                }
            )
        })
        .count();
    assert_eq!(segments, 4, "{shapes:?}");
    let corners = [
        (400.0, 100.0),
        (500.0, 100.0),
        (500.0, 180.0),
        (400.0, 180.0),
    ];
    for corner in corners {
        assert_eq!(round_at(&shapes, corner), [(7.0, WHITE)], "{corner:?}");
    }
    assert_eq!(round_handles(&shapes).len(), 4);
    // A point selected is filled, one under the pointer is larger.
    f.board
        .execute("vector.edit.select", &json!({"vertices": [0]}))
        .unwrap();
    f.board.session.tool.overlay.vector_hover = Some(VHit::Vertex { index: 1 });
    let ve = f.board.session.ds().unwrap().vector_edit.clone().unwrap();
    let o = vectoredit::overlay(&f.board.doc().unwrap(), &ve).unwrap();
    let screen_of = |[x, y]: [f64; 2]| (x + 100.0, y + 100.0);
    let shapes = overlay(&f.board.session);
    assert_eq!(
        round_at(&shapes, screen_of(o.vertices[0])),
        [(7.0, SELECTION)]
    );
    assert_eq!(round_at(&shapes, screen_of(o.vertices[1])), [(9.0, WHITE)]);
}

/// Item 11: a curve's Bézier handles in vector edit mode, each a line from its point to a dot.
#[test]
fn vector_edit_mode_shows_bezier_handles() {
    let mut f = fixture();
    let ellipse = make(
        &mut f.board,
        "node.create",
        json!({"type": "ellipse", "x": 0, "y": 300, "width": 100, "height": 100}),
    );
    f.board
        .execute("vector.edit.begin", &json!({"id": ellipse}))
        .unwrap();
    f.board
        .execute("vector.edit.select", &json!({"vertices": [0]}))
        .unwrap();
    let ve = f.board.session.ds().unwrap().vector_edit.clone().unwrap();
    let o = vectoredit::overlay(&f.board.doc().unwrap(), &ve).unwrap();
    assert!(!o.handles.is_empty());
    let shapes = overlay(&f.board.session);
    for h in &o.handles {
        let (vertex, end) = (
            (h.vertex[0] + 100.0, h.vertex[1] + 100.0),
            (h.handle[0] + 100.0, h.handle[1] + 100.0),
        );
        assert!(has_line(&shapes, vertex, end, 1.0), "{vertex:?} {end:?}");
        assert!(!round_at(&shapes, end).is_empty(), "{end:?}");
    }
}
