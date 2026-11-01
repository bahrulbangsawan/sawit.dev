//! The names of frames and sections, and the labels' boxes.

use figcraft_doc::NodeKind;
use figcraft_geom::kurbo;

use super::*;

#[test]
fn label_quad_round_trips_through_view() {
    let mut f = fixture();
    zoom(&mut f.board, 2.0);
    let view = &f.board.session.ds().unwrap().view;
    let quad = label_quad((100.0, 50.0), (60.0, 14.0), view);
    // At 200 % the page quad is half the size of the box on screen.
    assert_eq!((quad[1].x - quad[0].x, quad[3].y - quad[0].y), (30.0, 7.0));
    let back = quad.map(|p| view.page_to_screen(p.x, p.y));
    assert_eq!(
        back,
        [(100.0, 50.0), (160.0, 50.0), (160.0, 64.0), (100.0, 64.0)]
    );
}

#[test]
fn label_box_places_names_and_readouts() {
    let name = |background| Shape::Label {
        at: (100.0, 50.0),
        anchor: Anchor::TopLeft,
        text: "Frame".into(),
        color: SELECTION,
        background,
        node: Some(NodeId(7)),
    };
    // A frame's name starts at `at`, one line tall; a section's is on a pill there.
    assert_eq!(
        name(None).label_box(40.0),
        Some(((100.0, 50.0), (40.0, LABEL_LINE)))
    );
    assert_eq!(
        name(Some(SELECTION)).label_box(40.0),
        Some(((100.0, 50.0), (56.0, LABEL_LINE + 8.0)))
    );
    // A distance's pill is centred on `at`.
    let readout = |anchor| Shape::Label {
        at: (100.0, 50.0),
        anchor,
        text: "24".into(),
        color: WHITE,
        background: Some(GUIDE),
        node: None,
    };
    let height = LABEL_LINE + 4.0;
    assert_eq!(
        readout(Anchor::Middle).label_box(40.0),
        Some(((76.0, 50.0 - height / 2.0), (48.0, height)))
    );
    // A dragged handle's starts at `at`, beside the handle.
    assert_eq!(
        readout(Anchor::TopLeft).label_box(40.0),
        Some(((100.0, 50.0), (48.0, height)))
    );
    let caret = Shape::Caret {
        top: (0.0, 0.0),
        bottom: (0.0, 10.0),
        color: SELECTION,
    };
    assert_eq!(caret.label_box(40.0), None);
}

#[test]
fn names_have_their_frames_width() {
    let mut f = fixture();
    let room = |board: &BoardDoc, id| {
        let ds = board.session.ds().unwrap();
        name_room(&ds.doc, id, &ds.view).unwrap()
    };
    let narrow = make(
        &mut f.board,
        "node.create",
        json!({"type": "frame", "x": 0, "y": 300, "width": 5, "height": 5}),
    );
    let section = make(
        &mut f.board,
        "node.create",
        json!({"type": "section", "x": 0, "y": 400, "width": 100, "height": 100}),
    );
    // A frame's name: the frame's width on screen.
    assert_eq!(room(&f.board, f.frame), 200.0);
    zoom(&mut f.board, 2.0);
    assert_eq!(room(&f.board, f.frame), 400.0);
    // However narrow the frame, room for an ellipsis.
    assert_eq!(room(&f.board, narrow), NAME_MIN);
    // A section's pill keeps its padding inside the section, and holds 480 px of name at most.
    assert_eq!(room(&f.board, section), 200.0 - 16.0);
    zoom(&mut f.board, 10.0);
    assert_eq!(room(&f.board, section), SECTION_NAME_MAX);
    // The name of the 200 × 150 frame turned a quarter stays upright: the width of the
    // frame's box on screen, 150 px, not its top edge's 200.
    zoom(&mut f.board, 1.0);
    let doc = f.board.session.doc_mut().unwrap();
    doc.get_mut(f.frame).unwrap().transform = kurbo::Affine::rotate(std::f64::consts::FRAC_PI_2);
    let turned = room(&f.board, f.frame);
    assert!((turned - 150.0).abs() < 1e-9, "{turned}");
}

#[test]
fn frame_names_read_on_their_page() {
    let mut f = fixture();
    let color = |board: &BoardDoc| match &overlay(&board.session)[0] {
        Shape::Label { color, .. } => *color,
        other => panic!("{other:?}"),
    };
    assert_eq!(color(&f.board), ON_LIGHT.frame);
    hover(&mut f.board, f.frame);
    assert_eq!(color(&f.board), SELECTION);
    f.board.session.ds_mut().unwrap().hover = None;
    let page = f.board.session.page().unwrap();
    let doc = f.board.session.doc_mut().unwrap();
    if let NodeKind::Page { background, .. } = &mut doc.get_mut(page).unwrap().kind {
        *background = figcraft_color::Color::DARK_PAGE;
    }
    assert_eq!(color(&f.board), ON_DARK.frame);
    // A section's name is on a pill inside its top-left corner.
    let section = make(
        &mut f.board,
        "node.create",
        json!({
            "type": "section", "name": "Ideas", "x": 0, "y": 400, "width": 300, "height": 200,
        }),
    );
    select(&mut f.board, &[]);
    let shapes = overlay(&f.board.session);
    let pill = Shape::Label {
        at: (100.0, 500.0),
        anchor: Anchor::TopLeft,
        text: "Ideas".into(),
        color: ON_DARK.section,
        background: Some(ON_DARK.pill),
        node: Some(section),
    };
    assert!(shapes.contains(&pill), "{shapes:?}");
}
