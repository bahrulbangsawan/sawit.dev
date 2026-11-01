//! Keys pressed on the canvas, and the keyboard it takes and gives back.

use super::*;

/// CANVAS-UI-6: ⇧Space and ⌥⌘↩, FigCraft's Preview and Present, are no keys of the canvas,
/// which has no prototype player to show: on a board they only said, in a toast, that
/// prototypes aren't for boards, and on a design file the player's request went nowhere.
#[gpui_kit::test]
fn the_prototype_players_keys_do_nothing_on_the_canvas(cx: &mut TestAppContext) {
    let tmp = Scratch::new("prototype-keys");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    focus_canvas(&view, &mut cx);
    for keys in ["shift-space", "alt-cmd-enter"] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        let toast = view.read_with(&cx, |view, _| view.toast.clone());
        assert_eq!(toast, None, "{keys}");
    }
}

/// The user's screenshot: ⌥⌘G with nothing selected toasted "command `object.frameSelection`
/// is not available right now: nothing is selected (pass `ids`)". A key with nothing to act on
/// does nothing and says nothing: FigCraft's keys (⌥⌘G, ⌘G, ⌥⌘V, ⌃⌥⌘K, ⌘C), one of Sawit's
/// (⌃⌥M, Use as mask), and the key of what Sawit carries out itself (⌃⌥⌘C, Copy as text).
/// With a layer selected, ⌥⌘G frames it.
#[gpui_kit::test]
fn a_key_with_nothing_to_act_on_says_nothing(cx: &mut TestAppContext) {
    let tmp = Scratch::new("quiet-keys");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    focus_canvas(&view, &mut cx);
    let toast = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.toast.clone());
    for keys in [
        "alt-cmd-g",
        "cmd-g",
        "alt-cmd-v",
        "ctrl-alt-cmd-k",
        "cmd-c",
        "ctrl-alt-m",
        "ctrl-alt-cmd-c",
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        assert_eq!(toast(&cx), None, "{keys}");
    }
    let sticky = json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}});
    run_op(&view, sticky, &mut cx);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("alt-cmd-g");
    cx.run_until_parked();
    assert_eq!(toast(&cx), None);
    let framed = view.read_with(&cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        let page = doc.node(doc.first_page().unwrap()).unwrap();
        (page.children.iter())
            .filter_map(|id| doc.node(*id))
            .any(|node| matches!(node.kind, figcraft_doc::NodeKind::Frame))
    });
    assert!(framed, "⌥⌘G framed nothing");
}

/// SMOKE-3 still holds: a board's tool's key on a design file says why it does nothing, in
/// plain words.
#[gpui_kit::test]
fn a_boards_tool_key_on_a_design_file_says_why(cx: &mut TestAppContext) {
    let tmp = Scratch::new("boards-only-key");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    view.update(&mut cx, |view, cx| {
        let design = figcraft_engine::Session::with_document(Document::new("Design"));
        view.replace_doc(BoardDoc { session: design });
        cx.notify();
    });
    cx.run_until_parked();
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("q");
    cx.run_until_parked();
    let toast = view.read_with(&cx, |view, _| view.toast.clone());
    assert_eq!(
        toast.as_deref(),
        Some("This is for boards, and this canvas is a design file")
    );
}

#[gpui_kit::test]
fn a_keys_requests_reach_the_view(cx: &mut TestAppContext) {
    let tmp = Scratch::new("key-requests");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let made = run_op(&view, sticky_op(), &mut cx);
    let first = type_in(&view, &made, &mut cx);
    focus_canvas(&view, &mut cx);
    // ⇧⌘↩ while typing makes the next sticky, and the engine asks the view to go on typing
    // there.
    cx.simulate_keystrokes("shift-cmd-enter");
    let [one, next] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    assert_eq!(one, first);
    view.read_with(&cx, |view, _| {
        let board = view.board().unwrap();
        let ds = board.session.ds().unwrap();
        let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
        assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
        assert!(board.session.ui_requests.is_empty());
    });
}

