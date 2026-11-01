//! Panning and zooming by hand: scrolls, a drag of the middle button, and Space with a drag.

use super::*;

#[gpui_kit::test]
fn scrolling_pans_and_zooms_about_the_pointer(cx: &mut TestAppContext) {
    let tmp = Scratch::new("scroll");
    let view = open(cx, &tmp.0.join("canvas.sawit"));
    let pointer = point(px(100.), px(50.));
    let scroll = |dx: f32, dy: f32, cmd: bool| ScrollEvent {
        position: pointer,
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        modifiers: Modifiers {
            platform: cmd,
            ..Modifiers::default()
        },
        ..ScrollEvent::default()
    };
    let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;

    // A plain scroll pans: the content follows the fingers.
    let before = view_state(&view, cx);
    view.update(cx, |view, cx| view.on_scroll(&scroll(30., -20., false), cx));
    let panned = view_state(&view, cx);
    assert_eq!(panned.zoom, before.zoom);
    assert!(close(
        panned.page_to_screen(0., 0.),
        (
            before.page_to_screen(0., 0.).0 + 30.,
            before.page_to_screen(0., 0.).1 - 20.
        )
    ));
    assert!(view.read_with(cx, |view, _| view.zooming_until.is_none()));

    // With ⌘ it zooms by e^(-dy/100), keeping the page point under the pointer there.
    let under = panned.screen_to_page(100., 50.);
    view.update(cx, |view, cx| view.on_scroll(&scroll(0., -20., true), cx));
    let zoomed = view_state(&view, cx);
    assert!((zoomed.zoom - panned.zoom * 0.2f64.exp()).abs() < 1e-9);
    assert!(close(zoomed.screen_to_page(100., 50.), under));
    assert!(view.read_with(cx, |view, _| view.zooming_until.is_some()));

    // A pinch zooms by 1 + its delta, about the pointer too.
    let pinch = PinchEvent {
        position: pointer,
        delta: 0.5,
        ..PinchEvent::default()
    };
    view.update(cx, |view, cx| view.on_pinch(&pinch, cx));
    let pinched = view_state(&view, cx);
    assert!((pinched.zoom - zoomed.zoom * 1.5).abs() < 1e-9);
    assert!(close(pinched.screen_to_page(100., 50.), under));

    // Full quality comes back once the gesture has been still for a moment.
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.zooming_until.is_none()));
}

/// Releases the keys of `keystrokes`, last first, as `simulate_keystrokes` presses them.
fn release_keys(cx: &mut VisualTestContext, keystrokes: &str) {
    for source in keystrokes.split(' ').rev() {
        let keystroke = Keystroke::parse_with_os(source, OperatingSystem::Mac).unwrap();
        cx.simulate_event(KeyUpEvent { keystroke });
    }
}

/// Whether `a` and `b` are the same page point, but for rounding.
fn same_point(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
}

#[gpui_kit::test]
fn a_middle_button_drag_pans_with_any_tool(cx: &mut TestAppContext) {
    let tmp = Scratch::new("middle-pan");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let tool = json!({"op": "command", "command": "tool.rectangle"});
    run_op(&view, tool, &mut cx);
    let pressed = view_state(&view, &cx).screen_to_page(100., 100.);
    drag(&mut cx, MouseButton::Middle);
    // The page point under the press is under the release.
    let released = view_state(&view, &cx).screen_to_page(160., 140.);
    assert!(same_point(released, pressed), "{released:?} != {pressed:?}");
    // The tools got nothing: no shape, no undo step, the tool still on.
    assert!(layers(&view, &cx).is_empty());
    view.read_with(&cx, |view, _| {
        assert!(view.pan.is_none());
        let board = view.board().unwrap();
        assert_eq!(board.session.tool.tool.id(), "rectangle");
        assert_eq!(board.session.undo_depth(), (0, 0));
    });
}

#[gpui_kit::test]
fn space_and_a_drag_pan_instead_of_drawing(cx: &mut TestAppContext) {
    let tmp = Scratch::new("space-pan");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let tool = json!({"op": "command", "command": "tool.rectangle"});
    run_op(&view, tool, &mut cx);
    focus_canvas(&view, &mut cx);

    // Held, Space turns a drag into a pan.
    cx.simulate_keystrokes("space");
    let pressed = view_state(&view, &cx).screen_to_page(100., 100.);
    drag(&mut cx, MouseButton::Left);
    let released = view_state(&view, &cx).screen_to_page(160., 140.);
    assert!(same_point(released, pressed), "{released:?} != {pressed:?}");
    assert!(layers(&view, &cx).is_empty());

    // Released, the same drag draws.
    release_keys(&mut cx, "space");
    drag(&mut cx, MouseButton::Left);
    assert_eq!(layers(&view, &cx).len(), 1);
}

#[gpui_kit::test]
fn space_is_typed_while_text_is_edited(cx: &mut TestAppContext) {
    let tmp = Scratch::new("space-typed");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let made = run_op(&view, sticky_op(), &mut cx);
    type_in(&view, &made, &mut cx);
    focus_canvas(&view, &mut cx);
    let space = KeyDownEvent {
        keystroke: Keystroke::parse("space").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    let result = cx.update(|window, cx| window.dispatch_event(PlatformInput::KeyDown(space), cx));
    // It goes on past the canvas, to the input method, and arms no pan.
    assert!(result.propagate);
    view.read_with(&cx, |view, _| {
        assert!(!view.space_held);
        let ds = view.board().unwrap().session.ds().unwrap();
        assert!(ds.text_edit.is_some());
    });
}

#[gpui_kit::test]
fn losing_the_keyboard_lets_go_of_space(cx: &mut TestAppContext) {
    let tmp = Scratch::new("space-lost");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    // The window has the keyboard, as the one the user types in.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let held = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.space_held);
    // The canvas loses the keyboard in the window: Space's release goes elsewhere.
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("space");
    assert!(held(&cx));
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert!(!held(&cx));
    // The window loses it to another.
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("space");
    assert!(held(&cx));
    cx.deactivate_window();
    assert!(!held(&cx));
}
