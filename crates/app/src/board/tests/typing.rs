//! Typing in the text being edited, with the keys and with an input method.

use super::*;

/// Calls the canvas through `input`, as the platform's text input calls it.
fn with_input<R>(
    view: &Entity<BoardView>,
    cx: &mut VisualTestContext,
    input: impl FnOnce(&mut BoardView, &mut Window, &mut Context<BoardView>) -> R,
) -> R {
    cx.update(|window, cx| view.update(cx, |view, cx| input(view, window, cx)))
}

/// An input method composes `text`, its cursor at the end.
fn mark(view: &Entity<BoardView>, text: &str, cx: &mut VisualTestContext) {
    with_input(view, cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, text, None, window, cx)
    });
}

/// The input method takes `text` as typed, in place of its composition or the selection.
fn commit(view: &Entity<BoardView>, text: &str, cx: &mut VisualTestContext) {
    with_input(view, cx, |view, window, cx| {
        view.replace_text_in_range(None, text, window, cx)
    });
}

fn unmark(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
    with_input(view, cx, |view, window, cx| view.unmark_text(window, cx));
}

/// What sticky `id` reads in the document.
fn sticky_reads(view: &Entity<BoardView>, id: NodeId, cx: &VisualTestContext) -> String {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, id).unwrap();
        let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
        text.characters.clone()
    })
}

/// What an input method composes in the text being edited: shown there, not in the document.
fn composing(view: &Entity<BoardView>, cx: &VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        let ds = view.board().unwrap().session.ds().unwrap();
        let preedit = ds.text_edit.as_ref()?.preedit.as_ref()?;
        Some(preedit.text.clone())
    })
}

fn editing(view: &Entity<BoardView>, cx: &VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.editing_text())
}

#[gpui_kit::test]
fn input_handler_inserts_text(cx: &mut TestAppContext) {
    let tmp = Scratch::new("input-handler");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let made = run_op(&view, sticky_op(), &mut cx);
    let note = type_in(&view, &made, &mut cx);
    commit(&view, "ok", &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "ok");
    assert!(editing(&view, &cx));
}

#[gpui_kit::test]
fn typed_keys_reach_the_text_being_edited(cx: &mut TestAppContext) {
    let tmp = Scratch::new("typed-keys");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let made = run_op(&view, sticky_op(), &mut cx);
    let note = type_in(&view, &made, &mut cx);
    focus_canvas(&view, &mut cx);
    // The canvas leaves a typed character alone, and it comes back as text through the text
    // input the canvas's paint hands the window while it edits.
    cx.simulate_input("ok");
    assert_eq!(sticky_reads(&view, note, &cx), "ok");
    // Space too, which arms a pan only while no text is edited (ruling P2-C1).
    cx.simulate_keystrokes("space");
    assert_eq!(sticky_reads(&view, note, &cx), "ok ");
    // Named keys are the text editor's own.
    cx.simulate_keystrokes("backspace backspace");
    assert_eq!(sticky_reads(&view, note, &cx), "o");
    cx.simulate_keystrokes("escape");
    assert!(!editing(&view, &cx));
    assert_eq!(sticky_reads(&view, note, &cx), "o");
}

