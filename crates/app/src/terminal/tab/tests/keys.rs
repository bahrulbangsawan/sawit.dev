//! The pane menu, its keys and the terminal's pane keys (⇧⇥, ⌘], ⌘[), renaming, and
//! right-clicks.

use gpui_kit::{
    Action, AppContext as _, Focusable as _, KeyContext, Keystroke, Modifiers, MouseButton,
    TestAppContext, VisualTestContext,
};

use crate::terminal::RenameTab;
use crate::terminal::tab::menu::pane_menu_items;
use crate::terminal::testing::{
    focus_terminal, init, open_panel, ready, scratch_dir, start_cat, wait_until,
};

#[test]
fn pane_menu_lists_the_planned_items() {
    let labels: Vec<_> = pane_menu_items(false, true)
        .into_iter()
        .map(|(l, _, _)| l)
        .collect();
    assert_eq!(
        labels,
        [
            "Rename pane",
            "Split right",
            "Split down",
            "Zoom",
            "Send right-clicks to pane",
            "Close pane"
        ]
    );
    assert_eq!(pane_menu_items(true, false)[3].0, "Unzoom");
    assert_eq!(pane_menu_items(false, true)[4].1, Some(true));
}

/// The keys of the terminal's menus as a terminal sees them: each item of the pane menu
/// and the tab menu's Rename Tab, with the first keys bound to it that run it there.
fn menu_keys(cx: &mut TestAppContext) -> Vec<(String, Option<String>)> {
    let terminal: Vec<KeyContext> = ["Root", "Workspace", "Terminal"]
        .iter()
        .map(|c| KeyContext::parse(c).unwrap())
        .collect();
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let mut actions: Vec<Box<dyn Action>> = pane_menu_items(false, false)
            .into_iter()
            .map(|(_, _, action)| action)
            .collect();
        actions.push(Box::new(RenameTab));
        actions
            .iter()
            .map(|action| {
                let keys = keymap.bindings_for_action(action.as_ref()).find_map(|b| {
                    let typed: Vec<Keystroke> = b
                        .keystrokes()
                        .iter()
                        .map(|k| Keystroke {
                            modifiers: *k.modifiers(),
                            key: k.key().to_string(),
                            key_char: None,
                        })
                        .collect();
                    let (runs, _) = keymap.bindings_for_input(&typed, &terminal);
                    runs.first()
                        .is_some_and(|run| run.action().partial_eq(action.as_ref()))
                        .then(|| {
                            typed
                                .iter()
                                .map(|k| k.unparse(gpui_kit::OperatingSystem::Mac))
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                });
                (action.name().to_string(), keys)
            })
            .collect()
    })
}

/// Every item of the pane menu, and the tab menu's Rename Tab, has a key that runs it in a
/// terminal, which the menu shows beside it, and no two items share one.
#[gpui_kit::test]
fn every_menu_item_has_its_own_key(cx: &mut TestAppContext) {
    cx.update(init);
    let keys = menu_keys(cx);
    assert_eq!(
        keys,
        [
            ("terminal::RenamePane", Some("cmd-i")),
            ("terminal::SplitRight", Some("cmd-d")),
            ("terminal::SplitDown", Some("cmd-shift-d")),
            ("terminal::ZoomPane", Some("cmd-shift-enter")),
            ("terminal::ToggleSendRightClicks", Some("alt-cmd-r")),
            ("terminal::ClosePane", Some("alt-cmd-w")),
            ("terminal::RenameTab", Some("cmd-shift-i")),
        ]
        .map(|(action, keys)| (action.to_string(), keys.map(String::from)))
    );
}

/// The keymap editor lists the new keys as the terminal's defaults, so they can be changed
/// there (and the menus then show the keys as changed).
#[gpui_kit::test]
fn the_keymap_editor_lists_the_menu_keys(cx: &mut TestAppContext) {
    let file = scratch_dir("menu-keymap").join("keymap.json");
    cx.update(|cx| {
        init(cx);
        crate::keymap::init_at(file, cx);
        let rows = crate::keymap::store(cx).read(cx).rows(false, cx);
        for (action, keys) in [
            ("terminal::RenamePane", "cmd-i"),
            ("terminal::RenameTab", "cmd-shift-i"),
            ("terminal::ToggleSendRightClicks", "alt-cmd-r"),
            ("terminal::ClosePane", "alt-cmd-w"),
            ("terminal::FocusNextPane", "cmd-]"),
            ("terminal::FocusPreviousPane", "cmd-["),
            ("terminal::FocusNextPaneAtPrompt", "shift-tab"),
        ] {
            let row = rows
                .iter()
                .find(|r| r.action.as_ref() == action)
                .unwrap_or_else(|| panic!("the keymap editor doesn't list {action}"));
            assert_eq!(row.keystrokes.as_ref(), keys, "{action}");
            assert_eq!(row.context.as_deref(), Some("Terminal"), "{action}");
            assert!(
                matches!(row.source, crate::keymap::Source::Default(_)),
                "{action} isn't a default"
            );
        }
    });
}

