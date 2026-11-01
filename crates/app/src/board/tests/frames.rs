//! The frames the render thread draws: painted, replaced and let go of, only of the page on show;
//! and the frames' names painted over them.

use super::*;

/// Whether `image` is in the sprite atlas of `window`: painted, and not released since.
fn in_atlas(cx: &mut TestAppContext, window: WindowHandle<BoardView>, image: &RenderImage) -> bool {
    cx.update_window(window.into(), |_, window, _| {
        window.has_image_atlas_entry(image)
    })
    .unwrap()
}

#[gpui_kit::test]
fn frames_are_painted_and_replaced_ones_leave_the_atlas(cx: &mut TestAppContext) {
    let tmp = Scratch::new("frames");
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    let first = next_frame(cx, &view, None);
    assert!(in_atlas(cx, window, &first));
    // The engine has the canvas's size, the window's less the top bar, which the first frame
    // fitted the view to.
    let state = view_state(&view, cx);
    let canvas_height = 300.0 - f64::from(top_bar::HEIGHT);
    assert_eq!((state.vw, state.vh), (400.0, canvas_height));
    assert!(view.read_with(cx, |view, _| view.fitted));

    // A pan asks for another frame. Painting it takes the first one out of the atlas.
    let pan = ScrollEvent {
        delta: ScrollDelta::Pixels(point(px(50.), px(0.))),
        ..ScrollEvent::default()
    };
    view.update(cx, |view, cx| view.on_scroll(&pan, cx));
    let second = next_frame(cx, &view, Some(&first));
    assert!(in_atlas(cx, window, &second));
    assert!(!in_atlas(cx, window, &first));
}

/// CANVAS-UI-9: a canvas whose tab is hidden lets go of what it keeps to draw its next frames:
/// its frame's texture leaves the window's sprite atlas (and the render thread forgets the
/// render it kept, `worker::tests`). It keeps the frame's image itself, so that the canvas
/// shows at once when its tab comes back: that paint puts the texture back.
#[gpui_kit::test]
fn a_hidden_canvas_lets_go_of_its_frames_texture(cx: &mut TestAppContext) {
    let tmp = Scratch::new("hidden-frames");
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    let first = next_frame(cx, &view, None);
    assert!(in_atlas(cx, window, &first));
    let show = |shown: bool, cx: &mut TestAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            let home = cx.focus_handle();
            view.update(cx, |view, cx| view.set_shown(shown, &home, window, cx));
        })
        .unwrap();
    };
    // The test window draws the canvas whether it is "shown" or not: nothing is drawn between
    // the hide and the checks.
    show(false, cx);
    assert!(!in_atlas(cx, window, &first));
    let kept = view.read_with(cx, |view, _| {
        view.frame.as_ref().map(|frame| frame.image.clone())
    });
    assert!(kept.is_some_and(|kept| Arc::ptr_eq(&kept, &first)));
    show(true, cx);
    cx.run_until_parked();
    assert!(in_atlas(cx, window, &first));
}

/// The frame image the canvas's next paint would put on screen, as its prepaint works it out
/// at the bounds of its last paint.
fn painted_image(
    cx: &mut TestAppContext,
    window: WindowHandle<BoardView>,
    view: &Entity<BoardView>,
) -> Option<Arc<RenderImage>> {
    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |view, cx| {
            let bounds = view.bounds;
            let painting = view.prepaint(bounds, window, cx)?;
            painting.frame.map(|(image, _)| image)
        })
    })
    .unwrap()
}

