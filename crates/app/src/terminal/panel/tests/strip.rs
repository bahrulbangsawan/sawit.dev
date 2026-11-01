//! The tab strip: each tab's close button and menu, renaming a tab, and scrolling the active tab
//! into view.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    TestAppContext, VisualTestContext,
};

use super::super::TerminalPanel;
use super::{active, click_tab, has_keyboard, open_tabs, selector};
use crate::terminal::tab::TerminalTab;
use crate::terminal::testing::{init, open_panel, open_panel_sized, scratch_dir};

/// Clicks the × of tab `ix` in the strip.
fn click_close(cx: &mut TestAppContext, w: AnyWindowHandle, ix: usize) {
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    let at = vcx
        .debug_bounds(selector(format!("close-term-{ix}")))
        .unwrap_or_else(|| panic!("tab {ix}'s × isn't drawn"))
        .center();
    vcx.simulate_click(at, Modifiers::default());
    vcx.run_until_parked();
}

/// The × of the active, rightmost tab closes it and the tab left of it shows, with the
/// keyboard the closed tab had. The tab's own click must not run too: it would make the
/// closed tab's index (now past the end) the active one, and the dock would say "No
/// terminals open" while a tab is left.
#[gpui_kit::test]
fn the_close_button_of_the_last_tab_shows_the_one_left(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let first = active(cx, &panel);
    open_tabs(cx, &panel, 1);
    click_tab(cx, w, &panel, 1);
    click_close(cx, w, 1);
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.tab_count(), 1);
        assert_eq!(p.active_index(), 0, "the active tab is past the end");
        assert!(
            p.text(None, None, 5, cx).is_ok(),
            "reading the active tab fails"
        );
    });
    assert!(
        has_keyboard(cx, w, &first),
        "the keyboard didn't go to the tab left"
    );
}

/// The × of a tab in the background closes only that tab, on either side of the active
/// one: the active tab stays, and so does the keyboard.
#[gpui_kit::test]
fn the_close_button_of_a_background_tab_keeps_the_active_one(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 3);
    // [A, B*, C, D], B with the keyboard.
    click_tab(cx, w, &panel, 1);
    let b = active(cx, &panel);
    for (ix, closing) in [
        (2, "C, right of B"),
        (0, "A, left of B"),
        (1, "D, the last"),
    ] {
        click_close(cx, w, ix);
        assert!(active(cx, &panel) == b, "closing {closing} switched tabs");
        assert!(
            has_keyboard(cx, w, &b),
            "closing {closing} took the keyboard"
        );
    }
    cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 1));
}

/// The × of the active tab while it has the keyboard hands the keyboard to the tab that
/// takes its place.
#[gpui_kit::test]
fn the_close_button_of_the_typing_tab_hands_on_the_keyboard(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    // [A*, B], A with the keyboard.
    click_tab(cx, w, &panel, 0);
    let b = cx.update(|cx| panel.read(cx).tab(1).clone());
    click_close(cx, w, 0);
    assert!(active(cx, &panel) == b);
    assert!(has_keyboard(cx, w, &b), "the keyboard went nowhere");
}

/// A tab's menu acts on the tab it was opened for, even when the strip changes while it is
/// open: here an agent closes a tab left of it, and the menu's Close still closes its tab,
/// not the one that moved into its place.
#[gpui_kit::test]
fn a_tabs_menu_closes_the_tab_it_was_opened_for(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 3);
    let [a, b, c, d] = cx.update(|cx| {
        let p = panel.read(cx);
        [0, 1, 2, 3].map(|ix| p.tab(ix).clone())
    });
    // Right-click B in the strip.
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    let at = vcx
        .debug_bounds("term-tab-1")
        .expect("tab 1 is drawn")
        .center();
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    vcx.run_until_parked();
    // An agent closes A while the menu is open.
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.close_tab(0, None, window, cx))
    })
    .expect("the test window is open")
    .expect("tab 0 closes");
    cx.run_until_parked();
    // The menu's last item is Close.
    cx.simulate_keystrokes(w, "up enter");
    cx.run_until_parked();
    let left: Vec<Entity<TerminalTab>> = cx.update(|cx| {
        let p = panel.read(cx);
        (0..p.tab_count()).map(|ix| p.tab(ix).clone()).collect()
    });
    assert!(!left.contains(&a));
    assert!(!left.contains(&b), "the menu's Close didn't close its tab");
    assert_eq!(left, [c.clone(), d], "the menu's Close closed another tab");

    // [C, D*] with the keyboard in D: closing D from its menu hands the keyboard to C.
    click_tab(cx, w, &panel, 1);
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    let at = vcx
        .debug_bounds("term-tab-1")
        .expect("tab 1 is drawn")
        .center();
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    vcx.run_until_parked();
    cx.simulate_keystrokes(w, "up enter");
    cx.run_until_parked();
    assert!(active(cx, &panel) == c);
    assert!(has_keyboard(cx, w, &c), "the keyboard went nowhere");
}

/// Right-clicks tab `ix` in the strip, opening its menu.
fn open_tab_menu(cx: &mut TestAppContext, w: AnyWindowHandle, ix: usize) {
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    let at = vcx
        .debug_bounds(selector(format!("term-tab-{ix}")))
        .expect("the tab is drawn")
        .center();
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    vcx.run_until_parked();
}

/// Double-clicks tab `ix` in the strip.
fn double_click_tab(cx: &mut TestAppContext, w: AnyWindowHandle, ix: usize) {
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    let position = vcx
        .debug_bounds(selector(format!("term-tab-{ix}")))
        .expect("the tab is drawn")
        .center();
    for click_count in [1, 2] {
        vcx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        vcx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count,
        });
    }
    vcx.run_until_parked();
}

