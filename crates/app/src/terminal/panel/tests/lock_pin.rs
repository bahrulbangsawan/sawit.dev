//! Lock, pin and the bulk closes (item 2): a locked tab doesn't close, a pinned one moves to the
//! start and comes back with its project, and Close Others / Close All keep both.

use gpui_kit::{AppContext as _, TestAppContext};

use super::open_tabs;
use crate::terminal::panel::CloseScope;
use crate::terminal::testing::{init, open_panel, ready, scratch_dir, start};

#[gpui_kit::test]
fn a_locked_tab_does_not_close(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    assert!(cx.update(|cx| panel.update(cx, |p, cx| p.set_locked(0, Some(true), cx))));
    let locked = cx.update(|cx| panel.read(cx).tab(0).downgrade());
    // Its ×, its menu's Close and the pane's close all go through these.
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.close_for_user(&locked, false, window, cx))
    })
    .unwrap();
    assert_eq!(cx.update(|cx| panel.read(cx).tab_count()), 2);
    let notice = cx.update(|cx| panel.read(cx).notice.clone());
    assert!(notice.unwrap().contains("is locked"));
    // An agent's close is refused, confirm or not.
    let refusal = cx
        .update_window(w, |_, window, cx| {
            panel.read(cx).close_refusal(0, None, window, cx)
        })
        .unwrap();
    assert!(refusal.unwrap_err().contains("locked"));
    // Unlocked, it closes.
    cx.update(|cx| panel.update(cx, |p, cx| p.set_locked(0, Some(false), cx)));
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.close_for_user(&locked, false, window, cx))
    })
    .unwrap();
    assert_eq!(cx.update(|cx| panel.read(cx).tab_count()), 1);
}

#[gpui_kit::test]
fn a_pinned_tab_moves_to_the_start_and_bulk_closes_keep_it(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 3);
    let third = cx.update(|cx| panel.read(cx).tab(2).clone());
    let (pinned, at) = cx.update(|cx| panel.update(cx, |p, cx| p.set_pinned(2, Some(true), cx)));
    assert!(pinned);
    assert_eq!(at, 0, "the pinned tab moved to the start");
    assert!(cx.update(|cx| *panel.read(cx).tab(0) == third));
    cx.update(|cx| panel.update(cx, |p, cx| p.set_locked(3, Some(true), cx)));
    // From tab 1: Close Others keeps the pinned and the locked tab.
    let plan = cx.update(|cx| panel.read(cx).bulk_close(CloseScope::Others, 1, cx));
    assert_eq!(plan.closing, vec![2]);
    assert_eq!(plan.kept, 2);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.close_tabs_for_user(CloseScope::All, 1, window, cx)
        })
    })
    .unwrap();
    assert_eq!(cx.update(|cx| panel.read(cx).tab_count()), 2);
    let notice = cx.update(|cx| panel.read(cx).notice.clone());
    assert_eq!(notice.as_deref(), Some("Kept 2 locked or pinned tabs."));
}

/// Close All asks before it stops what runs, naming it; Cancel keeps every tab.
#[gpui_kit::test]
fn close_all_asks_before_stopping_a_running_process(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &view);
    start(cx, &view, "sleep 30", "sleep");
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.close_tabs_for_user(CloseScope::All, 0, window, cx)
        })
    })
    .unwrap();
    let (title, _) = crate::confirm::testing::pending(cx).expect("Close All asked");
    assert_eq!(title, "Close 2 terminals and stop `sleep`?");
    crate::confirm::testing::answer(cx, "Cancel");
    assert_eq!(cx.update(|cx| panel.read(cx).tab_count()), 2);
    drop(view);
}

#[gpui_kit::test]
fn pinned_tabs_come_back_with_their_project(cx: &mut TestAppContext) {
    cx.update(init);
    let (_, panel) = open_panel(cx);
    let project = scratch_dir("pins-project");
    let file = scratch_dir("pins-file").join("terminal-pins.json");
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.set_pins_file(file.clone());
            p.set_projects(vec![project.clone()], Some(project.clone()), cx)
        })
    });
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.tab(0)
                .update(cx, |t, _| t.state.name = Some("dev".into()));
            p.set_locked(0, Some(true), cx);
            p.set_pinned(0, Some(true), cx);
        })
    });
    assert!(file.exists(), "the pin was saved");
    // The project closes (its tabs with it), then opens again: the pinned tab is back.
    cx.update(|cx| panel.update(cx, |p, cx| p.set_projects(vec![], None, cx)));
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            let n = p.tab_count();
            for ix in (0..n).rev() {
                p.close(ix, cx);
            }
            p.set_projects(vec![project.clone()], Some(project.clone()), cx)
        })
    });
    let back = cx.update(|cx| {
        let p = panel.read(cx);
        (0..p.tab_count())
            .map(|ix| p.tab(ix).read(cx))
            .find(|t| t.state.pinned)
            .map(|t| (t.state.name.clone(), t.state.locked, t.group.clone()))
    });
    assert_eq!(back, Some((Some("dev".into()), true, Some(project))));
}
