//! Agents' `terminal_run` (B3): an agent's commands go to its own idle terminal, so fifty of
//! them leave one or two tabs; a locked tab is never used; a one-off tab closes itself once its
//! command is done and its output stays readable by the run's number.

use std::path::PathBuf;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::super::TerminalPanel;
use super::super::agent_runs::Ran;
use crate::terminal::testing::{init, open_panel, ready, wait_until};

fn run(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
    command: &str,
    cwd: Option<PathBuf>,
    new_tab: bool,
    close: bool,
) -> Ran {
    let ran = cx
        .update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run_for_agent(
                    command,
                    cwd,
                    None,
                    new_tab,
                    close,
                    Some("agent-1"),
                    window,
                    cx,
                )
            })
        })
        .expect("the test window is open")
        .expect("a shell runs it");
    cx.run_until_parked();
    ran
}

fn tabs(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>) -> usize {
    cx.update(|cx| panel.read(cx).tabs.len())
}

fn text(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>, run: u64) -> String {
    cx.update(|cx| panel.read(cx).run_text(run, 200, cx).unwrap_or_default())
}

#[gpui_kit::test]
fn an_agents_commands_reuse_its_own_idle_terminal(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let first = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &first);
    let home = std::env::temp_dir().canonicalize().unwrap();
    // With a folder and no terminal of its own yet, the agent gets a new tab there.
    let one = run(
        cx,
        w,
        &panel,
        "printf 'step-%s\\n' one",
        Some(home.clone()),
        false,
        false,
    );
    assert!(!one.reused);
    assert_eq!(tabs(cx, &panel), 2);
    wait_until(cx, "the first command ran", |cx| {
        panel
            .read(cx)
            .run_text(one.run, 50, cx)
            .unwrap_or_default()
            .contains("step-one")
    });
    // Its next commands, in the same folder or another, go to that terminal.
    let mut last = one.run;
    for n in 0..12 {
        let cwd = (n % 2 == 0).then(|| home.clone());
        let ran = run(
            cx,
            w,
            &panel,
            &format!("printf 'again-%s\\n' {n}"),
            cwd,
            false,
            false,
        );
        assert!(ran.reused, "command {n} opened a tab");
        assert_eq!(ran.tab, one.tab, "command {n} went to another tab");
        wait_until(cx, "the command ran", |cx| {
            panel
                .read(cx)
                .run_text(ran.run, 50, cx)
                .unwrap_or_default()
                .contains(&format!("again-{n}"))
        });
        last = ran.run;
    }
    assert_eq!(tabs(cx, &panel), 2, "twelve commands left more tabs");
    // In another folder, the reused terminal goes there first.
    let other = run(cx, w, &panel, "pwd", Some(PathBuf::from("/")), false, false);
    assert!(other.reused);
    wait_until(cx, "pwd printed /", |cx| {
        panel
            .read(cx)
            .run_text(other.run, 50, cx)
            .unwrap_or_default()
            .lines()
            .any(|l| l.trim() == "/")
    });
    assert!(text(cx, &panel, last).contains("again-11"));
}

#[gpui_kit::test]
fn a_locked_terminal_is_never_reused(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let first = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &first);
    let one = run(cx, w, &panel, "true", None, true, false);
    let view = cx.update(|cx| panel.read(cx).tab(one.tab).read(cx).focused_view().clone());
    ready(cx, &view);
    cx.update(|cx| panel.update(cx, |p, cx| p.set_locked(one.tab, Some(true), cx)));
    let two = run(
        cx,
        w,
        &panel,
        "true",
        Some(std::env::temp_dir()),
        false,
        false,
    );
    assert!(!two.reused);
    assert_ne!(two.tab, one.tab, "the locked tab was used");
}

#[gpui_kit::test]
fn a_one_off_tab_closes_when_done_and_its_output_stays(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let first = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &first);
    let before = tabs(cx, &panel);
    let ran = run(
        cx,
        w,
        &panel,
        "sleep 1; printf 'one-%s\\n' off",
        None,
        true,
        true,
    );
    assert!(ran.closes);
    assert_eq!(tabs(cx, &panel), before + 1);
    // The tab looks every half second (the test clock's), and closes once the command has
    // been done for two looks in a row (real time: the shell is a real process).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while tabs(cx, &panel) != before {
        assert!(
            std::time::Instant::now() < deadline,
            "the one-off tab stayed open"
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let out = text(cx, &panel, ran.run);
    assert!(out.contains("one-off"), "{out}");
    assert!(out.contains("closed when its command was done"), "{out}");
    // A reused terminal never closes: close_when_done only applies to a tab opened for it.
    let kept = run(cx, w, &panel, "true", None, false, true);
    assert!(!kept.closes || kept.tab >= before);
}