#[gpui_kit::test]
fn only_a_frame_of_the_page_on_show_is_painted(cx: &mut TestAppContext) {
    let tmp = Scratch::new("pages");
    let path = tmp.0.join("design.sawit");
    // A design file of two pages: a frame on the first, nothing on the second.
    let mut session = figcraft_engine::Session::new();
    let frame =
        json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100});
    session.execute("node.create", &frame).unwrap();
    let added = session.execute("page.add", &json!({})).unwrap();
    let second = NodeId(added["id"].as_u64().unwrap());
    session
        .execute("page.select", &json!({"index": 0}))
        .unwrap();
    session
        .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
        .unwrap();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (window, view) = open_design_in_window(cx, &path);
    let first = next_frame(cx, &view, None);
    let (uid, first_page) = view.read_with(cx, |view, _| {
        let ds = view.board().unwrap().session.ds().unwrap();
        (ds.uid, ds.page)
    });
    assert_ne!(first_page, second);
    let painted = painted_image(cx, window, &view);
    assert!(painted.is_some_and(|image| Arc::ptr_eq(&image, &first)));

    // The second page goes on show, as an agent's `page.select` or `replace` shows one. Until
    // its own frame comes, the first page's is not painted over it: the canvas shows the
    // page's colour alone.
    let select = json!({"op": "command", "command": "page.select", "params": {"id": second.0}});
    let reply = run_op(&view, select, cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let painted = painted_image(cx, window, &view);
    assert!(
        painted.is_none_or(|image| !Arc::ptr_eq(&image, &first)),
        "the first page's frame was painted over the second page"
    );

    // The second page's own frame comes, and is painted.
    let second_frame = next_frame(cx, &view, Some(&first));
    view.read_with(cx, |view, _| {
        assert!(view.frame.as_ref().unwrap().shows(uid, second));
    });
    assert!(in_atlas(cx, window, &second_frame));

    // A render of the first page that comes late (asked for before the page changed) is
    // dropped: the second page keeps its frame.
    let late = super::super::worker::Frame {
        view: figcraft_render::View {
            origin: (0.0, 0.0),
            scale: 1.0,
            width: 1,
            height: 1,
        },
        image: first.clone(),
        key: 0,
        uid,
        page: first_page,
        work: figcraft_render::Work::Full,
    };
    view.update(cx, |view, cx| view.show_frame(late, cx));
    view.read_with(cx, |view, _| {
        let shown = &view.frame.as_ref().unwrap().image;
        assert!(Arc::ptr_eq(shown, &second_frame));
    });
}

/// The page colour of the first page of `doc`.
fn first_page_rgb(doc: &Document) -> [u8; 3] {
    sawit_board::view::page_rgb(doc, doc.first_page().unwrap())
}

/// A blank file another app made (FigCraft's new design file, saved) opens in the dark theme
/// with its own page colour, as FigCraft opens a file from disk, and that is no edit to save.
/// CANVAS-CORE-11: it took the dark page (FigCraft gives one only to a new file it makes, and
/// counted the canvas's, which keeps no path, as new), and the first edit saved that into the
/// user's file.
#[gpui_kit::test]
fn opening_in_a_dark_theme_is_no_edit(cx: &mut TestAppContext) {
    let tmp = Scratch::new("dark");
    let path = tmp.0.join("design.sawit");
    let mut session = figcraft_engine::Session::new();
    session
        .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
    });
    let (_, view) = open_design_in_window(cx, &path);
    next_frame(cx, &view, None);
    let [r, g, b, _] = Color::DEFAULT_PAGE.to_rgba8();
    let page = view.read_with(cx, |view, _| {
        first_page_rgb(&view.board().unwrap().doc().unwrap())
    });
    assert_eq!(page, [r, g, b], "the file's page took the dark colour");
    // No edit to save: neither the frames after it nor a flush write the file.
    cx.executor().advance_clock(SAVE_DELAY * 2);
    cx.run_until_parked();
    view.update(cx, |view, _| view.flush());
    assert!(std::fs::read(&path).unwrap() == bytes, "the file was saved");
    assert_eq!(names(&tmp.0), ["design.sawit"]);
    // An edit saves the file with its own page colour.
    let rectangle = json!({"type": "rectangle"});
    let made = json!({"op": "command", "command": "node.create", "params": rectangle});
    let reply = run_op(&view, made, cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    view.update(cx, |view, _| view.flush());
    let saved = store::open(&path).unwrap().doc().unwrap();
    assert_eq!(first_page_rgb(&saved), [r, g, b], "the dark page was saved");
}