/// The id of the tool the canvas's pointer events go to.
fn tool_id(view: &Entity<BoardView>, cx: &TestAppContext) -> &'static str {
    view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
}

/// Whether the canvas itself has the keyboard.
fn has_keyboard(view: &Entity<BoardView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.read(cx).focus.is_focused(window))
}

/// Gives the keyboard to something else than the canvas, as a terminal pane or the Files tree
/// has it, and returns that.
fn keyboard_elsewhere(cx: &mut VisualTestContext) -> FocusHandle {
    cx.update(|window, cx| {
        let elsewhere = cx.focus_handle();
        window.focus(&elsewhere, cx);
        elsewhere
    })
}

/// CANVAS-UI-2: the canvas the user goes to (⌃⌘C, the title bar's Canvas button, a canvas
/// file opened, a click on its tab, ⌃Tab) takes the keyboard with the next frame it is drawn
/// in, as the Excalidraw canvas's webview did each time it came on screen: the workspace asks
/// it to ([`BoardView::take_keyboard`]). Before, its keys went to whatever had the keyboard,
/// and R typed into the shell of the terminal pane that had it.
#[gpui_kit::test]
fn a_canvas_asked_to_take_the_keyboard_has_it_with_the_next_frame(cx: &mut TestAppContext) {
    let tmp = Scratch::new("take-keyboard");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    keyboard_elsewhere(&mut cx);
    cx.simulate_keystrokes("r");
    assert_eq!(tool_id(&view, &cx), "move");
    view.update(&mut cx, |view, cx| view.take_keyboard(cx));
    cx.run_until_parked();
    assert!(has_keyboard(&view, &mut cx));
    cx.simulate_keystrokes("r");
    assert_eq!(tool_id(&view, &cx), "rectangle");
}

/// CANVAS-UI-2: a canvas whose tab goes off screen with the keyboard gives it to the
/// workspace (`home`), so that the app's keys (⌃Tab, ⌘W) still work: a focus left on what
/// isn't drawn sends the keys nowhere. Shown again, it takes the keyboard back, unless
/// something else took it meanwhile.
#[gpui_kit::test]
fn a_hidden_canvas_gives_the_keyboard_back_and_takes_it_again(cx: &mut TestAppContext) {
    let tmp = Scratch::new("keyboard-home");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let home = cx.update(|_, cx| cx.focus_handle());
    let shown = |shown: bool, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.set_shown(shown, &home, window, cx));
        });
        cx.run_until_parked();
    };
    focus_canvas(&view, &mut cx);
    shown(false, &mut cx);
    assert!(cx.update(|window, _| home.is_focused(window)));
    shown(true, &mut cx);
    assert!(has_keyboard(&view, &mut cx));

    // Hidden with the keyboard, then the keyboard goes elsewhere: shown again, the canvas
    // leaves it there.
    shown(false, &mut cx);
    let elsewhere = keyboard_elsewhere(&mut cx);
    shown(true, &mut cx);
    assert!(!has_keyboard(&view, &mut cx));
    assert!(cx.update(|window, _| elsewhere.is_focused(window)));
    // Hidden without the keyboard: shown again, it doesn't take it either.
    shown(false, &mut cx);
    cx.update(|window, cx| window.focus(&home, cx));
    shown(true, &mut cx);
    assert!(!has_keyboard(&view, &mut cx));
}

#[gpui_kit::test]
fn keys_reach_the_tools_and_cmd_s_saves(cx: &mut TestAppContext) {
    let tmp = Scratch::new("key-tools");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("r");
    let tool = |cx: &VisualTestContext| {
        view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
    };
    assert_eq!(tool(&cx), "rectangle");
    // ⌘S is Save, which Sawit carries out itself (ruling P6-C2): the file is written now,
    // before the canvas has been still for the autosave's delay.
    run_op(&view, sticky_op(), &mut cx);
    cx.simulate_keystrokes("cmd-s");
    let toast = view.read_with(&cx, |view, _| view.toast.clone());
    assert_eq!(toast.as_deref(), Some("Saved"));
    assert_eq!(saved_layers(&tmp.0.join("canvas.sawit")), 1);
}
