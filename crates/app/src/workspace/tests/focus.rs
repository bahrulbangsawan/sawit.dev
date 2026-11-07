//! Where the keyboard goes as views go off screen and come back, tried in the workspace's
//! stand-in (`keyboard`).

/// What had the keyboard and goes off screen gives it to the workspace
/// ([`super::super::KeyboardHome`]), whatever it is: a terminal pane once ⌘T hides the dock, a file
/// editor's text once ⌃Tab shows another tab. Before, it kept the keyboard unseen, GPUI sent
/// the keys from the window's root, and the second ⌘T or ⌃Tab did nothing. Each of three
/// presses works, and the keyboard ends where it belongs: with the workspace once the dock is
/// hidden, with the text once ⌃Tab has gone round to its tab again. (The browser's page and
/// address field: `browser::tests`.)
#[gpui_kit::test]
fn what_goes_off_screen_gives_the_keyboard_to_the_workspace(cx: &mut gpui_kit::TestAppContext) {
    use super::keyboard::{Keyboard, has, open};
    use gpui_kit::Focusable as _;

    cx.update(|cx| {
        gpui_kit::init(cx);
        super::super::init(cx);
    });
    let mut failed = Vec::new();
    let home: Keyboard = |this, _| this.focus.clone();
    let pane: Keyboard = |this, _| this.pane.clone();
    let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);
    for (what, keyboard, keys, ends) in [
        ("a terminal pane", pane, "cmd-t", ("the workspace", home)),
        ("a file's text", text, "ctrl-tab", ("the text", text)),
    ] {
        let (window, workspace) = open(keyboard, cx);
        for _ in 0..3 {
            cx.simulate_keystrokes(window, keys);
            cx.run_until_parked();
        }
        let presses = cx.update(|cx| workspace.read(cx).presses);
        let has_it = has(ends.1, window, &workspace, cx);
        if presses != 3 || !has_it {
            failed.push(format!(
                "with the keyboard in {what}, {keys} pressed 3 times worked {presses} times; \
                     {} has the keyboard = {has_it}",
                ends.0
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// What went off screen with the keyboard has it again once it is shown again
/// ([`super::super::KeyboardHome::give_back`], review I4): a terminal pane hidden and shown by the
/// title bar's button (clicks, no key) or by ⌘T twice, and a file's text whose tab is left and
/// come back to by clicks, take the typing that follows, as they did before the workspace took
/// the keyboard from what is hidden. What has been clicked meanwhile keeps the keyboard, and
/// the browser's page (here the stand-in's page) waits for a click, as it always did.
#[gpui_kit::test]
fn what_is_shown_again_has_the_keyboard_again(cx: &mut gpui_kit::TestAppContext) {
    use super::keyboard::{Keyboard, has, open, show};
    use gpui_kit::{AppContext as _, Focusable as _};

    cx.update(|cx| {
        gpui_kit::init(cx);
        super::super::init(cx);
    });
    let mut failed = Vec::new();
    let home: Keyboard = |this, _| this.focus.clone();
    let pane: Keyboard = |this, _| this.pane.clone();
    let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);
    let page: Keyboard = |this, _| this.page.clone();

    let (window, workspace) = open(pane, cx);
    for dock in [false, true] {
        show(&workspace, cx, |w| w.dock = dock);
    }
    cx.simulate_keystrokes(window, "l s");
    let typed = cx.update(|cx| workspace.read(cx).typed.clone());
    if typed != ["l", "s"] {
        failed.push(format!(
            "the terminal pane hidden and shown by clicks got {typed:?} of `l s`"
        ));
    }

    let (window, workspace) = open(pane, cx);
    for _ in 0..2 {
        cx.simulate_keystrokes(window, "cmd-t");
        cx.run_until_parked();
    }
    cx.simulate_keystrokes(window, "l s");
    let (dock, typed) = cx.update(|cx| {
        let workspace = workspace.read(cx);
        (workspace.dock, workspace.typed.clone())
    });
    if !dock || typed != ["l", "s"] {
        failed.push(format!(
            "after cmd-t twice the dock shows = {dock}, and the pane got {typed:?} of `l s`"
        ));
    }

    let (window, workspace) = open(text, cx);
    for tab in [1, 0] {
        show(&workspace, cx, |w| w.tab = tab);
    }
    cx.simulate_input(window, "hi");
    let value = cx.update(|cx| workspace.read(cx).text.read(cx).value().to_string());
    if value != "hi" {
        failed.push(format!(
            "the file's text, its tab left and come back to, got {value:?} of \"hi\""
        ));
    }

    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    let to = cx.update(|cx| pane(workspace.read(cx), cx));
    cx.update_window(window, |_, window, cx| window.focus(&to, cx))
        .unwrap();
    cx.run_until_parked();
    show(&workspace, cx, |w| w.tab = 0);
    if !has(pane, window, &workspace, cx) {
        failed.push(
            "the terminal pane clicked while the text was hidden lost the keyboard when \
                 the text was shown again"
                .into(),
        );
    }

    let (window, workspace) = open(page, cx);
    for tab in [1, 0] {
        show(&workspace, cx, |w| w.tab = tab);
    }
    if !has(home, window, &workspace, cx) {
        failed.push("the page, hidden and shown, took the keyboard back".into());
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Given back while it is still off screen, the keyboard comes home
/// ([`super::super::KeyboardHome`], review I5): GPUI takes it from what isn't drawn only in a frame
/// after one with a keyboard path, and the frame that hid it had none. So two changes on screen in
/// a row, the second showing something else again, left the keyboard on the hidden view, past every
/// workspace shortcut, until a click: ⌘T, ⌃Tab, ⌘T from a terminal pane left the dock hidden; ⌃Tab,
/// ⌘T, ⌃Tab from a file's text lost the last ⌃Tab. Also: what is clicked meanwhile makes the
/// workspace forget what it took, and a redraw that changes nothing gives nothing back (one would,
/// given back to what is hidden, come home and ask for the next frame, without end).
#[gpui_kit::test]
fn the_keyboard_never_stays_on_what_is_hidden(cx: &mut gpui_kit::TestAppContext) {
    use super::keyboard::{Keyboard, has, open, show};
    use gpui_kit::{AppContext as _, Focusable as _};

    cx.update(|cx| {
        gpui_kit::init(cx);
        super::super::init(cx);
    });
    let mut failed = Vec::new();
    let home: Keyboard = |this, _| this.focus.clone();
    let pane: Keyboard = |this, _| this.pane.clone();
    let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);

    for (what, from, keys, presses, ends) in [
        (
            "⌘T, ⌃Tab, ⌘T",
            pane,
            "cmd-t ctrl-tab cmd-t",
            3,
            ("the pane", pane),
        ),
        (
            "⌃Tab, ⌘T, ⌃Tab, ⌃Tab",
            text,
            "ctrl-tab cmd-t ctrl-tab ctrl-tab",
            4,
            ("the text", text),
        ),
    ] {
        let (window, workspace) = open(from, cx);
        for key in keys.split(' ') {
            cx.simulate_keystrokes(window, key);
            cx.run_until_parked();
        }
        let worked = cx.update(|cx| workspace.read(cx).presses);
        let has_it = has(ends.1, window, &workspace, cx);
        if worked != presses || !has_it {
            failed.push(format!(
                "{what}: {worked} of {presses} presses worked; {} has the keyboard = {has_it}",
                ends.0
            ));
        }
    }

    // The text's tab left, then the dock hidden, both by clicks: ⌘T still works.
    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    show(&workspace, cx, |w| w.dock = false);
    cx.simulate_keystrokes(window, "cmd-t");
    cx.run_until_parked();
    let (worked, dock) = cx.update(|cx| (workspace.read(cx).presses, workspace.read(cx).dock));
    if worked != 1 || !dock || !has(home, window, &workspace, cx) {
        failed.push(format!(
            "after the text's tab and the dock were hidden by clicks, cmd-t worked {worked} \
                 times and the dock shows = {dock}"
        ));
    }

    // What is clicked meanwhile makes the workspace forget the text: the workspace gets the
    // keyboard back another way (as a canvas going off screen gives it), and the text's tab
    // shown again doesn't take it.
    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    for to in [pane, home] {
        let to = cx.update(|cx| to(workspace.read(cx), cx));
        cx.update_window(window, |_, window, cx| window.focus(&to, cx))
            .unwrap();
        cx.run_until_parked();
    }
    show(&workspace, cx, |w| w.tab = 0);
    if !has(home, window, &workspace, cx) {
        failed.push("the text took the keyboard back after the pane had it meanwhile".into());
    }

    // A redraw that changes nothing gives nothing back; the text's tab shown again does.
    let (window, workspace) = open(text, cx);
    show(&workspace, cx, |w| w.tab = 1);
    show(&workspace, cx, |_| {});
    let home_had_it = has(home, window, &workspace, cx);
    show(&workspace, cx, |w| w.tab = 0);
    if !home_had_it || !has(text, window, &workspace, cx) {
        failed.push(format!(
            "after a redraw with the text's tab hidden the workspace had the keyboard = \
                 {home_had_it}, and the text didn't get it back when shown"
        ));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
