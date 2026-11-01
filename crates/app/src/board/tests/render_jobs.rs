//! The render the canvas asks for: drawn as the View menu has it, and with an input method's
//! composition in place of the document.

use super::super::frames::{job_key, render_job};
use super::*;

/// A design file in a session of its own, with a 100 × 50 rectangle at the page origin.
fn design_with_a_rectangle() -> (figcraft_engine::Session, NodeId) {
    let mut session = figcraft_engine::Session::new();
    let rectangle = json!({"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50});
    let made = session.execute("node.create", &rectangle).unwrap();
    (session, NodeId(made["id"].as_u64().unwrap()))
}

#[test]
fn the_view_menus_render_toggles_reach_the_render() {
    let (mut session, rectangle) = design_with_a_rectangle();
    let job = |session: &figcraft_engine::Session| {
        let wanted = figcraft_render::View {
            origin: (0.0, 0.0),
            scale: 2.0,
            width: 100,
            height: 100,
        };
        render_job(session.ds().unwrap(), None, DocKind::Design, wanted)
    };
    let key = |session: &figcraft_engine::Session| job_key(&job(session), 1.0, false);
    let run = |session: &mut figcraft_engine::Session, id: &str, params: Value| {
        session.execute(id, &params).unwrap();
    };

    let plain = job(&session);
    let ds = session.ds().unwrap();
    assert_eq!((plain.uid, plain.page), (ds.uid, ds.page));
    assert!(!plain.opts.outline_mode && plain.opts.outlined.is_empty());
    assert_eq!(plain.opts.outline, figcraft_render::Outline::default());
    assert!(!plain.proof);
    assert_eq!(plain.pixels, None);
    let plain_key = key(&session);

    // Outlines ▸ Show outlines.
    run(&mut session, "view.outlines", json!({}));
    assert!(job(&session).opts.outline_mode);
    assert_ne!(key(&session), plain_key);
    run(&mut session, "view.outlines", json!({}));
    assert_eq!(key(&session), plain_key);

    // Show outlines for selection: the layers selected, and so none while none is.
    run(&mut session, "view.outlinesSelection", json!({}));
    session.set_selection(vec![rectangle]);
    assert_eq!(job(&session).opts.outlined, [rectangle]);
    assert_ne!(key(&session), plain_key);
    session.set_selection(Vec::new());
    assert!(job(&session).opts.outlined.is_empty());
    assert_eq!(key(&session), plain_key);

    // Include hidden layers and Include object bounds.
    run(&mut session, "view.outlinesHiddenLayers", json!({}));
    let hidden_key = key(&session);
    assert!(job(&session).opts.outline.hidden);
    run(&mut session, "view.outlinesObjectBounds", json!({}));
    assert!(job(&session).opts.outline.bounds);
    assert!(![plain_key, hidden_key].contains(&key(&session)));

    // Soft-proof sRGB: nothing to mark in an sRGB file, and the stripes in a Display P3 one.
    run(&mut session, "view.proofSrgb", json!({}));
    assert!(!job(&session).proof);
    let profile = json!({"profile": "displayP3"});
    run(&mut session, "file.colorProfile", profile);
    assert!(job(&session).proof);
    let proofed_key = key(&session);
    run(&mut session, "view.proofSrgb", json!({}));
    assert!(!job(&session).proof);
    assert_ne!(key(&session), proofed_key);

    // Pixel preview: at its resolution once zoomed in past it, and not before.
    session.ds_mut().unwrap().view.zoom = 4.0;
    let unpreviewed_key = key(&session);
    run(&mut session, "view.pixelPreview", json!({}));
    assert_eq!(job(&session).pixels, Some(1.0));
    assert_ne!(key(&session), unpreviewed_key);
    run(&mut session, "view.pixelPreview", json!({"scale": 2}));
    assert_eq!(job(&session).pixels, Some(2.0));
    session.ds_mut().unwrap().view.zoom = 2.0;
    assert_eq!(job(&session).pixels, None);
}

#[test]
fn a_composition_is_rendered_in_place_of_the_document() {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let made = board
        .execute("board.sticky", &json!({"x": 0, "y": 0}))
        .unwrap();
    let note = figcraft_engine::params::id_of(&made["id"]).unwrap();
    assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
    figcraft_engine::textedit::preedit(&mut board.session, "ni", None).unwrap();
    let composition = Composition::of(&board.session).expect("a composition");
    let wanted = || figcraft_render::View {
        origin: (0.0, 0.0),
        scale: 2.0,
        width: 100,
        height: 100,
    };
    let ds = board.session.ds().unwrap();
    let composed = render_job(ds, Some(&composition), DocKind::Board, wanted());
    let plain = render_job(ds, None, DocKind::Board, wanted());
    // The sticky renders with the composition in it, which the document doesn't have.
    assert!(Arc::ptr_eq(&composed.doc, &composition.doc));
    assert!(Arc::ptr_eq(&plain.doc, &ds.doc));
    assert_eq!((composed.uid, composed.page), (plain.uid, plain.page));
    assert_ne!(job_key(&composed, 1.0, false), job_key(&plain, 1.0, false));
}

/// The colour (BGRA) of the frame shown at page point `at`.
fn frame_pixel(view: &Entity<BoardView>, cx: &TestAppContext, at: (f64, f64)) -> [u8; 4] {
    view.read_with(cx, |view, _| {
        let frame = view.frame.as_ref().unwrap();
        let pixel = |page: f64, origin: f64| ((page - origin) * frame.view.scale) as usize;
        let (x, y) = (
            pixel(at.0, frame.view.origin.0),
            pixel(at.1, frame.view.origin.1),
        );
        let start = (y * frame.view.width as usize + x) * 4;
        let bytes = frame.image.as_bytes(0).unwrap();
        bytes[start..start + 4].try_into().unwrap()
    })
}

#[gpui_kit::test]
fn show_outlines_redraws_the_page_as_outlines(cx: &mut TestAppContext) {
    let tmp = Scratch::new("outlines");
    let path = tmp.0.join("design.sawit");
    let (mut session, _) = design_with_a_rectangle();
    session
        .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
        .unwrap();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (_, view) = open_design_in_window(cx, &path);
    let filled = next_frame(cx, &view, None);
    let (middle, outside) = ((50.0, 25.0), (-50.0, -50.0));
    let page = frame_pixel(&view, cx, outside);
    assert_ne!(frame_pixel(&view, cx, middle), page, "no rectangle");

    // ⇧⌘O's command: the canvas draws again, and the rectangle is its outline alone.
    let outlines = json!({"op": "command", "command": "view.outlines"});
    let reply = run_op(&view, outlines.clone(), cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let outlined = next_frame(cx, &view, Some(&filled));
    assert_eq!(frame_pixel(&view, cx, middle), page);

    // And back.
    run_op(&view, outlines, cx);
    next_frame(cx, &view, Some(&outlined));
    assert_ne!(frame_pixel(&view, cx, middle), page);
}