/// Types `name` in the open rename field and presses Enter.
fn enter_name(cx: &mut TestAppContext, w: AnyWindowHandle, name: &str) {
    if !name.is_empty() {
        cx.simulate_input(w, name);
    } else {
        cx.simulate_keystrokes(w, "backspace");
    }
    cx.simulate_keystrokes(w, "enter");
    cx.run_until_parked();
}

/// A tab's menu starts with Rename Tab, which names the tab it was opened for and shows it;
/// a double-click on a tab does the same. An empty name gives the tab its pane's title back.
#[gpui_kit::test]
fn a_tabs_menu_and_a_double_click_rename_the_tab(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    let [a, b] = cx.update(|cx| {
        let p = panel.read(cx);
        [0, 1].map(|ix| p.tab(ix).clone())
    });
    assert!(active(cx, &panel) == b);

    open_tab_menu(cx, w, 0);
    cx.simulate_keystrokes(w, "down enter");
    cx.run_until_parked();
    enter_name(cx, w, "server");
    assert!(active(cx, &panel) == a, "the renamed tab isn't shown");
    cx.update(|cx| {
        assert_eq!(a.read(cx).state.name.as_deref(), Some("server"));
        assert_eq!(a.read(cx).title(cx), "server");
        assert_eq!(b.read(cx).state.name, None, "another tab was renamed");
    });
    assert!(
        has_keyboard(cx, w, &a),
        "the keyboard didn't go back to the tab"
    );
    double_click_tab(cx, w, 1);
    enter_name(cx, w, "db");
    assert!(active(cx, &panel) == b);
    cx.update(|cx| assert_eq!(b.read(cx).state.name.as_deref(), Some("db")));

    double_click_tab(cx, w, 0);
    enter_name(cx, w, "");
    cx.update(|cx| {
        let t = a.read(cx);
        assert_eq!(t.state.name, None, "an empty name didn't clear it");
        assert_eq!(
            t.title(cx),
            t.pane_title(t.state.focused, t.focused_view().read(cx))
        );
    });
}

/// Closing a tab left of the active one keeps the active tab (and the keyboard in it), as
/// when an agent's `terminal_close` tidies up while the user types in a later tab.
#[gpui_kit::test]
fn closing_a_tab_left_of_the_active_one_keeps_it(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 3);
    // [A, B, C*, D], C with the keyboard.
    click_tab(cx, w, &panel, 2);
    let c = active(cx, &panel);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.close_tab(0, None, window, cx))
    })
    .expect("the test window is open")
    .expect("tab 0 closes");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 3));
    assert!(active(cx, &panel) == c, "the active tab moved off C");
    assert!(has_keyboard(cx, w, &c));
}

/// Asserts that the active tab is drawn whole inside the strip, as the last frame drew them,
/// its scroll included; `what` says which tab that is.
fn assert_active_in_view(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
    what: &str,
) {
    let ix = cx.update(|cx| panel.read(cx).active_index());
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    let strip = vcx.debug_bounds("term-tabs").expect("the strip is drawn");
    let tab = vcx
        .debug_bounds(selector(format!("term-tab-{ix}")))
        .unwrap_or_else(|| panic!("{what}: tab {ix} isn't drawn"));
    let slack = gpui_kit::px(0.5);
    assert!(
        tab.left() >= strip.left() - slack && tab.right() <= strip.right() + slack,
        "{what}: tab {ix} is at x {:?}..{:?}, out of the strip's {:?}..{:?}",
        tab.left(),
        tab.right(),
        strip.left(),
        strip.right(),
    );
}

/// SMOKE-2: the strip scrolls the active tab into view, however it became the active one: a
/// new tab opened past the strip's end, a tab activated left of what shows, a new tab that
/// takes the active tab's index, and a tab after a folded group. The strip draws a header in
/// front of each project's tabs and a folded group's other tabs not at all, so the scroll goes
/// by the tab's place among what is drawn, not by its index.
#[gpui_kit::test]
fn the_strip_scrolls_the_active_tab_into_view(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel_sized(cx, 420., 300.);
    let (alpha, beta) = (scratch_dir("strip-alpha"), scratch_dir("strip-beta"));
    let projects = vec![alpha.clone(), beta.clone()];
    // `n` new tabs in `project`, the way ⌘T opens them there.
    let open_in = |cx: &mut TestAppContext, project: &std::path::PathBuf, n: usize| {
        let (projects, project) = (projects.clone(), project.clone());
        cx.update(|cx| panel.update(cx, |p, cx| p.set_projects(projects, Some(project), cx)));
        open_tabs(cx, &panel, n);
    };

    // Alpha's tabs, then more of beta's than the strip holds.
    open_in(cx, &alpha, 2);
    open_in(cx, &beta, 8);
    assert_active_in_view(cx, w, &panel, "the newest tab");

    // Beta's first tab, by now left of what the strip shows.
    let first_beta = cx.update(|cx| {
        let p = panel.read(cx);
        (0..p.tab_count())
            .find(|&ix| p.tab(ix).read(cx).group.as_ref() == Some(&beta))
            .expect("beta has tabs")
    });
    click_tab(cx, w, &panel, first_beta);
    assert_active_in_view(cx, w, &panel, "a tab left of what shows");

    // A new tab of alpha goes at the end of alpha's tabs: at the active tab's index.
    open_in(cx, &alpha, 1);
    cx.update(|cx| assert_eq!(panel.read(cx).active_index(), first_beta));
    assert_active_in_view(cx, w, &panel, "a new tab at the active tab's index");

    // Beta's newest tab, with alpha folded: alpha draws its header alone.
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.collapsed.insert(alpha.clone());
            cx.notify();
        })
    });
    open_in(cx, &beta, 1);
    assert_active_in_view(cx, w, &panel, "a tab after a folded group");
}
