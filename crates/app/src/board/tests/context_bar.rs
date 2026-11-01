//! The context bar floating over a board's selection.

use super::*;

#[gpui_kit::test]
fn the_context_bar_runs_the_options_it_shows(cx: &mut TestAppContext) {
    use sawit_board::context_bar::{Control, context_bar};

    let tmp = Scratch::new("context-options");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    // A rectangle with text, selected.
    let shape = json!({
        "op": "command", "command": "board.shape",
        "params": {"shape": "rectangle", "x": -80, "y": 0},
    });
    run_op(&view, shape, &mut cx);
    cx.run_until_parked();
    let controls = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| {
            context_bar(&view.board().unwrap().session).map(|bar| bar.controls)
        })
    };
    let undo_steps = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| view.board().unwrap().session.undo_depth().0)
    };
    let toast = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.toast.clone());
    let steps = undo_steps(&cx);
    // Its colours, shape and text size fill the bar, inside its border and padding: the bar is
    // as wide as the place it is given in the canvas takes it to be.
    let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
    let first = cx
        .debug_bounds("board-context-color-yellow")
        .expect("a colour");
    let last = cx
        .debug_bounds("board-context-size")
        .expect("the text size");
    assert_eq!(first.left(), bar.left() + px(7.));
    assert_eq!(last.right(), bar.right() - px(7.));

    // A swatch sets the selection's colour.
    click(&mut cx, "board-context-color-blue");
    cx.run_until_parked();
    let colours = controls(&cx).and_then(|controls| controls.into_iter().next());
    assert!(
        matches!(
            colours,
            Some(Control::Colors {
                current: Some("blue"),
                ..
            })
        ),
        "{colours:?}"
    );
    assert_eq!(undo_steps(&cx), steps + 1);

    // The shape it has already, picked from its menu, does nothing: no command the engine
    // would refuse, and no toast (ruling P3-C2).
    click(&mut cx, "board-context-shape");
    cx.run_until_parked();
    click(&mut cx, "board-context-shape-Rectangle");
    cx.run_until_parked();
    assert_eq!(toast(&cx), None);
    assert_eq!(undo_steps(&cx), steps + 1);
    // Another one changes it.
    click(&mut cx, "board-context-shape");
    cx.run_until_parked();
    click(&mut cx, "board-context-shape-Ellipse");
    cx.run_until_parked();
    assert_eq!(toast(&cx), None);
    let shape = controls(&cx).and_then(|controls| {
        controls.into_iter().find_map(|control| match control {
            Control::Shape { current } => current,
            _ => None,
        })
    });
    assert_eq!(shape, Some(figcraft_doc::BoardShape::Ellipse));
    assert_eq!(undo_steps(&cx), steps + 2);

    // A connector's bar, the widest, fits its controls the same way.
    let connector = json!({
        "op": "command", "command": "board.connector",
        "params": {"from": {"x": -200, "y": 300}, "to": {"x": 200, "y": 300}},
    });
    run_op(&view, connector, &mut cx);
    cx.run_until_parked();
    let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
    let first = cx
        .debug_bounds("board-context-color-yellow")
        .expect("a colour");
    let last = cx.debug_bounds("board-context-end").expect("the end cap");
    assert!(cx.debug_bounds("board-context-routing").is_some());
    assert_eq!(first.left(), bar.left() + px(7.));
    assert_eq!(last.right(), bar.right() - px(7.));
}

#[gpui_kit::test]
fn the_context_bar_floats_over_the_selection_inside_the_canvas(cx: &mut TestAppContext) {
    let tmp = Scratch::new("context-bar");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    assert!(cx.debug_bounds("board-context-bar").is_none());
    // A sticky that an op makes is selected. Its top is on page point (0, 0).
    let sticky = json!({
        "op": "command", "command": "board.sticky", "params": {"x": -100, "y": 0},
    });
    let made = run_op(&view, sticky, &mut cx);
    cx.run_until_parked();
    let canvas = view.read_with(&cx, |view, _| view.bounds);
    // Where page point (x, y) is in the window.
    let on_screen = |cx: &VisualTestContext, (x, y): (f64, f64)| {
        let (x, y) = view_state(&view, cx).page_to_screen(x, y);
        point(
            canvas.origin.x + px(x as f32),
            canvas.origin.y + px(y as f32),
        )
    };
    let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
    // Its bottom 12 px over the sticky's top, its middle over the sticky's.
    let top = on_screen(&cx, (0.0, 0.0));
    assert_eq!(bar.bottom(), top.y - px(12.));
    assert_eq!(bar.center().x, top.x);

    // With the sticky's top 20 px under the canvas's, the bar stays in the canvas, 8 px from
    // its edge.
    let pan = json!({
        "op": "command", "command": "view.set",
        "params": {"cx": 0, "cy": f64::from(canvas.size.height) / 2.0 - 20.0},
    });
    run_op(&view, pan, &mut cx);
    cx.run_until_parked();
    assert_eq!(on_screen(&cx, (0.0, 0.0)).y, canvas.top() + px(20.));
    let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
    assert_eq!(bar.top(), canvas.top() + px(8.));
    assert!(bar.left() >= canvas.left() + px(8.) && bar.right() <= canvas.right() - px(8.));

    // A drag hides it until the drag ends.
    let (left, none) = (MouseButton::Left, Modifiers::default());
    let middle = on_screen(&cx, (0.0, 100.0));
    cx.simulate_mouse_down(middle, left, none);
    cx.simulate_mouse_move(middle + point(px(20.), px(10.)), left, none);
    assert!(cx.debug_bounds("board-context-bar").is_none());
    cx.simulate_mouse_up(middle + point(px(20.), px(10.)), left, none);
    assert!(cx.debug_bounds("board-context-bar").is_some());

    // So does typing in the sticky.
    type_in(&view, &made, &mut cx);
    cx.run_until_parked();
    assert!(cx.debug_bounds("board-context-bar").is_none());
}

