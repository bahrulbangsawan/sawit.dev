//! Where the keyboard goes when panes close or files are dropped on them.

use gpui_kit::{
    AppContext as _, Entity, ExternalPaths, FileDropEvent, Focusable as _, TestAppContext,
    VisualTestContext, point, px,
};

use crate::terminal::TerminalPanel;
use crate::terminal::tab::TerminalTab;
use crate::terminal::testing::{focus_terminal, init, open_panel, ready, scratch_dir, wait_until};

/// The index of `tab` in `panel`'s strip.
fn index_of(
    cx: &mut TestAppContext,
    panel: &Entity<TerminalPanel>,
    tab: &Entity<TerminalTab>,
) -> usize {
    cx.update(|cx| {
        let p = panel.read(cx);
        (0..p.tab_count())
            .find(|&ix| p.tab(ix) == tab)
            .expect("the tab is in the panel")
    })
}

#[gpui_kit::test]
fn closing_a_background_pane_by_index_leaves_the_keyboard_alone(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let background = cx.update(|cx| panel.read(cx).tab(0).clone());
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.open_shell(Some(window), cx))
    })
    .unwrap();
    cx.run_until_parked();
    let active = cx.update(|cx| {
        let p = panel.read(cx);
        p.tab(p.active_index()).clone()
    });
    assert!(active != background, "the new tab is not the active one");
    let typing = cx.update(|cx| active.read(cx).focused_view().focus_handle(cx));
    let has_keyboard = |cx: &mut TestAppContext| {
        cx.update_window(w, |_, window, _| typing.is_focused(window))
            .unwrap()
    };
    assert!(has_keyboard(cx), "the new tab did not take the keyboard");

    let ix = index_of(cx, &panel, &background);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.close_tab(ix, Some(1), window, cx))
    })
    .unwrap()
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(background.read(cx).views().count(), 1));
    assert!(
        has_keyboard(cx),
        "closing a background pane took the keyboard"
    );
}

#[gpui_kit::test]
fn closing_a_tabs_only_pane_gives_the_keyboard_to_the_next_tab(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.open_shell(Some(window), cx))
    })
    .unwrap();
    cx.run_until_parked();
    let closing = cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.tab_count(), 2);
        p.tab(p.active_index()).clone()
    });
    focus_terminal(cx, w, &panel);
    let pane = cx.update(|cx| closing.read(cx).state.focused);
    cx.update_window(w, |_, window, cx| {
        closing.update(cx, |t, cx| t.run_menu_item(pane, "Close pane", window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    let next = cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.tab_count(), 1);
        p.tab(p.active_index()).clone()
    });
    let kept = cx
        .update_window(w, |_, window, cx| next.read(cx).has_focus(window, cx))
        .unwrap();
    assert!(kept, "the keyboard did not go to the next tab");
}

/// A file dropped on a pane gives that pane the keyboard and the tab's focus too: the
/// focus ring, ⌘D, zoom and the agents' default pane all follow the keyboard.
#[gpui_kit::test]
fn dropping_a_file_on_a_pane_focuses_it(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let panes = cx.update(|cx| tab.read(cx).state.layout.panes());
    assert_eq!(panes.len(), 2);
    cx.simulate_keystrokes(w, "cmd-alt-left");
    cx.update(|cx| assert_eq!(tab.read(cx).state.focused, panes[0]));
    // The middle of the second pane, in window coordinates.
    let at = cx.update(|cx| {
        let t = tab.read(cx);
        let bounds = t.bounds.get().expect("the tab was drawn");
        let (_, rect) = t
            .state
            .visible(t.area().expect("the tab was drawn"))
            .into_iter()
            .find(|(pane, _)| *pane == panes[1])
            .expect("the second pane is drawn");
        bounds.origin + point(px(rect.x + rect.w / 2.), px(rect.y + rect.h / 2.))
    });
    let file = scratch_dir("dropped").join("notes.txt");
    let mut vcx = VisualTestContext::from_window(w, cx);
    // Straight after the keys, as a drag from Finder comes.
    vcx.simulate_event(FileDropEvent::Entered {
        position: at,
        paths: ExternalPaths(vec![file.clone()].into()),
    });
    vcx.simulate_event(FileDropEvent::Submit { position: at });
    vcx.run_until_parked();
    let (focused, second) = cx.update(|cx| {
        let t = tab.read(cx);
        (
            t.state.focused,
            t.view(panes[1]).expect("the second pane").clone(),
        )
    });
    let typed = cx
        .update_window(w, |_, window, cx| {
            second.focus_handle(cx).is_focused(window)
        })
        .unwrap();
    assert!(typed, "the drop didn't give the pane the keyboard");
    assert_eq!(focused, panes[1], "the tab still focuses the first pane");
}

/// A file dragged in from Finder (a screenshot for Claude Code, say) right after typing in
/// the terminal: macOS sends the window no mouse move while another app drags, only the
/// drag's own events, so the last input the window saw is still a key. The drop types the
/// file's path all the same.
#[gpui_kit::test]
fn a_file_dropped_right_after_typing_types_its_path(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let view = cx.update(|cx| tab.read(cx).focused_view().clone());
    ready(cx, &view);
    cx.simulate_keystrokes(w, "e c h o space");
    let at = cx.update(|cx| {
        tab.read(cx)
            .bounds
            .get()
            .expect("the tab was drawn")
            .center()
    });
    let dir = scratch_dir("drop");
    let file = dir.join("Screenshot 1.png");
    std::fs::write(&file, b"").unwrap();
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.simulate_event(FileDropEvent::Entered {
        position: at,
        paths: ExternalPaths(vec![file.clone()].into()),
    });
    vcx.simulate_event(FileDropEvent::Pending { position: at });
    vcx.simulate_event(FileDropEvent::Submit { position: at });
    vcx.simulate_event(FileDropEvent::Exited);
    vcx.run_until_parked();
    // Quoted, for its space; the space typed after it isn't on screen yet.
    let typed = format!("echo '{}'", file.display());
    wait_until(cx, "the dropped file's path is typed", |cx| {
        view.read(cx).text_tail(5).contains(&typed)
    });
}

#[gpui_kit::test]
fn closing_panes_closes_the_tab_last(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let panes = cx.update(|cx| tab.read(cx).state.layout.panes());
    assert_eq!(panes.len(), 2);

    let close = |cx: &mut TestAppContext, pane| {
        cx.update_window(w, |_, window, cx| {
            tab.update(cx, |t, cx| t.close_pane(pane, window, cx))
        })
        .unwrap();
        cx.run_until_parked();
    };
    let focused = cx.update(|cx| tab.read(cx).state.focused);
    close(cx, focused);
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.views().count(), 1);
        assert_eq!(t.views.len(), 1, "the closed pane's terminal was kept");
        assert_eq!(t.state.focused, panes[0]);
    });

    close(cx, panes[0]);
    cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 0));
}
