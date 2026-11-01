//! The text being edited: its box, its selected characters, the composition and the caret.

use std::sync::Arc;

use super::*;

/// The carets drawn: each one's top and bottom.
fn carets(shapes: &[Shape]) -> Vec<((f64, f64), (f64, f64))> {
    let caret = |s: &Shape| match s {
        Shape::Caret { top, bottom, .. } => Some((*top, *bottom)),
        _ => None,
    };
    shapes.iter().filter_map(caret).collect()
}

fn fills(shapes: &[Shape]) -> Vec<&Shape> {
    let fill = |s: &&Shape| matches!(s, Shape::Fill { .. });
    shapes.iter().filter(fill).collect()
}

/// The lines drawn that don't close: their points.
fn open_lines(shapes: &[Shape]) -> Vec<&[(f64, f64)]> {
    let mut lines = Vec::new();
    for shape in shapes {
        if let Shape::Outline {
            points,
            closed: false,
            ..
        } = shape
        {
            lines.push(points.as_slice());
        }
    }
    lines
}

/// A text layer reading "hi" with its top-left corner at page point (100, 50) of the
/// fixture's design file, (200, 150) on screen at 100 %, being edited: the caret before
/// character `caret`, or all of it selected.
fn editing_hi(f: &mut Fixture, caret: Option<usize>) -> NodeId {
    // Without fonts nothing is shaped: Sawit's own and the system's are added first, each once
    // per process, so the layer is made the size its letters are and its font stays the same.
    crate::fonts::register();
    figcraft_text::scan_system_fonts();
    let text = make(
        &mut f.board,
        "node.create",
        json!({"type": "text", "x": 100, "y": 50, "text": "hi"}),
    );
    textedit::begin(&mut f.board.session, text, caret, false).unwrap();
    text
}

/// What text layer `id` reads in the document.
fn reads(board: &BoardDoc, id: NodeId) -> String {
    let doc = board.doc().unwrap();
    let text = doc.node(id).and_then(|n| n.kind.text()).unwrap();
    text.characters.clone()
}

#[test]
fn caret_overlay_while_editing() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    zoom(&mut board, 1.0);
    let note = make(
        &mut board,
        "board.sticky",
        json!({"x": 0, "y": 0, "text": "hi"}),
    );
    assert!(carets(&overlay(&board.session)).is_empty());
    // Typing in it (all of "hi" selected): one caret, in the selection colour.
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    let shapes = overlay(&board.session);
    assert_eq!(carets(&shapes).len(), 1, "{shapes:?}");
    let caret_color = |s: &Shape| match s {
        Shape::Caret { color, .. } => Some(*color),
        _ => None,
    };
    let colors: Vec<Rgba> = shapes.iter().filter_map(caret_color).collect();
    assert_eq!(colors, [SELECTION]);
    // The selection makes way: no handles, no size.
    assert!(handles(&shapes).is_empty(), "{shapes:?}");
    assert!(readouts(&shapes).is_empty(), "{shapes:?}");
    // Once the typing ends, the caret goes.
    textedit::end(&mut board.session).unwrap();
    assert!(carets(&overlay(&board.session)).is_empty());
}

