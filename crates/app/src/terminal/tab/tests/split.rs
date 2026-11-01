//! Splitting panes, dragging dividers, zooming, and where a new pane starts.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Render, Styled as _, TestAppContext, Window,
    WindowBounds, WindowOptions, div, point, px, size,
};
use sawit_core::terminal::{Layout, MIN_COLS, MIN_ROWS, SplitDir};

use crate::terminal::TerminalPanel;
use crate::terminal::tab::Drag;
use crate::terminal::tab::panes::min_size;
use crate::terminal::testing::{focus_terminal, init, open_panel, scratch_dir, wait_until};
use crate::terminal::view::cell_size;

#[gpui_kit::test]
fn a_new_panel_has_one_tab_of_one_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (_w, panel) = open_panel(cx);
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.tab_count(), 1);
        let tab = p.tab(0).read(cx);
        assert_eq!(tab.views().count(), 1);
        assert!(!tab.state.is_split());
    });
}

#[gpui_kit::test]
fn cmd_d_splits_right_and_focuses_the_new_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    cx.update(|cx| {
        let tab = panel.read(cx).tab(0).read(cx);
        assert_eq!(tab.views().count(), 2);
        assert!(matches!(
            tab.state.layout,
            Layout::Split {
                dir: SplitDir::Right,
                ..
            }
        ));
        assert_eq!(tab.state.focused, tab.state.layout.panes()[1]);
    });
}

#[gpui_kit::test]
fn cmd_shift_d_splits_down_and_cmd_alt_arrows_move_focus(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-shift-d");
    cx.run_until_parked();
    let panes = cx.update(|cx| panel.read(cx).tab(0).read(cx).state.layout.panes());
    assert_eq!(panes.len(), 2);
    cx.simulate_keystrokes(w, "cmd-alt-up");
    cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).state.focused, panes[0]));
    cx.simulate_keystrokes(w, "cmd-alt-down");
    cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).state.focused, panes[1]));
}

/// Every pane's grid size in `tab`, after the last paint.
fn grids(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>) -> Vec<(u16, u16)> {
    cx.update(|cx| {
        let tab = panel.read(cx).tab(0).read(cx);
        tab.views()
            .map(|(_, v)| {
                let g = v.read(cx).grid();
                (g.cols, g.rows)
            })
            .collect()
    })
}

fn assert_big_enough(grids: &[(u16, u16)]) {
    for &(cols, rows) in grids {
        assert!(
            cols >= MIN_COLS && rows >= MIN_ROWS,
            "a pane is {cols}×{rows}, under {MIN_COLS}×{MIN_ROWS}"
        );
    }
}

/// Splits with `key` until the tab refuses; returns the pane count.
fn split_until_refused(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
    key: &str,
) -> usize {
    let count =
        |cx: &mut TestAppContext| cx.update(|cx| panel.read(cx).tab(0).read(cx).views().count());
    let mut last = count(cx);
    loop {
        cx.simulate_keystrokes(w, key);
        cx.run_until_parked();
        let now = count(cx);
        if now == last {
            return now;
        }
        assert!(now < 64, "splitting never stopped");
        last = now;
    }
}

#[gpui_kit::test]
fn splitting_stops_before_a_pane_gets_too_small(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let count = split_until_refused(cx, w, &panel, "cmd-shift-d");
    assert!(count > 1, "the tab was never split");
    cx.update(|cx| {
        let tab = panel.read(cx).tab(0).read(cx);
        assert_eq!(tab.views.len(), count, "a refused pane's terminal was kept");
    });
    assert_big_enough(&grids(cx, &panel));

    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    assert!(split_until_refused(cx, w, &panel, "cmd-d") > 1);
    assert_big_enough(&grids(cx, &panel));
}