#[gpui_kit::test]
fn the_context_bar_stays_over_the_toolbar(cx: &mut TestAppContext) {
    let tmp = Scratch::new("context-bar-toolbar");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    // A sticky that an op makes is selected. Its top is on page point (0, 0).
    let sticky = json!({
        "op": "command", "command": "board.sticky", "params": {"x": -100, "y": 0},
    });
    run_op(&view, sticky, &mut cx);
    // The view moves until the sticky is below the canvas, where the bar would follow it to
    // the canvas's bottom edge, under the toolbar.
    let canvas = view.read_with(&cx, |view, _| view.bounds);
    let pan = json!({
        "op": "command", "command": "view.set",
        "params": {"cx": 0, "cy": -f64::from(canvas.size.height)},
    });
    run_op(&view, pan, &mut cx);
    cx.run_until_parked();
    let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
    let toolbar = cx.debug_bounds("board-toolbar").expect("the toolbar");
    // It stays in sight, 8 px over the toolbar, as the toast does.
    assert_eq!(bar.bottom(), toolbar.top() - px(8.));
    assert_eq!(bar.center().x, toolbar.center().x);
}

#[gpui_kit::test]
fn the_context_bar_scrolls_inside_a_narrow_canvas(cx: &mut TestAppContext) {
    use sawit_board::context_bar::{Control, context_bar};

    let tmp = Scratch::new("context-bar-narrow");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let drawn = |cx: &mut VisualTestContext, selector: &'static str| {
        let drawn = cx.debug_bounds(selector);
        drawn.unwrap_or_else(|| panic!("{selector} isn't drawn"))
    };
    let resize = |cx: &mut VisualTestContext, width: f32| {
        cx.simulate_resize(size(px(width), px(600.)));
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        view.read_with(&*cx, |view, _| view.bounds)
    };
    // Its whole width shows: the first and the last controls just inside its border and
    // padding.
    let whole = |cx: &mut VisualTestContext| {
        let bar = drawn(cx, "board-context-bar");
        let first = drawn(cx, "board-context-color-yellow");
        let last = drawn(cx, "board-context-end");
        assert_eq!(first.left(), bar.left() + px(7.));
        assert_eq!(last.right(), bar.right() - px(7.));
        bar
    };
    // A connector across the middle of the canvas, selected. Its bar is the widest: its
    // colours, how it runs and both its caps.
    let connector = json!({
        "op": "command", "command": "board.connector",
        "params": {"from": {"x": -100, "y": 0}, "to": {"x": 100, "y": 0}},
    });
    run_op(&view, connector, &mut cx);
    cx.run_until_parked();
    // The 800 px canvas has room for all of it.
    let canvas = view.read_with(&cx, |view, _| view.bounds);
    let bar = whole(&mut cx);
    assert!(bar.size.width < canvas.size.width - px(16.));

    // A 300 px canvas, narrower than the bar (391 px, its menus' buttons pictures alone): the
    // bar is as wide as the canvas less 8 px at each side, and its controls keep their size,
    // the end cap's menu past its right edge.
    let canvas = resize(&mut cx, 300.);
    assert_eq!(canvas.size.width, px(300.));
    let inside = |bounds: Bounds<Pixels>| {
        bounds.left() >= canvas.left() + px(8.) && bounds.right() <= canvas.right() - px(8.)
    };
    let bar = drawn(&mut cx, "board-context-bar");
    assert_eq!(bar.left(), canvas.left() + px(8.));
    assert_eq!(bar.right(), canvas.right() - px(8.));
    let first = drawn(&mut cx, "board-context-color-yellow");
    assert_eq!(first.left(), bar.left() + px(7.));
    assert!(!inside(drawn(&mut cx, "board-context-end")));
    // The wheel over it scrolls them sideways, and the end cap's menu comes into sight. The
    // bar takes the scroll: the canvas under it doesn't pan.
    let before = view_state(&view, &cx);
    cx.simulate_event(ScrollEvent {
        position: bar.center(),
        delta: ScrollDelta::Lines(point(0., -50.)),
        ..ScrollEvent::default()
    });
    cx.run_until_parked();
    let end = drawn(&mut cx, "board-context-end");
    assert!(inside(end), "{end:?} in {canvas:?}");
    // Scrolled to its end, it sits where it does in a bar of full width.
    assert_eq!(end.right(), bar.right() - px(7.));
    assert!(!inside(drawn(&mut cx, "board-context-color-yellow")));
    let after = view_state(&view, &cx);
    assert_eq!(
        (after.cx, after.cy, after.zoom),
        (before.cx, before.cy, before.zoom)
    );
    // Its menu opens there, and the cap picked from it is the connector's.
    let end_cap = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| {
            let bar = context_bar(&view.board().unwrap().session)?;
            bar.controls.into_iter().find_map(|control| match control {
                Control::EndCap { current } => current,
                _ => None,
            })
        })
    };
    assert_eq!(end_cap(&cx).as_deref(), Some("triangleArrow"));
    click(&mut cx, "board-context-end");
    cx.run_until_parked();
    click(&mut cx, "board-context-end-Circle");
    cx.run_until_parked();
    assert_eq!(end_cap(&cx).as_deref(), Some("circleFilled"));

    // With room again, it shows its whole width once more, from its first control.
    resize(&mut cx, 800.);
    whole(&mut cx);
}
