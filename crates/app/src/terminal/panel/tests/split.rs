//! An agent's splits (`terminal_split`): beside the pane the user types in, of a tab never drawn,
//! and refused when there is no room.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, Focusable as _, TestAppContext};
use sawit_core::terminal::SplitDir;

use super::super::{Origin, TerminalPanel};
use super::{active, has_keyboard};
use crate::terminal::testing::{
    focus_terminal, init, open_panel, open_panel_sized, ready, scratch_dir, wait_until,
};

/// An agent splits the tab the user types in, `pane` of tab `ix` (from the test window `w`),
/// with `command` in the new half.
fn agent_split(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
    (ix, pane): (usize, Option<usize>),
    dir: SplitDir,
    command: Option<&str>,
) -> Result<(usize, sawit_core::terminal::PaneId), String> {
    let split = cx
        .update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.split(ix, pane, dir, None, command, Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open");
    cx.run_until_parked();
    split
}

/// An agent's split (terminal_split) runs its command in the new half, beside the shell the
/// user types in: the keyboard and the tab's focus stay with the user's pane, so their next
/// keys still go there, and the answer names the new pane by its place in the tab, as
/// app_state lists panes. Without a folder, the new shell starts in the split pane's, as ⌘D's
/// does.
#[gpui_kit::test]
fn an_agents_split_runs_beside_the_user_and_leaves_the_keyboard(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let dir = scratch_dir("agent-split");
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.open_shell_in(dir.clone(), window, cx))
    })
    .expect("the test window is open");
    cx.run_until_parked();
    let user = active(cx, &panel);
    let (ix, users_pane, typing) = cx.update(|cx| {
        let p = panel.read(cx);
        let t = user.read(cx);
        (p.active_index(), t.state.focused, t.focused_view().clone())
    });
    ready(cx, &typing);
    wait_until(cx, "the user's shell runs in the folder", |cx| {
        typing.read(cx).cwd().as_ref() == Some(&dir)
    });
    assert!(has_keyboard(cx, w, &user), "the user isn't typing there");

    let (at, new) = agent_split(
        cx,
        w,
        &panel,
        (ix, None),
        SplitDir::Right,
        Some("printf 'sawit-%s\\n' split"),
    )
    .expect("the pane splits");
    let fresh = cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!((at, p.pane_index(at, new, cx)), (ix, Some(1)));
        assert!(p.tab(ix) == &user, "the split went to another tab");
        let t = user.read(cx);
        assert_eq!(t.views().count(), 2);
        assert_eq!(
            t.state.focused, users_pane,
            "the split took the tab's focus"
        );
        t.view(new).expect("the new pane").clone()
    });
    let typed = cx
        .update_window(w, |_, window, cx| {
            typing.focus_handle(cx).is_focused(window)
        })
        .expect("the test window is open");
    assert!(typed, "the agent's split took the keyboard");
    wait_until(cx, "the command ran in the new pane", |cx| {
        panel
            .read(cx)
            .text(Some(ix), Some(1), 50, cx)
            .is_ok_and(|text| text.contains("sawit-split"))
    });
    wait_until(cx, "the new shell runs in the split pane's folder", |cx| {
        fresh.read(cx).cwd().as_ref() == Some(&dir)
    });
    cx.simulate_keystrokes(w, "x y x y");
    wait_until(cx, "the user's keys reach their shell", |cx| {
        typing.read(cx).text_tail(5).contains("xyxy")
    });
    cx.update(|cx| assert!(!fresh.read(cx).text_tail(50).contains("xyxy")));
}

/// A tab an agent opened behind the one the user types in has never been drawn, so it has no
/// size of its own: it is as big as the dock's other tabs, and splits. With the dock never on
/// screen at all there is no size to go by, and the split says so instead of starting a
/// shell.
#[gpui_kit::test]
fn an_agents_split_of_a_tab_never_drawn_takes_the_docks_size(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let user = active(cx, &panel);
    let (behind, _) = cx
        .update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run_in_new(None, "true", Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("a new shell opens");
    cx.run_until_parked();
    cx.update(|cx| {
        let p = panel.read(cx);
        assert!(p.tab(behind) != &user, "the agent's tab came in front");
        assert_eq!(p.tab(behind).read(cx).area(), None, "the tab was drawn");
    });
    let (at, new) = agent_split(cx, w, &panel, (behind, None), SplitDir::Down, None)
        .expect("the tab behind splits");
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!((at, p.pane_index(at, new, cx)), (behind, Some(1)));
        assert_eq!(p.tab(behind).read(cx).views().count(), 2);
    });
    assert!(active(cx, &panel) == user, "the split switched tabs");
    assert!(has_keyboard(cx, w, &user), "the split took the keyboard");

    // A panel whose dock was never drawn: the window shows another one.
    let hidden = cx.update(|cx| cx.new(TerminalPanel::new));
    let refused = agent_split(cx, w, &hidden, (0, None), SplitDir::Right, Some("true"))
        .expect_err("a split of a dock never drawn");
    assert!(refused.contains("hasn't been on screen"), "{refused}");
    cx.update(|cx| assert_eq!(hidden.read(cx).tab(0).read(cx).views().count(), 1));
}

/// A split that would leave a half under 20 columns by 4 rows is refused, as ⌘D's is, and
/// says why; nothing is started. A split tab needs the pane to split named, and a pane it
/// doesn't have is an error.
#[gpui_kit::test]
fn a_split_with_no_room_says_why_and_starts_nothing(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel_sized(cx, 280., 600.);
    let refused = agent_split(cx, w, &panel, (0, None), SplitDir::Right, Some("true"))
        .expect_err("a split with no room");
    assert!(refused.contains("20 columns by 4 rows"), "{refused}");
    assert!(refused.contains("nothing was started"), "{refused}");
    cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).views().count(), 1));

    agent_split(cx, w, &panel, (0, None), SplitDir::Down, None).expect("room to stack");
    let unnamed = agent_split(cx, w, &panel, (0, None), SplitDir::Down, None)
        .expect_err("a split tab with no pane named");
    assert!(unnamed.contains("name the pane"), "{unnamed}");
    let missing = agent_split(cx, w, &panel, (0, Some(5)), SplitDir::Down, None)
        .expect_err("a pane the tab doesn't have");
    assert!(missing.contains("no pane 5"), "{missing}");
    let tab =
        agent_split(cx, w, &panel, (3, None), SplitDir::Down, None).expect_err("a tab there isn't");
    assert!(tab.contains("no terminal 3"), "{tab}");
    cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).views().count(), 2));
}