/// Review Focus 3 through the platform's text input (ruling P2-C9): a composition the input
/// method cancels, or one a press away from the text leaves behind, leaves no text.
#[gpui_kit::test]
fn a_cancelled_composition_leaves_no_text(cx: &mut TestAppContext) {
    let tmp = Scratch::new("ime-cancel");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    // A 200 × 200 sticky about page point (0, 0), the middle of the canvas, typed in.
    let sticky = json!({
        "op": "command", "command": "board.sticky", "params": {"x": -100, "y": -100},
    });
    let made = run_op(&view, sticky, &mut cx);
    let note = type_in(&view, &made, &mut cx);
    // Marked, the composition shows in the sticky but isn't in the document.
    mark(&view, "ni", &mut cx);
    assert_eq!(composing(&view, &cx).as_deref(), Some("ni"));
    assert_eq!(sticky_reads(&view, note, &cx), "");
    // The input method cancels it (Escape while it composes): marked empty, it is gone.
    mark(&view, "", &mut cx);
    assert_eq!(composing(&view, &cx), None);
    assert_eq!(sticky_reads(&view, note, &cx), "");
    assert!(editing(&view, &cx));

    // Marked again, then a press on the canvas away from the sticky ends the edit, and the
    // unmark the input method sends after that commits nothing.
    mark(&view, "ni", &mut cx);
    let (away, left, none) = (
        point(px(20.), px(60.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    cx.simulate_mouse_down(away, left, none);
    cx.simulate_mouse_up(away, left, none);
    assert!(!editing(&view, &cx));
    unmark(&view, &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "");
    assert_eq!(layers(&view, &cx), [note]);

    // Each time typing in the sticky starts, it is another edit of the same text. The input
    // method marks text in one; once a second has started, the composition there isn't the
    // one it made, and its unmark doesn't commit it.
    type_in(&view, &made, &mut cx);
    mark(&view, "ni", &mut cx);
    type_in(&view, &made, &mut cx);
    view.update(&mut cx, |view, _| {
        let session = &mut view.board_mut().unwrap().session;
        figcraft_engine::textedit::preedit(session, "ni", None).unwrap();
    });
    unmark(&view, &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "");
}

#[gpui_kit::test]
fn the_input_method_commits_its_composition(cx: &mut TestAppContext) {
    let tmp = Scratch::new("ime-commit");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let made = run_op(&view, sticky_op(), &mut cx);
    let note = type_in(&view, &made, &mut cx);
    // Composing, the canvas renders the sticky with the composition in it.
    mark(&view, "ni", &mut cx);
    view.read_with(&cx, |view, _| {
        let composition = view.composition.as_ref().expect("the composition shown");
        let (_, asked) = view.requested.as_ref().expect("a render");
        assert!(Arc::ptr_eq(asked, &composition.doc));
    });
    // Committed in its final form, that takes the composition's place, in the document.
    commit(&view, "你", &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "你");
    assert_eq!(composing(&view, &cx), None);
    view.read_with(&cx, |view, _| {
        assert!(view.composition.is_none());
        let doc = view.board().unwrap().doc().unwrap();
        let (_, asked) = view.requested.as_ref().unwrap();
        assert!(Arc::ptr_eq(asked, &doc));
    });
    // Unmarked, the input method keeps its composition as it is: it goes in as composed.
    mark(&view, "hao", &mut cx);
    unmark(&view, &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "你hao");
    assert_eq!(composing(&view, &cx), None);
    // Once committed there is nothing left to commit.
    unmark(&view, &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "你hao");
    assert!(editing(&view, &cx));
}

#[gpui_kit::test]
fn the_input_method_reads_the_text_in_utf16(cx: &mut TestAppContext) {
    let tmp = Scratch::new("ime-utf16");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let made = run_op(&view, sticky_op(), &mut cx);
    type_in(&view, &made, &mut cx);
    // An emoji is two UTF-16 units: "a😀b" is four, and the caret is after the b.
    commit(&view, "a😀b", &mut cx);
    let selected = |cx: &mut VisualTestContext| {
        with_input(&view, cx, |view, window, cx| {
            view.selected_text_range(false, window, cx)
        })
        .expect("a selection")
    };
    let read = |range: Range<usize>, cx: &mut VisualTestContext| {
        let mut adjusted = None;
        let text = with_input(&view, cx, |view, window, cx| {
            view.text_for_range(range, &mut adjusted, window, cx)
        });
        (text.expect("the text"), adjusted.expect("the range read"))
    };
    let marked = |cx: &mut VisualTestContext| {
        with_input(&view, cx, |view, window, cx| {
            view.marked_text_range(window, cx)
        })
    };
    let caret = selected(&mut cx);
    assert_eq!((caret.range, caret.reversed), (4..4, false));
    assert_eq!(read(0..4, &mut cx), ("a😀b".to_string(), 0..4));
    // Half the emoji reads as all of it.
    assert_eq!(read(2..3, &mut cx), ("😀".to_string(), 1..3));
    assert_eq!(marked(&mut cx), None);

    // Composing, the composition is in the text it reads, marked, with its cursor in it.
    mark(&view, "ni", &mut cx);
    assert_eq!(marked(&mut cx), Some(4..6));
    assert_eq!(selected(&mut cx).range, 6..6);
    assert_eq!(read(0..6, &mut cx).0, "a😀bni");

    // Its candidates show by the caret, where the canvas draws it, in the window.
    let canvas = Bounds::new(point(px(10.), px(20.)), size(px(400.), px(300.)));
    let bounds = with_input(&view, &mut cx, |view, window, cx| {
        view.bounds_for_range(6..6, canvas, window, cx)
    })
    .expect("where the caret is");
    let (top, bottom) = view.read_with(&cx, |view, _| {
        let shapes = overlay::overlay(&view.board().unwrap().session);
        let caret = |shape: &Shape| match shape {
            Shape::Caret { top, bottom, .. } => Some((*top, *bottom)),
            _ => None,
        };
        shapes.iter().find_map(caret).expect("a caret")
    });
    let at = |(x, y): (f64, f64)| point(px(10. + x as f32), px(20. + y as f32));
    assert_eq!(bounds.origin, at(top));
    assert_eq!(bounds.bottom_left(), at(bottom));
}

#[gpui_kit::test]
fn an_accent_held_down_replaces_the_letter_before_the_caret(cx: &mut TestAppContext) {
    let tmp = Scratch::new("ime-replace");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let made = run_op(&view, sticky_op(), &mut cx);
    let note = type_in(&view, &made, &mut cx);
    commit(&view, "cafe", &mut cx);
    let replace = |range: Range<usize>, text: &str, cx: &mut VisualTestContext| {
        with_input(&view, cx, |view, window, cx| {
            view.replace_text_in_range(Some(range), text, window, cx)
        });
    };
    let caret = |cx: &mut VisualTestContext| {
        with_input(&view, cx, |view, window, cx| {
            view.selected_text_range(false, window, cx)
        })
        .expect("a selection")
        .range
    };
    // The accents menu (e held down, then é chosen) puts its letter in place of the e.
    replace(3..4, "é", &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "café");
    assert_eq!(caret(&mut cx), 4..4);
    // A range apart from the caret is replaced where it is, the caret after it.
    replace(0..1, "C", &mut cx);
    assert_eq!(sticky_reads(&view, note, &cx), "Café");
    assert_eq!(caret(&mut cx), 1..1);
}
