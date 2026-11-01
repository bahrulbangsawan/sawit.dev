//! The selection and the layer under the pointer: outlines, the selection's box, handles and size.

use super::*;

fn outlines_of(board: &BoardDoc) -> usize {
    outlines(&overlay(&board.session)).len()
}

#[test]
fn nothing_selected_draws_only_frame_labels() {
    let f = fixture();
    let shapes = overlay(&f.board.session);
    assert_eq!(labels(&shapes), [("Frame", Some(f.frame))]);
    assert_eq!(shapes.len(), 1, "{shapes:?}");
    // Its line ends 4 px above the frame's top-left corner.
    let Shape::Label { at, .. } = &shapes[0] else {
        unreachable!()
    };
    assert_eq!(*at, (100.0, 100.0 - 4.0 - LABEL_LINE));
}

#[test]
fn one_selected_layer_has_box_and_four_handles() {
    let mut f = fixture();
    select(&mut f.board, &[f.rect]);
    let shapes = overlay(&f.board.session);
    let corners = [
        (400.0, 100.0),
        (500.0, 100.0),
        (500.0, 180.0),
        (400.0, 180.0),
    ];
    // Its own outline is the box: no second one around it.
    assert_eq!(
        outlines(&shapes),
        [&Shape::Outline {
            points: corners.to_vec(),
            closed: true,
            color: SELECTION,
            width: 1.0,
            dashed: false,
        }]
    );
    // Its 4 corner handles; the round ones inside them round its corners.
    assert_eq!(box_handles(&shapes), corners);
    assert_eq!(round_handles(&shapes).len(), 4);
    // Twice the zoom, twice as far from the canvas's top-left and from one another.
    zoom(&mut f.board, 2.0);
    let shapes = overlay(&f.board.session);
    let doubled = corners.map(|(x, y)| (2.0 * x, 2.0 * y));
    assert_eq!(box_handles(&shapes), doubled);
}

#[test]
fn size_label_reads_w_by_h() {
    let mut f = fixture();
    select(&mut f.board, &[f.rect]);
    let shapes = overlay(&f.board.session);
    let size = readouts(&shapes);
    assert_eq!(size.len(), 1, "{shapes:?}");
    let Shape::Label {
        text, background, ..
    } = size[0]
    else {
        unreachable!()
    };
    assert_eq!((text.as_str(), *background), ("100 × 80", Some(SELECTION)));
    // Centred under the box, 9 px below it, whatever the text's width.
    let (top_left, (width, _)) = size[0].label_box(40.0).unwrap();
    assert_eq!((top_left.0 + width / 2.0, top_left.1), (450.0, 189.0));
    // In whole pixels.
    let odd = make(
        &mut f.board,
        "node.create",
        json!({"type": "rectangle", "x": 0, "y": 300, "width": 99.6, "height": 80.4}),
    );
    select(&mut f.board, &[odd]);
    let shapes = overlay(&f.board.session);
    assert!(labels(&shapes).contains(&("100 × 80", None)), "{shapes:?}");
}

#[test]
fn hover_outline_only_when_not_selected() {
    let mut f = fixture();
    select(&mut f.board, &[f.rect]);
    hover(&mut f.board, f.rect);
    assert_eq!(outlines(&overlay(&f.board.session)).len(), 1);
    hover(&mut f.board, f.frame);
    let shapes = overlay(&f.board.session);
    let outlines = outlines(&shapes);
    assert_eq!(outlines.len(), 2, "{shapes:?}");
    // The frame's box, 1.5 px, under the selection.
    let frame = vec![
        (100.0, 100.0),
        (300.0, 100.0),
        (300.0, 250.0),
        (100.0, 250.0),
    ];
    assert_eq!(
        outlines[0],
        &Shape::Outline {
            points: frame,
            closed: true,
            color: SELECTION,
            width: 1.5,
            dashed: false,
        }
    );
    // Only the Move and Scale tools show it.
    f.board.session.set_tool(ToolId::Rectangle);
    hover(&mut f.board, f.frame);
    assert_eq!(outlines_of(&f.board), 1);
}