#[test]
fn caret_stands_before_its_character_on_screen() {
    let mut f = fixture();
    let text = editing_hi(&mut f, Some(0));
    let shapes = overlay(&f.board.session);
    let [(top, bottom)] = carets(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    // Before the first character: at the text's left side, from its top down one line.
    assert!(near(top, (200.0, 150.0)), "{top:?}");
    assert!((bottom.0 - 200.0).abs() < 1e-6, "{bottom:?}");
    let line = bottom.1 - top.1;
    let (width, height) = {
        let doc = f.board.doc().unwrap();
        let n = doc.node(text).unwrap();
        (n.width, n.height)
    };
    assert!(line > 0.0 && line <= height + 1e-6, "{line} in {height}");
    // The text's box is outlined, and nothing else is.
    let outline = Shape::Outline {
        points: vec![
            (200.0, 150.0),
            (200.0 + width, 150.0),
            (200.0 + width, 150.0 + height),
            (200.0, 150.0 + height),
        ],
        closed: true,
        color: SELECTION,
        width: 1.0,
        dashed: false,
    };
    assert_eq!(outlines(&shapes), [&outline]);
    // After the last character: further right, within the text's box.
    textedit::key(&mut f.board.session, "End", false, false, false).unwrap();
    let [(end, _)] = carets(&overlay(&f.board.session))[..] else {
        panic!()
    };
    assert!(
        end.0 > top.0 + 1.0 && end.0 <= 200.0 + width + 1e-6,
        "{end:?}"
    );
    assert!((end.1 - top.1).abs() < 1e-6, "{end:?}");
    // At twice the zoom: twice as far from the canvas's top-left, and twice as tall.
    textedit::key(&mut f.board.session, "Home", false, false, false).unwrap();
    zoom(&mut f.board, 2.0);
    let [(top, bottom)] = carets(&overlay(&f.board.session))[..] else {
        panic!()
    };
    assert!(near(top, (400.0, 300.0)), "{top:?}");
    assert!((bottom.1 - top.1 - 2.0 * line).abs() < 1e-6, "{bottom:?}");
}

/// A caret at the start of a line stands on the left edge of the text's box, where the box's
/// outline is drawn in the same colour: painted on its line, it would vanish into the
/// outline, as in a new sticky. It is painted as a band from its line into the text, wider
/// than the outline.
#[test]
fn a_caret_at_a_line_start_shows_inside_the_outlined_box() {
    let mut f = fixture();
    editing_hi(&mut f, Some(0));
    let shapes = overlay(&f.board.session);
    let [(top, bottom)] = carets(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    // The caret's line is the box's left edge, which the outline covers.
    let [
        Shape::Outline {
            points,
            closed: true,
            width,
            ..
        },
    ] = outlines(&shapes)[..]
    else {
        panic!("{shapes:?}")
    };
    assert!(near(top, points[0]), "{top:?} {points:?}");
    assert!(
        (bottom.0 - points[0].0).abs() < 1e-6,
        "{bottom:?} {points:?}"
    );
    assert!(CARET_WIDTH > *width);
    // Painted, it is the band from that edge into the box.
    let band = caret_band(top, bottom).unwrap();
    let inside = [
        top,
        (top.0 + CARET_WIDTH, top.1),
        (bottom.0 + CARET_WIDTH, bottom.1),
        bottom,
    ];
    assert!(
        band.iter().zip(inside).all(|(a, b)| near(*a, b)),
        "{band:?}"
    );
}

/// A caret's band runs from its line toward the text's right, however the text is turned. A
/// caret with no length isn't painted.
#[test]
fn a_caret_band_runs_toward_the_text() {
    // Upright: the text's right is the screen's.
    let band = caret_band((10.0, 0.0), (10.0, 20.0));
    assert_eq!(
        band,
        Some([(10.0, 0.0), (12.0, 0.0), (12.0, 20.0), (10.0, 20.0)])
    );
    // A quarter turn clockwise: down its lines is the screen's left, and the text's right is
    // the screen's down.
    let band = caret_band((10.0, 0.0), (-10.0, 0.0));
    assert_eq!(
        band,
        Some([(10.0, 0.0), (10.0, 2.0), (-10.0, 2.0), (-10.0, 0.0)])
    );
    assert_eq!(caret_band((5.0, 5.0), (5.0, 5.0)), None);
}

#[test]
fn selected_text_is_tinted() {
    let mut f = fixture();
    // All of "hi" selected: the caret after it, the selection back to its start.
    editing_hi(&mut f, None);
    let shapes = overlay(&f.board.session);
    let [(top, bottom)] = carets(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    let [Shape::Fill { points, color }] = fills(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    // The selection colour at 30 %, from the line's start to the caret, a line high.
    assert_eq!(*color, [0x0D, 0x99, 0xFF, 77]);
    let tinted = [
        (200.0, top.1),
        (top.0, top.1),
        (top.0, bottom.1),
        (200.0, bottom.1),
    ];
    assert_eq!(points.len(), 4, "{points:?}");
    for (point, corner) in points.iter().zip(tinted) {
        assert!(near(*point, corner), "{points:?} != {tinted:?}");
    }
    // Collapsed to a caret, nothing is tinted.
    textedit::key(&mut f.board.session, "ArrowRight", false, false, false).unwrap();
    let shapes = overlay(&f.board.session);
    assert!(fills(&shapes).is_empty(), "{shapes:?}");
    assert_eq!(carets(&shapes).len(), 1);
}

#[test]
fn composition_is_underlined_where_it_shows() {
    let mut f = fixture();
    let text = editing_hi(&mut f, Some(2));
    let shapes = overlay(&f.board.session);
    let [(before, _)] = carets(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    let narrow = outlines(&shapes)[0].clone();
    textedit::preedit(&mut f.board.session, "ni", None).unwrap();
    let shapes = overlay(&f.board.session);
    // The caret is the input method's, at the composition's end, which shows after "hi"; the
    // composition is underlined from where the caret was to there.
    let [(top, bottom)] = carets(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    assert!(top.0 > before.0 + 1.0, "{top:?} {before:?}");
    let [&[start, end]] = open_lines(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    assert!((start.0 - before.0).abs() < 1e-6, "{start:?} {before:?}");
    assert!((end.0 - top.0).abs() < 1e-6, "{end:?} {top:?}");
    // Level, below the baseline, inside the line.
    assert!((start.1 - end.1).abs() < 1e-9, "{start:?} {end:?}");
    assert!(start.1 > top.1 && start.1 < bottom.1, "{start:?}");
    // Nothing is selected, and the box, drawn before the underline, grew with the
    // composition.
    assert!(fills(&shapes).is_empty(), "{shapes:?}");
    let [wide, _underline] = outlines(&shapes)[..] else {
        panic!("{shapes:?}")
    };
    let right = |s: &Shape| match s {
        Shape::Outline { points, .. } => points[1].0,
        _ => unreachable!(),
    };
    assert!(right(wide) > right(&narrow) + 1.0, "{wide:?} {narrow:?}");
    // The document doesn't have the composition.
    assert_eq!(reads(&f.board, text), "hi");
}

#[test]
fn a_composition_is_made_again_only_when_it_changes() {
    let mut f = fixture();
    let text = editing_hi(&mut f, Some(2));
    let mut kept = None;
    Composition::refresh(&mut kept, &f.board.session);
    assert!(kept.is_none());
    textedit::preedit(&mut f.board.session, "ni", None).unwrap();
    Composition::refresh(&mut kept, &f.board.session);
    let first = kept.as_ref().expect("a composition");
    assert_eq!((first.range, first.cursor), ((2, 4), 4));
    let first = first.doc.clone();
    // Unchanged, it is the same document: the render asked for it stays the one asked for.
    Composition::refresh(&mut kept, &f.board.session);
    assert!(Arc::ptr_eq(&first, &kept.as_ref().unwrap().doc));
    // A longer composition, the input method's cursor in it: made again.
    textedit::preedit(&mut f.board.session, "nih", Some(1)).unwrap();
    Composition::refresh(&mut kept, &f.board.session);
    let grown = kept.as_ref().expect("a composition");
    assert!(!Arc::ptr_eq(&first, &grown.doc));
    assert_eq!((grown.range, grown.cursor), ((2, 5), 3));
    let shown = grown.doc.node(text).and_then(|n| n.kind.text()).unwrap();
    assert_eq!(shown.characters, "hinih");
    // Committed, there is none, and the document has the text.
    textedit::insert_ime_commit(&mut f.board.session, "你").unwrap();
    Composition::refresh(&mut kept, &f.board.session);
    assert!(kept.is_none());
    assert_eq!(reads(&f.board, text), "hi你");
}
