//! What gestures under way draw: the marquee, snap guides, drops, and the tools' previews.

use super::*;

/// The fixture's frame, outlined on screen as a closed 1 px line in `color`.
fn frame_outline(color: Rgba) -> Shape {
    Shape::Outline {
        points: vec![
            (100.0, 100.0),
            (300.0, 100.0),
            (300.0, 250.0),
            (100.0, 250.0),
        ],
        closed: true,
        color,
        width: 1.0,
        dashed: false,
    }
}

#[test]
fn marquee_draws_fill_and_outline() {
    let mut f = fixture();
    // Pressed on empty canvas and dragged, not let go.
    pointer(&mut f.board, PointerKind::Down, 500.0, 300.0);
    pointer(&mut f.board, PointerKind::Move, 600.0, 400.0);
    let shapes = overlay(&f.board.session);
    let fills: Vec<&Shape> = shapes
        .iter()
        .filter(|s| matches!(s, Shape::Fill { .. }))
        .collect();
    let corners = vec![
        (600.0, 400.0),
        (700.0, 400.0),
        (700.0, 500.0),
        (600.0, 500.0),
    ];
    let tint = [0x0D, 0x99, 0xFF, 26];
    assert_eq!(
        fills,
        [&Shape::Fill {
            points: corners.clone(),
            color: tint,
        }]
    );
    let outline = Shape::Outline {
        points: corners,
        closed: true,
        color: SELECTION,
        width: 1.0,
        dashed: false,
    };
    assert_eq!(outlines(&shapes), [&outline]);
}

#[test]
fn guides_draw_lines_crosses_and_distances() {
    use figcraft_engine::snap::Guide;
    let mut f = fixture();
    let line = |x0, y0, x1, y1| Guide::new(Point::new(x0, y0), Point::new(x1, y1));
    f.board.session.tool.overlay.guides = vec![
        Guide {
            label: Some("24".into()),
            ..line(0.0, 200.0, 100.0, 200.0)
        },
        Guide {
            dashed: true,
            ..line(0.0, 0.0, 0.0, 50.0)
        },
        line(10.0, 10.0, 10.0, 10.0),
    ];
    let shapes = overlay(&f.board.session);
    let guide = |points: Vec<(f64, f64)>, width, dashed| Shape::Outline {
        points,
        closed: false,
        color: GUIDE,
        width,
        dashed,
    };
    let expected = [
        guide(vec![(100.0, 300.0), (200.0, 300.0)], 1.0, false),
        guide(vec![(100.0, 100.0), (100.0, 150.0)], 1.0, true),
        // A point snapped onto: a cross.
        guide(vec![(106.0, 106.0), (114.0, 114.0)], 1.5, false),
        guide(vec![(106.0, 114.0), (114.0, 106.0)], 1.5, false),
        // The distance over the middle of its line, after every line.
        Shape::Label {
            at: (150.0, 300.0),
            anchor: Anchor::Middle,
            text: "24".into(),
            color: WHITE,
            background: Some(GUIDE),
            node: None,
        },
    ];
    // After the frame's name.
    assert_eq!(shapes[1..], expected);
}

#[test]
fn ink_previews_its_stroke() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    zoom(&mut board, 2.0);
    board.session.set_tool(ToolId::Marker);
    pointer(&mut board, PointerKind::Down, 0.0, 0.0);
    pointer(&mut board, PointerKind::Move, 50.0, 10.0);
    pointer(&mut board, PointerKind::Move, 100.0, 0.0);
    let shapes = overlay(&board.session);
    let [r, g, b, a] = board.session.tool.board.marker.to_rgba8();
    assert_eq!(
        shapes,
        [Shape::Outline {
            points: vec![(200.0, 200.0), (300.0, 220.0), (400.0, 200.0)],
            closed: false,
            color: [r, g, b, a],
            width: 8.0,
            dashed: false,
        }]
    );
}

/// Item 9: where a drag drops.
#[test]
fn a_drag_into_a_frame_outlines_it() {
    let mut f = fixture();
    select(&mut f.board, &[f.rect]);
    assert!(!overlay(&f.board.session).contains(&frame_outline(SELECTION)));
    // The rectangle, pressed in its middle and dragged over the frame's middle, held.
    pointer(&mut f.board, PointerKind::Down, 350.0, 40.0);
    pointer(&mut f.board, PointerKind::Move, 250.0, 60.0);
    pointer(&mut f.board, PointerKind::Move, 100.0, 75.0);
    assert_eq!(f.board.session.tool.overlay.drop_target, Some(f.frame));
    let shapes = overlay(&f.board.session);
    assert!(shapes.contains(&frame_outline(SELECTION)), "{shapes:?}");
    pointer(&mut f.board, PointerKind::Up, 100.0, 75.0);
    assert!(!overlay(&f.board.session).contains(&frame_outline(SELECTION)));
}

