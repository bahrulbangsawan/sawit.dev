//! The selection's handles that the Move tool answers: the box's corners, an ellipse's arc handles
//! and a shape's corner radius handles, and the readout of the one dragged.

use super::*;

/// Whether `handles` are `expected` (where, and how big), but for rounding.
fn same_handles(handles: &[((f64, f64), f64)], expected: &[((f64, f64), f64)]) -> bool {
    handles.len() == expected.len()
        && (handles.iter().zip(expected))
            .all(|((a, size), (b, wanted))| near(*a, *b) && size == wanted)
}

/// A board at 100 %, the page point (-100, -100) at the canvas's top-left, with a 200 × 200
/// sticky from there selected: from (0, 0) to (200, 200) on screen.
fn selected_sticky() -> (BoardDoc, NodeId) {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    zoom(&mut board, 1.0);
    let note = make(&mut board, "board.sticky", json!({"x": -100, "y": -100}));
    select(&mut board, &[note]);
    (board, note)
}

/// How far in from a corner along its diagonal a corner radius handle is at least, on each
/// axis: 10 px along the diagonal.
fn radius_inset() -> f64 {
    10.0 / 2f64.sqrt()
}

/// The readouts of dragged handles: their text, and where their box starts.
fn beside(shapes: &[Shape]) -> Vec<(&str, (f64, f64))> {
    let mut readouts = Vec::new();
    for shape in shapes {
        if let Shape::Label {
            at,
            anchor: Anchor::TopLeft,
            text,
            color: WHITE,
            background: Some(_),
            node: None,
        } = shape
        {
            readouts.push((text.as_str(), *at));
        }
    }
    readouts
}

/// As FigCraft draws them (ui-egui overlays.rs `shape_overlay`): the Move tool answers a press
/// on these handles, before the box's, by rounding the corners.
#[test]
fn a_selected_sticky_shows_its_corner_radius_handles() {
    let (board, _) = selected_sticky();
    let shapes = overlay(&board.session);
    // The box's 4 corner handles, then a round one 10 px in from each corner along its
    // diagonal: a sticky's corners are rounded 2 px, less than that.
    let corners = [(0.0, 0.0), (200.0, 0.0), (200.0, 200.0), (0.0, 200.0)];
    assert_eq!(box_handles(&shapes), corners);
    let i = radius_inset();
    let radius = [
        (i, i),
        (200.0 - i, i),
        (200.0 - i, 200.0 - i),
        (i, 200.0 - i),
    ];
    let expected = radius.map(|center| (center, 7.0));
    let drawn = round_handles(&shapes);
    assert!(same_handles(&drawn, &expected), "{drawn:?}");
}

/// As FigCraft draws it (ui-egui overlays.rs `arc_overlay`): the Move tool answers a press on
/// it, before the box's handles, by opening the ellipse into an arc.
#[test]
fn a_selected_ellipse_shows_its_sweep_handle() {
    let mut f = fixture();
    // From (0, 300) on the page, 200 × 100: its right edge's middle is at (300, 450) on
    // screen.
    let ellipse = make(
        &mut f.board,
        "node.create",
        json!({"type": "ellipse", "x": 0, "y": 300, "width": 200, "height": 100}),
    );
    select(&mut f.board, &[ellipse]);
    let shapes = overlay(&f.board.session);
    // A whole ellipse: only its sweep handle, 10 px in from its right edge, and no corner
    // radius handles.
    let drawn = round_handles(&shapes);
    assert!(same_handles(&drawn, &[((290.0, 450.0), 8.0)]), "{drawn:?}");
    assert_eq!(box_handles(&shapes).len(), 4);
}