/// ⇧⇥ at a shell prompt in a split tab moves to the next pane, wrapping round, and never
/// reaches the shell. With a program in front (`cat`, standing in for Claude Code, which
/// switches modes with ⇧⇥) it goes to the program, and so it does in a tab of one pane.
#[gpui_kit::test]
fn shift_tab_moves_to_the_next_pane_only_at_a_prompt(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let one = cx.update(|cx| tab.read(cx).focused_view().clone());
    ready(cx, &one);
    // One pane: the shell gets the key.
    cx.simulate_keystrokes(w, "shift-tab");
    cx.update(|cx| {
        assert!(
            one.read(cx).touched,
            "a one-pane tab kept ⇧⇥ from the shell"
        );
        assert_eq!(tab.read(cx).views().count(), 1);
    });

    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let (left, right) = cx.update(|cx| {
        let t = tab.read(cx);
        let panes = t.state.layout.panes();
        (panes[0], panes[1])
    });
    let right_view = cx.update(|cx| tab.read(cx).view(right).unwrap().clone());
    ready(cx, &right_view);
    let focused = |cx: &mut TestAppContext| cx.update(|cx| tab.read(cx).state.focused);
    assert_eq!(focused(cx), right);
    cx.simulate_keystrokes(w, "shift-tab");
    assert_eq!(
        focused(cx),
        left,
        "⇧⇥ at the prompt didn't wrap to the first pane"
    );
    let typed = cx
        .update_window(w, |_, window, cx| one.focus_handle(cx).is_focused(window))
        .unwrap();
    assert!(typed, "the keyboard didn't follow the pane");
    cx.simulate_keystrokes(w, "shift-tab");
    assert_eq!(focused(cx), right);
    cx.update(|cx| {
        assert!(
            !right_view.read(cx).touched,
            "⇧⇥ also reached the shell it left"
        )
    });

    // A program in front gets the key, and the focus stays.
    start_cat(cx, &right_view);
    cx.simulate_keystrokes(w, "shift-tab");
    assert_eq!(focused(cx), right, "⇧⇥ left a pane running a program");
    wait_until(cx, "cat gets ⇧⇥", |cx| {
        right_view.read(cx).text_tail(5).contains("^[[Z")
    });
}

/// ⌘] and ⌘[ step through the panes in reading order (left to right, then down), whatever
/// runs in them, and wrap round.
#[gpui_kit::test]
fn cmd_brackets_step_through_the_panes_and_wrap(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    cx.simulate_keystrokes(w, "cmd-shift-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    // left | right-top / right-bottom, the keyboard in right-bottom.
    let order = cx.update(|cx| {
        let t = tab.read(cx);
        t.state.reading_order(t.area())
    });
    assert_eq!(order.len(), 3);
    let focused = |cx: &mut TestAppContext| cx.update(|cx| tab.read(cx).state.focused);
    assert_eq!(focused(cx), order[2]);
    let mut seen = Vec::new();
    for keys in ["cmd-]", "cmd-]", "cmd-[", "cmd-["] {
        cx.simulate_keystrokes(w, keys);
        seen.push(focused(cx));
    }
    assert_eq!(seen, [order[0], order[1], order[0], order[2]]);
    // A program in front doesn't stop them.
    let view = cx.update(|cx| tab.read(cx).focused_view().clone());
    ready(cx, &view);
    start_cat(cx, &view);
    cx.simulate_keystrokes(w, "cmd-]");
    assert_eq!(focused(cx), order[0]);
}

/// ⌘[ and ⌘] are the terminal's own: nothing in the workspace around it uses them (the
/// editors' indent keys and the canvas's have contexts of their own), so a terminal taking
/// them takes nothing away.
#[gpui_kit::test]
fn cmd_brackets_take_no_workspace_key(cx: &mut TestAppContext) {
    cx.update(init);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let stack = |contexts: &[&str]| -> Vec<KeyContext> {
            contexts
                .iter()
                .map(|c| KeyContext::parse(c).unwrap())
                .collect()
        };
        for (key, action) in [
            ("cmd-]", "terminal::FocusNextPane"),
            ("cmd-[", "terminal::FocusPreviousPane"),
        ] {
            let keys = [Keystroke::parse(key).unwrap()];
            let (outside, _) = keymap.bindings_for_input(&keys, &stack(&["Root", "Workspace"]));
            assert!(
                outside.is_empty(),
                "{key} is already {:?}",
                outside[0].action().name()
            );
            let (inside, _) =
                keymap.bindings_for_input(&keys, &stack(&["Root", "Workspace", "Terminal"]));
            assert_eq!(inside.first().map(|b| b.action().name()), Some(action));
        }
    });
}