/// Item 9: where a drag goes in an auto layout, a 2 px line, last.
#[test]
fn an_insert_line_is_drawn_2_px_wide() {
    let mut f = fixture();
    f.board.session.tool.overlay.insert_line = Some([0.0, 10.0, 0.0, 90.0]);
    let shapes = overlay(&f.board.session);
    let line = Shape::Outline {
        points: vec![(100.0, 110.0), (100.0, 190.0)],
        closed: false,
        color: SELECTION,
        width: 2.0,
        dashed: false,
    };
    assert_eq!(shapes.last(), Some(&line), "{shapes:?}");
}

/// Item 10: the Pen's points, its handles and the segment a click would add.
#[test]
fn the_pen_shows_its_points_handles_and_next_segment() {
    let mut f = fixture();
    f.board.session.set_tool(ToolId::Pen);
    // Its first point at (500, 300) on the page, (600, 400) on screen; the pointer moved on
    // to (700, 450).
    pointer(&mut f.board, PointerKind::Down, 500.0, 300.0);
    pointer(&mut f.board, PointerKind::Up, 500.0, 300.0);
    pointer(&mut f.board, PointerKind::Hover, 600.0, 350.0);
    let shapes = overlay(&f.board.session);
    // After the frame's name: the point, white, then the next segment to the pointer.
    let first = Shape::Handle {
        center: (600.0, 400.0),
        size: 7.0,
        round: true,
        fill: WHITE,
        stroke: SELECTION,
    };
    assert_eq!(shapes[1], first, "{shapes:?}");
    assert!(
        has_line(&shapes, (600.0, 400.0), (700.0, 450.0), 1.0),
        "{shapes:?}"
    );
    // A second point pressed at (700, 300) and dragged right pulls out its handles: each a
    // line from the point to a white dot.
    pointer(&mut f.board, PointerKind::Down, 700.0, 300.0);
    pointer(&mut f.board, PointerKind::Move, 750.0, 300.0);
    let handles = f.board.session.tool.overlay.pen_handles.clone();
    assert!(!handles.is_empty());
    let shapes = overlay(&f.board.session);
    for [x0, y0, x1, y1] in handles {
        let (point, end) = ((x0 + 100.0, y0 + 100.0), (x1 + 100.0, y1 + 100.0));
        assert!(has_line(&shapes, point, end, 1.0), "{point:?} {end:?}");
        let dot = |s: &Shape| matches!(s, Shape::Handle { center, size: 7.0, round: true, fill: WHITE, .. } if near(*center, end));
        assert!(shapes.iter().any(dot), "{end:?}");
    }
}

/// Item 10: a connector being drawn.
#[test]
fn a_connector_being_drawn_shows_its_line() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    zoom(&mut board, 1.0);
    make(&mut board, "board.sticky", json!({"x": -100, "y": -100}));
    make(&mut board, "board.sticky", json!({"x": 300, "y": -100}));
    select(&mut board, &[]);
    board.session.set_tool(ToolId::Connector);
    // From the first sticky's middle toward the second, held.
    pointer(&mut board, PointerKind::Down, 0.0, 0.0);
    pointer(&mut board, PointerKind::Move, 200.0, 0.0);
    pointer(&mut board, PointerKind::Move, 400.0, 0.0);
    let [x0, y0, x1, y1] = board
        .session
        .tool
        .overlay
        .rubber
        .expect("a connector drawn");
    assert_eq!((x1, y1), (400.0, 0.0));
    let shapes = overlay(&board.session);
    let (from, to) = ((x0 + 100.0, y0 + 100.0), (500.0, 100.0));
    assert!(has_line(&shapes, from, to, 1.5), "{shapes:?}");
}

/// Item 10: the eraser's reach, while it erases.
#[test]
fn the_eraser_shows_its_reach() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    zoom(&mut board, 2.0);
    board.session.set_tool(ToolId::Eraser);
    pointer(&mut board, PointerKind::Down, 0.0, 0.0);
    pointer(&mut board, PointerKind::Move, 10.0, 0.0);
    let [x, y, r] = board
        .session
        .tool
        .overlay
        .brush
        .expect("the eraser's reach");
    let shapes = overlay(&board.session);
    // Two rings around the pointer, its reach on the page across at twice the size: white,
    // with the selection colour over it.
    let center = (2.0 * (x + 100.0), 2.0 * (y + 100.0));
    let mut styles = Vec::new();
    for shape in &shapes {
        let Shape::Outline {
            points,
            closed: true,
            color,
            width,
            ..
        } = shape
        else {
            continue;
        };
        styles.push((*color, *width));
        assert!(points.len() > 8, "{points:?}");
        for p in points {
            let from_center = (p.0 - center.0).hypot(p.1 - center.1);
            assert!((from_center - 2.0 * r).abs() < 0.5, "{p:?}");
        }
    }
    assert_eq!(styles, [(WHITE, 3.0), (SELECTION, 1.5)], "{shapes:?}");
}