#[gpui_kit::test]
fn an_op_redraws_the_canvas(cx: &mut TestAppContext) {
    let tmp = Scratch::new("op-redraw");
    let view = open(cx, &tmp.0.join("canvas.sawit"));
    let redraws = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let redraws = redraws.clone();
        cx.observe(&view, move |_, _| redraws.set(redraws.get() + 1))
            .detach();
    });
    // A command that only moves the view changes nothing in the document, but the canvas
    // has to show it.
    let zoom = |cx: &TestAppContext| view_state(&view, cx).zoom;
    let before = zoom(cx);
    run_op(
        &view,
        json!({"op": "command", "command": "view.zoomIn"}),
        cx,
    );
    assert!(zoom(cx) > before);
    assert!(redraws.get() > 0, "the canvas wasn't told to redraw");
}

#[gpui_kit::test]
fn painted_frame_names_go_back_to_the_engine(cx: &mut TestAppContext) {
    let tmp = Scratch::new("names");
    let path = tmp.0.join("design.sawit");
    let mut session = figcraft_engine::Session::new();
    let frame =
        json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100});
    let made = session.execute("node.create", &frame).unwrap();
    let frame = NodeId(made["id"].as_u64().unwrap());
    session
        .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
        .unwrap();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (_, view) = open_design_in_window(cx, &path);
    // Selected, it has its outline, handles and size painted over it too.
    view.update(cx, |view, cx| {
        let board = view.board_mut().unwrap();
        board.session.set_selection(vec![frame]);
        cx.notify();
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let board = view.board().unwrap();
        let labels = &board.session.tool.labels;
        let named: Vec<NodeId> = labels.iter().map(|(id, _)| *id).collect();
        assert_eq!(named, [frame]);
        // Its name is just above it on the page, where a press is one on the frame.
        let quad = labels[0].1;
        assert!(quad.iter().all(|p| p.y < 0.0), "{quad:?}");
        let centre = PagePoint::new(
            quad.iter().map(|p| p.x).sum::<f64>() / 4.0,
            quad.iter().map(|p| p.y).sum::<f64>() / 4.0,
        );
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let pressed = figcraft_engine::tools::label_at(&doc, page, labels, centre);
        assert_eq!(pressed, Some(frame));
    });
}

#[gpui_kit::test]
fn long_names_are_cut_to_their_frames(cx: &mut TestAppContext) {
    let tmp = Scratch::new("long-names");
    let path = tmp.0.join("design.sawit");
    // A frame and a section, each named far wider than it is on screen.
    let long = "Checkout – payment method – error state, ".repeat(8);
    let mut session = figcraft_engine::Session::new();
    let mut make = |layer: serde_json::Value| {
        let made = session.execute("node.create", &layer).unwrap();
        NodeId(made["id"].as_u64().unwrap())
    };
    let frame = make(json!({
        "type": "frame", "name": long, "x": 0, "y": 0, "width": 100, "height": 60,
    }));
    let section = make(json!({
        "type": "section", "name": long, "x": 0, "y": 100, "width": 200, "height": 100,
    }));
    session
        .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
        .unwrap();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (_, view) = open_design_in_window(cx, &path);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let board = view.board().unwrap();
        let doc = board.doc().unwrap();
        let page = doc.first_page().unwrap();
        let labels = &board.session.tool.labels;
        let named: Vec<NodeId> = labels.iter().map(|(id, _)| *id).collect();
        assert_eq!(named, [frame, section]);
        for (id, quad) in labels {
            let n = doc.node(*id).unwrap();
            let world = doc.world_transform(*id);
            let (left, right) = (
                world * PagePoint::ZERO,
                world * PagePoint::new(n.width, 0.0),
            );
            // Cut with an ellipsis: from the frame's left side to no further than its right.
            assert!((quad[0].x - left.x).abs() < 1e-3, "{quad:?}");
            assert!(quad[1].x <= right.x + 1e-3, "{quad:?} past {right:?}");
            assert!(quad[1].x - quad[0].x > n.width / 2.0, "{quad:?}");
            // A press past the frame's right side, where the whole name would have run, is
            // not one on the name.
            let past = PagePoint::new(right.x + 10.0, (quad[0].y + quad[3].y) / 2.0);
            let pressed = figcraft_engine::tools::label_at(&doc, page, labels, past);
            assert_eq!(pressed, None);
        }
    });
}