#[test]
fn a_dragged_corner_radius_handle_reads_out_the_radius_beside_it() {
    let (mut board, note) = selected_sticky();
    let i = radius_inset();
    // Under the pointer, the top-left corner's handle is drawn larger.
    pointer(&mut board, PointerKind::Hover, -100.0 + i, -100.0 + i);
    let drawn = round_handles(&overlay(&board.session));
    assert!(near(drawn[0].0, (i, i)) && drawn[0].1 == 9.0, "{drawn:?}");
    // A press there takes the handle, not the sticky, and a drag along the corner's diagonal
    // to 20 px in rounds every corner 20 px.
    pointer(&mut board, PointerKind::Down, -100.0 + i, -100.0 + i);
    pointer(&mut board, PointerKind::Move, -80.0, -80.0);
    let shapes = overlay(&board.session);
    let [(text, at)] = beside(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    // Its value, 12 px right of and below the handle, which now stands 20 px in on each axis.
    assert_eq!(text, "Radius 20");
    assert!(near(at, (32.0, 32.0)), "{at:?}");
    let drawn = round_handles(&shapes);
    assert!(
        near(drawn[0].0, (20.0, 20.0)) && drawn[0].1 == 9.0,
        "{drawn:?}"
    );
    // The size still reads under the box; the release ends the readout, as one undo step.
    assert!(labels(&shapes).contains(&("200 × 200", None)), "{shapes:?}");
    pointer(&mut board, PointerKind::Up, -80.0, -80.0);
    assert!(beside(&overlay(&board.session)).is_empty());
    let doc = board.doc().unwrap();
    let radii = doc.node(note).unwrap().corners.radii;
    assert!(radii.iter().all(|r| (r - 20.0).abs() < 1e-6), "{radii:?}");
}

#[test]
fn an_arc_shows_its_start_and_ratio_handles_and_reads_out_the_drag() {
    let mut f = fixture();
    let ellipse = make(
        &mut f.board,
        "node.create",
        json!({"type": "ellipse", "x": 0, "y": 300, "width": 200, "height": 100}),
    );
    // A half: from the right edge round the bottom to the left edge.
    f.board
        .execute(
            "node.set",
            &json!({"id": ellipse, "props": {"arcSweep": 180}}),
        )
        .unwrap();
    select(&mut f.board, &[ellipse]);
    let shapes = overlay(&f.board.session);
    // Ratio in the middle (no ring), Start 10 px in from the right edge with a dot inside, and
    // Sweep 10 px in from the left edge.
    let drawn = round_handles(&shapes);
    let expected = [
        ((200.0, 450.0), 8.0),
        ((290.0, 450.0), 8.0),
        ((110.0, 450.0), 8.0),
    ];
    assert!(same_handles(&drawn, &expected), "{drawn:?}");
    let dots: Vec<&Shape> = shapes
        .iter()
        .filter(|s| matches!(s, Shape::Handle { size: 3.0, .. }))
        .collect();
    let [
        &Shape::Handle {
            center,
            round: true,
            fill: SELECTION,
            stroke: SELECTION,
            ..
        },
    ] = dots[..]
    else {
        panic!("{shapes:?}")
    };
    assert!(near(center, (290.0, 450.0)), "{center:?}");
    // Dragging Sweep reads out its share of a full turn beside it.
    pointer(&mut f.board, PointerKind::Down, 10.0, 350.0);
    pointer(&mut f.board, PointerKind::Move, 100.0, 300.0);
    let hint = f
        .board
        .session
        .tool
        .overlay
        .arc
        .clone()
        .expect("an arc drag");
    assert!(hint.label.starts_with("Sweep "), "{hint:?}");
    let shapes = overlay(&f.board.session);
    let [(text, at)] = beside(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    let handle = (hint.at[0] + 100.0, hint.at[1] + 100.0);
    assert_eq!(text, hint.label);
    assert!(near(at, (handle.0 + 12.0, handle.1 + 12.0)), "{at:?}");
    // The handle dragged is drawn larger.
    let dragged = round_handles(&shapes);
    assert!(
        dragged
            .iter()
            .any(|(c, size)| near(*c, handle) && *size == 10.0),
        "{dragged:?}"
    );
}

#[test]
fn handles_the_move_tool_doesnt_answer_arent_drawn() {
    let (mut board, note) = selected_sticky();
    assert_eq!(round_handles(&overlay(&board.session)).len(), 4);
    // Another tool.
    board.session.set_tool(ToolId::Rectangle);
    assert!(round_handles(&overlay(&board.session)).is_empty());
    board.session.set_tool(ToolId::Move);
    // While measuring (Alt), as the box's handles.
    board.session.tool.overlay.measure_box = Some([0.0, 0.0, 10.0, 10.0]);
    assert!(round_handles(&overlay(&board.session)).is_empty());
    board.session.tool.overlay.measure_box = None;
    // Two layers selected.
    let other = make(&mut board, "board.sticky", json!({"x": 300, "y": -100}));
    select(&mut board, &[note, other]);
    assert!(round_handles(&overlay(&board.session)).is_empty());
    // A layer under 48 px on screen.
    select(&mut board, &[note]);
    zoom(&mut board, 0.2);
    assert!(round_handles(&overlay(&board.session)).is_empty());
    zoom(&mut board, 1.0);
    // A locked layer.
    board
        .session
        .doc_mut()
        .unwrap()
        .get_mut(note)
        .unwrap()
        .locked = true;
    assert!(round_handles(&overlay(&board.session)).is_empty());
}