#[test]
fn connector_has_round_end_handles() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let a = make(&mut board, "board.shape", json!({"x": 0, "y": 0}));
    let b = make(&mut board, "board.shape", json!({"x": 400, "y": 0}));
    let line = make(&mut board, "board.connector", json!({"from": a, "to": b}));
    select(&mut board, &[line]);
    zoom(&mut board, 1.0);
    let shapes = overlay(&board.session);
    let doc = board.doc().unwrap();
    let (_, ends) = figcraft_engine::board::selected_connector_ends(&doc, &[line]).unwrap();
    let ends = ends.map(|p| ((p.x + 100.0, p.y + 100.0), true));
    assert_eq!(handles(&shapes), ends);
    // Its ends are all it has: no box, no size.
    assert!(readouts(&shapes).is_empty(), "{shapes:?}");
}

#[test]
fn several_layers_share_one_outlined_box() {
    let mut f = fixture();
    select(&mut f.board, &[f.frame, f.rect]);
    let shapes = overlay(&f.board.session);
    // The frame's and the rectangle's own outlines, then the box around both.
    let union = [
        (100.0, 100.0),
        (500.0, 100.0),
        (500.0, 250.0),
        (100.0, 250.0),
    ];
    let outlines = outlines(&shapes);
    assert_eq!(outlines.len(), 3, "{shapes:?}");
    assert_eq!(
        outlines[2],
        &Shape::Outline {
            points: union.to_vec(),
            closed: true,
            color: SELECTION,
            width: 1.0,
            dashed: false,
        }
    );
    assert_eq!(handles(&shapes), union.map(|c| (c, false)));
    assert!(labels(&shapes).contains(&("400 × 150", None)), "{shapes:?}");
}

#[test]
fn locked_and_small_layers_get_no_handles() {
    let mut f = fixture();
    // Locked: selected in the layers panel, but not resized on the canvas.
    f.board
        .session
        .doc_mut()
        .unwrap()
        .get_mut(f.rect)
        .unwrap()
        .locked = true;
    select(&mut f.board, &[f.rect]);
    let shapes = overlay(&f.board.session);
    assert!(handles(&shapes).is_empty(), "{shapes:?}");
    assert_eq!(outlines(&shapes).len(), 1);
    // Smaller on screen than its handles (both sides under 16 px): its outline only.
    let small = make(
        &mut f.board,
        "node.create",
        json!({"type": "rectangle", "x": 0, "y": 300, "width": 10, "height": 10}),
    );
    select(&mut f.board, &[small]);
    assert!(handles(&overlay(&f.board.session)).is_empty());
    zoom(&mut f.board, 2.0);
    assert_eq!(handles(&overlay(&f.board.session)).len(), 4);
}

/// As built and ratified (delegate ruling on the report's Decision 3): a selection with a
/// component in it is drawn in the component colour throughout.
#[test]
fn a_component_colours_the_whole_selection() {
    let mut f = fixture();
    let component = make(
        &mut f.board,
        "node.create",
        json!({"type": "component", "x": 0, "y": 300, "width": 100, "height": 100}),
    );
    select(&mut f.board, &[f.rect, component]);
    let shapes = overlay(&f.board.session);
    let mut drawn = (0, 0, 0);
    for shape in &shapes {
        match shape {
            Shape::Outline { color, .. } => {
                assert_eq!(*color, COMPONENT);
                drawn.0 += 1;
            }
            Shape::Handle { stroke, .. } => {
                assert_eq!(*stroke, COMPONENT);
                drawn.1 += 1;
            }
            Shape::Label {
                node: None,
                background,
                ..
            } => {
                assert_eq!(*background, Some(COMPONENT));
                drawn.2 += 1;
            }
            _ => {}
        }
    }
    // Both outlines and the box, 4 handles, the size.
    assert_eq!(drawn, (3, 4, 1), "{shapes:?}");
}

/// As built and ratified (delegate ruling on the report's Decision 4): one selected line reads
/// its length, on its 2 end handles.
#[test]
fn a_line_reads_its_length() {
    let mut f = fixture();
    let line = make(
        &mut f.board,
        "node.create",
        json!({"type": "line", "x": 0, "y": 300, "width": 100}),
    );
    select(&mut f.board, &[line]);
    let shapes = overlay(&f.board.session);
    assert_eq!(readouts(&shapes).len(), 1, "{shapes:?}");
    assert!(labels(&shapes).contains(&("100", None)), "{shapes:?}");
    assert_eq!(
        handles(&shapes),
        [(100.0, 400.0), (200.0, 400.0)].map(|c| (c, false))
    );
}