/// Drags the tab's first divider to `to` (window coordinates) and lets
/// the panes repaint.
fn drag_root_divider(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
    to: impl Fn(Bounds<Pixels>) -> Point<Pixels>,
) {
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    cx.update_window(w, |_, window, cx| {
        let pane_min = min_size(cell_size(window, cx));
        tab.update(cx, |t, cx| {
            let divider = t.state.layout.dividers(t.area().unwrap()).remove(0);
            t.drag = Some(Drag {
                path: divider.path,
                dir: divider.dir,
                span: divider.span,
                pane_min,
            });
            let at = to(t.bounds.get().unwrap());
            t.drag_to(at, cx);
            t.drag = None;
        })
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn dragging_a_divider_keeps_20_columns_and_4_rows(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    drag_root_divider(cx, w, &panel, |b| b.origin);
    assert_big_enough(&grids(cx, &panel));
    drag_root_divider(cx, w, &panel, |b| b.origin + point(b.size.width, px(0.)));
    assert_big_enough(&grids(cx, &panel));

    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-shift-d");
    cx.run_until_parked();
    drag_root_divider(cx, w, &panel, |b| b.origin);
    assert_big_enough(&grids(cx, &panel));
    drag_root_divider(cx, w, &panel, |b| b.origin + point(px(0.), b.size.height));
    assert_big_enough(&grids(cx, &panel));
}

#[gpui_kit::test]
fn a_split_starts_in_the_focused_panes_folder(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let dir = scratch_dir("split-cwd");
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.open_shell_in(dir.clone(), window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    let tab = cx.update(|cx| {
        let p = panel.read(cx);
        p.tab(p.active_index()).clone()
    });
    wait_until(cx, "the shell runs in the folder", |cx| {
        tab.read(cx).focused_view().read(cx).cwd().as_ref() == Some(&dir)
    });

    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let new = cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.views().count(), 2);
        t.focused_view().clone()
    });
    wait_until(cx, "the new pane runs in the folder", |cx| {
        new.read(cx).cwd().as_ref() == Some(&dir)
    });
}

#[gpui_kit::test]
fn a_split_from_a_deleted_folder_starts_in_the_project(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let project = scratch_dir("split-project");
    let gone = project.join("gone");
    std::fs::create_dir(&gone).unwrap();
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.set_projects(vec![project.clone()], Some(project.clone()), cx)
        })
    });
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.open_shell_in(gone.clone(), window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    let tab = cx.update(|cx| {
        let p = panel.read(cx);
        p.tab(p.active_index()).clone()
    });
    cx.update(|cx| assert_eq!(tab.read(cx).group.as_ref(), Some(&project)));
    wait_until(cx, "the shell runs in the inner folder", |cx| {
        tab.read(cx).focused_view().read(cx).cwd().as_ref() == Some(&gone)
    });
    std::fs::remove_dir(&gone).unwrap();

    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let new = cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.views().count(), 2);
        t.focused_view().clone()
    });
    wait_until(cx, "the new pane runs in the project", |cx| {
        new.read(cx).cwd().as_ref() == Some(&project)
    });
}

#[gpui_kit::test]
fn cmd_shift_enter_zooms_a_split_tab(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let focused = cx.update(|cx| tab.read(cx).state.focused);
    cx.simulate_keystrokes(w, "cmd-shift-enter");
    cx.update(|cx| assert_eq!(tab.read(cx).state.zoomed, Some(focused)));
    cx.simulate_keystrokes(w, "cmd-shift-enter");
    cx.update(|cx| assert_eq!(tab.read(cx).state.zoomed, None));
}

/// Stand-in for the workspace around the panel: counts its maximize
/// action.
struct Host {
    panel: Entity<TerminalPanel>,
    maximized: usize,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Workspace")
            .size_full()
            .on_action(
                cx.listener(|this, _: &crate::workspace::ToggleMaximizePane, _, _| {
                    this.maximized += 1
                }),
            )
            .child(self.panel.clone())
    }
}

#[gpui_kit::test]
fn cmd_shift_enter_with_one_pane_reaches_the_workspace(cx: &mut TestAppContext) {
    cx.update(init);
    cx.executor().allow_parking();
    let (w, host) = cx.update(|cx| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|cx| Host {
                panel: cx.new(TerminalPanel::new),
                maximized: 0,
            })
        })
        .unwrap()
    });
    cx.run_until_parked();
    let panel = cx.update(|cx| host.read(cx).panel.clone());
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-shift-enter");
    cx.update(|cx| {
        assert_eq!(
            host.read(cx).maximized,
            1,
            "the workspace did not get the key"
        );
        assert_eq!(panel.read(cx).tab(0).read(cx).state.zoomed, None);
    });
}