/// The pane menu's keys act on the pane with the keyboard: ⌘I names it in its header, ⌥⌘R
/// sends its right-clicks on and ⌥⌘W closes it. ⇧⌘I names the tab, which keeps the name
/// whichever pane has focus, and after its other panes close.
#[gpui_kit::test]
fn the_menu_keys_act_on_the_focused_pane_and_its_tab(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let panes = cx.update(|cx| tab.read(cx).state.layout.panes());
    assert_eq!(panes.len(), 2);
    let rename = |cx: &mut TestAppContext, keys: &str, name: &str| {
        cx.simulate_keystrokes(w, keys);
        cx.run_until_parked();
        cx.simulate_input(w, name);
        cx.simulate_keystrokes(w, "enter");
        cx.run_until_parked();
    };

    rename(cx, "cmd-i", "api");
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.state.panes[&panes[1]].name.as_deref(), Some("api"));
        assert_eq!(t.state.panes[&panes[0]].name, None);
        assert_eq!(t.state.name, None, "⌘I in a split named the tab");
    });
    rename(cx, "cmd-shift-i", "server");
    cx.update(|cx| assert_eq!(tab.read(cx).title(cx), "server"));
    cx.simulate_keystrokes(w, "cmd-alt-left");
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.state.focused, panes[0]);
        assert_eq!(t.title(cx), "server", "the tab's name followed the focus");
    });

    cx.simulate_keystrokes(w, "cmd-alt-r");
    let sent = |cx: &mut TestAppContext, pane| {
        cx.update(|cx| tab.read(cx).view(pane).unwrap().read(cx).send_right_clicks)
    };
    assert!(
        sent(cx, panes[0]),
        "⌥⌘R left the focused pane's right-clicks"
    );
    assert!(!sent(cx, panes[1]), "⌥⌘R sent another pane's right-clicks");

    cx.simulate_keystrokes(w, "cmd-alt-w");
    cx.run_until_parked();
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.state.layout.panes(), [panes[1]]);
        assert_eq!(t.title(cx), "server");
    });
    let kept = cx
        .update_window(w, |_, window, cx| tab.read(cx).has_focus(window, cx))
        .unwrap();
    assert!(kept, "the keyboard did not go to the pane left");
}

/// A tab of one pane shows no header, so renaming its pane names the tab, over the program's
/// title; the name stays on the tab once it is split and the new pane has focus.
#[gpui_kit::test]
fn renaming_the_pane_of_a_one_pane_tab_names_the_tab(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let pane = cx.update(|cx| tab.read(cx).state.focused);
    cx.update_window(w, |_, window, cx| {
        tab.update(cx, |t, cx| t.rename_pane(pane, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_input(w, "build");
    cx.simulate_keystrokes(w, "enter");
    cx.run_until_parked();
    cx.update(|cx| {
        let t = tab.read(cx);
        assert!(t.renaming.is_none());
        assert_eq!(t.title(cx), "build");
        assert_eq!(t.state.name.as_deref(), Some("build"));
    });
    let back = cx
        .update_window(w, |_, window, cx| tab.read(cx).has_focus(window, cx))
        .unwrap();
    assert!(back, "the keyboard did not go back to the pane");
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_ne!(t.state.focused, pane);
        assert_eq!(t.title(cx), "build", "the split lost the tab's name");
    });
}

#[gpui_kit::test]
fn right_clicks_open_the_pane_menu_unless_sent_on(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let pane = cx.update(|cx| tab.read(cx).state.focused);
    let at = cx.update(|cx| tab.read(cx).bounds.get().unwrap().center());
    let mut vcx = VisualTestContext::from_window(w, cx);
    let right_click = |vcx: &mut VisualTestContext, modifiers: Modifiers| {
        vcx.simulate_mouse_down(at, MouseButton::Right, modifiers);
        vcx.simulate_mouse_up(at, MouseButton::Right, modifiers);
        vcx.run_until_parked();
    };
    let menu_open = |vcx: &mut VisualTestContext| vcx.update(|_, cx| tab.read(cx).menu.is_some());

    right_click(&mut vcx, Modifiers::default());
    assert!(menu_open(&mut vcx), "a right-click did not open the menu");

    vcx.update(|window, cx| {
        tab.update(cx, |t, cx| {
            t.run_menu_item(pane, "Send right-clicks to pane", window, cx);
            t.menu = None;
        })
    });
    let sent = vcx.update(|_, cx| tab.read(cx).focused_view().read(cx).send_right_clicks);
    assert!(sent, "the pane does not send right-clicks on");
    // The shell does not take the mouse, so the click goes nowhere.
    right_click(&mut vcx, Modifiers::default());
    assert!(
        !menu_open(&mut vcx),
        "the menu opened for a sent right-click"
    );

    right_click(&mut vcx, Modifiers::shift());
    assert!(
        menu_open(&mut vcx),
        "Shift+right-click did not open the menu"
    );
}
