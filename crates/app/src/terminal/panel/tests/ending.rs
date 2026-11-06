//! Closing a project closes its terminals (item 12): its tabs go, everything their shells
//! started stops, even what ignores the hang-up and SIGTERM, and a tab of no project stays.

use std::time::Duration;

use gpui_kit::{AppContext as _, TestAppContext};

use super::open_tabs;
use crate::terminal::testing::{init, open_panel, ready, scratch_dir, start, wait_until};

/// Whether a process whose command line has `pattern` runs.
fn runs(pattern: &str) -> bool {
    std::process::Command::new("/usr/bin/pgrep")
        .args(["-f", pattern])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[gpui_kit::test]
fn closing_a_project_stops_everything_its_terminals_started(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let project = scratch_dir("ending-project");
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.set_projects(vec![project.clone()], Some(project.clone()), cx)
        })
    });
    // A second tab, then out of any project: it must survive.
    open_tabs(cx, &panel, 1);
    let other = cx.update(|cx| panel.read(cx).tab(1).downgrade());
    cx.update(|cx| panel.update(cx, |p, cx| p.move_to_group(&other, None, cx)));
    let view = cx.update(|cx| {
        let p = panel.read(cx);
        (0..p.tab_count())
            .map(|ix| p.tab(ix))
            .find(|t| t.read(cx).group.as_ref() == Some(&project))
            .expect("a tab of the project")
            .read(cx)
            .focused_view()
            .clone()
    });
    ready(cx, &view);
    // A stubborn background job, which ignores SIGHUP and SIGTERM, and a foreground one.
    let id = std::process::id();
    let (bg, fg) = (format!("sleep 3{id}1"), format!("sleep 3{id}2"));
    start(
        cx,
        &view,
        &format!("(trap '' HUP TERM; {bg}; true) & {fg}"),
        "sleep",
    );
    wait_until(cx, "both sleeps run", |_| runs(&bg) && runs(&fg));
    let running = cx.update(|cx| panel.read(cx).project_terminals(&project, cx));
    assert_eq!(running.tabs, 1);
    assert_eq!(running.running, vec!["sleep".to_string()]);

    // The test's own handle would keep the terminal alive past its tab.
    drop(view);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.close_project(&project, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| panel.read(cx).tab_count()), 1);
    assert!(other.upgrade().is_some(), "the tab of no project closed");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while runs(&bg) || runs(&fg) {
        assert!(
            std::time::Instant::now() < deadline,
            "left running: {bg}: {}, {fg}: {}",
            runs(&bg),
            runs(&fg)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
