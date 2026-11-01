//! The pointer on the canvas: drags, double and triple clicks, and hovering.

use super::*;

#[gpui_kit::test]
fn a_drag_out_of_the_window_ends_as_one_undo_step(cx: &mut TestAppContext) {
    let tmp = Scratch::new("drag-out");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let tool = json!({"op": "command", "command": "tool.rectangle"});
    let reply = run_op(&view, tool, &mut cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let (left, none) = (MouseButton::Left, Modifiers::default());

    cx.simulate_mouse_down(point(px(100.), px(100.)), left, none);
    // The press gave the canvas the keyboard.
    assert!(cx.update(|window, cx| view.read(cx).focus.is_focused(window)));
    cx.simulate_mouse_move(point(px(150.), px(140.)), left, none);
    // Out of the window: only the window's listeners see the last move and the release.
    cx.simulate_mouse_move(point(px(600.), px(500.)), left, none);
    cx.simulate_mouse_up(point(px(600.), px(500.)), left, none);

    let [drawn] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    view.read_with(&cx, |view, _| {
        assert!(!view.dragging);
        let board = view.board().unwrap();
        // The gesture is over: the tool is free, and back to Move once the shape is drawn.
        assert!(!board.session.tool.busy());
        assert_eq!(board.session.tool.tool.id(), "move");
        // From the press to the last move, out of the window: 500 × 400 page units at 100%.
        let doc = board.doc().unwrap();
        let node = doc.node(drawn).unwrap();
        assert_eq!((node.width, node.height), (500.0, 400.0));
        assert_eq!(board.session.undo_depth(), (1, 0));
    });
    let undo = json!({"op": "command", "command": "edit.undo"});
    let reply = run_op(&view, undo, &mut cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(layers(&view, &cx).is_empty());
}

#[gpui_kit::test]
fn a_press_ends_a_drag_whose_release_never_came(cx: &mut TestAppContext) {
    let tmp = Scratch::new("lost-release");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let tool = json!({"op": "command", "command": "tool.rectangle"});
    run_op(&view, tool, &mut cx);
    let (left, none) = (MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_down(point(px(100.), px(100.)), left, none);
    cx.simulate_mouse_move(point(px(150.), px(140.)), left, none);
    // The release went elsewhere, as when the tab is hidden mid-drag. The next press ends
    // that drag as it was drawn, then makes its own gesture.
    cx.simulate_mouse_down(point(px(300.), px(200.)), left, none);
    cx.simulate_mouse_up(point(px(300.), px(200.)), left, none);

    let [drawn] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    view.read_with(&cx, |view, _| {
        let board = view.board().unwrap();
        assert!(!board.session.tool.busy());
        let doc = board.doc().unwrap();
        let node = doc.node(drawn).unwrap();
        assert_eq!((node.width, node.height), (50.0, 40.0));
        assert_eq!(board.session.undo_depth(), (1, 0));
    });
}

/// CANVAS-UI-8: the canvas's tab is hidden mid-drag (⌃Tab with the button held, an agent
/// switching tabs), so nothing hears the drag's release any more. Hiding ends the drag where
/// it was, kept, and the pan held by the middle button: an agent's edit then goes through,
/// where it waited for "the drag on the canvas" to finish until the next press on it.
#[gpui_kit::test]
fn hiding_the_canvas_ends_a_drag_whose_release_it_will_never_hear(cx: &mut TestAppContext) {
    let tmp = Scratch::new("hidden-mid-drag");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let tool = json!({"op": "command", "command": "tool.rectangle"});
    run_op(&view, tool, &mut cx);
    let (left, shift) = (
        MouseButton::Left,
        Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    );
    cx.simulate_mouse_down(point(px(100.), px(100.)), left, Modifiers::default());
    // ⇧ held: a square, as wide as the move is: 50 × 50.
    cx.simulate_mouse_move(point(px(150.), px(140.)), left, shift);
    show(&view, false, &mut cx);
    view.read_with(&cx, |view, _| {
        assert!(!view.dragging);
        assert!(!view.board().unwrap().session.tool.busy());
    });
    let reply = run_op(&view, sticky_op(), &mut cx);
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let [drawn, _] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    view.read_with(&cx, |view, _| {
        let board = view.board().unwrap();
        let node = board.doc().unwrap().node(drawn).cloned().unwrap();
        assert_eq!((node.width, node.height), (50.0, 50.0));
        assert_eq!(board.session.undo_depth(), (2, 0));
    });

    // A pan by the middle button, away from the sticky and its context bar, hidden before
    // its release.
    show(&view, true, &mut cx);
    cx.simulate_mouse_down(point(px(60.), px(100.)), MouseButton::Middle, shift);
    assert!(view.read_with(&cx, |view, _| view.pan.is_some()));
    show(&view, false, &mut cx);
    assert!(view.read_with(&cx, |view, _| view.pan.is_none()));
}

#[gpui_kit::test]
fn a_double_click_types_in_a_sticky(cx: &mut TestAppContext) {
    let tmp = Scratch::new("double-click");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    // A 200 × 200 sticky about page point (0, 0), the middle of the canvas, not selected.
    let sticky = json!({
        "op": "command", "command": "board.sticky", "params": {"x": -100, "y": -100},
    });
    run_op(&view, sticky, &mut cx);
    run_op(
        &view,
        json!({"op": "command", "command": "select.none"}),
        &mut cx,
    );
    let (middle, button, modifiers) = (
        point(px(200.), px(150.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position: middle,
            button,
            modifiers,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: middle,
            button,
            modifiers,
            click_count,
        });
    }

    let [note] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    view.read_with(&cx, |view, _| {
        let ds = view.board().unwrap().session.ds().unwrap();
        let text = figcraft_engine::board::text_of(&ds.doc, note).unwrap();
        assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
    });
}

#[gpui_kit::test]
fn a_triple_click_selects_the_line_being_typed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("triple-click");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    // A 200 × 200 sticky about page point (0, 0), reading one line, not selected.
    let sticky = json!({
        "op": "command", "command": "board.sticky",
        "params": {"x": -100, "y": -100, "text": "one two"},
    });
    run_op(&view, sticky, &mut cx);
    run_op(
        &view,
        json!({"op": "command", "command": "select.none"}),
        &mut cx,
    );
    let [note] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    // The middle of its text, in the window.
    let (text, on_text) = view.read_with(&cx, |view, _| {
        let ds = view.board().unwrap().session.ds().unwrap();
        let text = figcraft_engine::board::text_of(&ds.doc, note).unwrap();
        let b = ds.doc.world_bounds(text).unwrap();
        let (x, y) = ds
            .view
            .page_to_screen((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
        let origin = view.bounds.origin;
        (
            text,
            point(origin.x + px(x as f32), origin.y + px(y as f32)),
        )
    });
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    for click_count in [1, 2, 3] {
        cx.simulate_event(MouseDownEvent {
            position: on_text,
            button,
            modifiers,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: on_text,
            button,
            modifiers,
            click_count,
        });
    }

    // The second click started typing with all of the text selected, and the third selected
    // the line it is on, all of "one two", where a press alone leaves a caret.
    view.read_with(&cx, |view, _| {
        let session = &view.board().unwrap().session;
        let selected = figcraft_engine::textedit::selected_range(session);
        assert_eq!(selected, Some((text, 0, 7)));
    });
}

#[gpui_kit::test]
fn leaving_the_canvas_clears_what_hovering_showed(cx: &mut TestAppContext) {
    let tmp = Scratch::new("leave");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    // What Alt measured from over the canvas, outlined.
    let measure = |cx: &mut TestAppContext| {
        view.update(cx, |view, _| {
            let overlay = &mut view.board_mut().unwrap().session.tool.overlay;
            overlay.measure_box = Some([0.0, 0.0, 10.0, 10.0]);
        })
    };
    let measured = |cx: &TestAppContext| {
        view.read_with(cx, |view, _| {
            view.board().unwrap().session.tool.overlay.measure_box
        })
    };
    let (over, none) = (point(px(200.), px(150.)), Modifiers::default());

    // From over the canvas to off it (it is as wide as the window).
    cx.simulate_mouse_move(over, None, none);
    measure(&mut cx);
    cx.simulate_mouse_move(point(px(600.), px(150.)), None, none);
    assert_eq!(measured(&cx), None);

    // From over the canvas out of the window, which comes with no move off the canvas first.
    cx.simulate_mouse_move(over, None, none);
    measure(&mut cx);
    cx.simulate_event(MouseExitEvent {
        position: point(px(200.), px(-1.)),
        pressed_button: None,
        modifiers: none,
    });
    assert_eq!(measured(&cx), None);
}
