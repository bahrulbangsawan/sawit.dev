//! The Run panel's scripts in the terminals: their shells left alone between runs, a script run
//! again in its folder, found in a split, and where an agent's script runs.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use sawit_core::terminal::{run_bytes, shell_quote};

use super::super::{JobTag, Origin};
use super::active;
use crate::terminal::testing::{
    focus_terminal, init, open_panel, ready, scratch_dir, start_cat, wait_until,
};
use crate::terminal::view::TerminalView;

/// `run` never types into a Run panel script's terminal, even once the script has ended
/// and its shell waits at its prompt: the script would then read as running again, and
/// its Stop would interrupt the other command.
#[gpui_kit::test]
fn run_leaves_a_finished_scripts_shell_alone(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &shell);
    let tag = JobTag {
        key: "build".into(),
        name: "build".into(),
    };
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.start_job(
                tag,
                std::env::temp_dir(),
                "printf 'sawit-%s\\n' built",
                Some(Origin::User),
                window,
                cx,
            )
        })
    })
    .expect("the test window is open")
    .expect("the script starts");
    let job = cx.update(|cx| {
        let p = panel.read(cx);
        p.tab(p.active_index()).read(cx).focused_view().clone()
    });
    wait_until(cx, "the script ran and its shell is at its prompt", |cx| {
        let v = job.read(cx);
        v.text_tail(50).contains("sawit-built") && v.at_prompt()
    });

    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.run("printf 'sawit-%s\\n' other", Origin::User, window, cx)
        })
    })
    .expect("the test window is open")
    .expect("a shell runs it");
    wait_until(cx, "the command ran", |cx| {
        let ran = |v: &Entity<TerminalView>| v.read(cx).text_tail(50).contains("sawit-other");
        ran(&shell) || ran(&job)
    });
    cx.update(|cx| {
        assert!(
            !job.read(cx).text_tail(50).contains("sawit-other"),
            "the command went into the script's terminal"
        );
        assert!(shell.read(cx).text_tail(50).contains("sawit-other"));
        let status = panel
            .read(cx)
            .job_status("build", cx)
            .expect("the script's tab");
        assert_eq!(status.running, None, "the script reads as running");
    });
}

/// Running a script again types it into its old shell after a `cd` back to the script's
/// folder: its command is relative (`pnpm run dev`, `make build`) and the shell may have
/// moved since.
#[gpui_kit::test]
fn running_a_script_again_goes_back_to_its_folder(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let home = scratch_dir("script-home");
    let elsewhere = scratch_dir("script-elsewhere");
    let start = |cx: &mut TestAppContext, command: &str| {
        let tag = JobTag {
            key: "build".into(),
            name: "build".into(),
        };
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.start_job(tag, home.clone(), command, Some(Origin::User), window, cx)
            })
        })
        .expect("the test window is open")
        .expect("the script starts");
    };
    start(cx, "printf 'sawit-%s\\n' first");
    let job = cx.update(|cx| {
        let p = panel.read(cx);
        p.tab(p.active_index()).read(cx).focused_view().clone()
    });
    wait_until(cx, "the script ran in its folder", |cx| {
        let v = job.read(cx);
        v.text_tail(50).contains("sawit-first") && v.at_prompt() && v.cwd().as_ref() == Some(&home)
    });
    let cd = format!("cd {}", shell_quote(&elsewhere));
    cx.update(|cx| job.read(cx).write(run_bytes(&cd)));
    wait_until(cx, "the script's shell moved", |cx| {
        let v = job.read(cx);
        v.at_prompt() && v.cwd().as_ref() == Some(&elsewhere)
    });

    start(cx, "printf 'sawit-%s\\n' second");
    wait_until(cx, "the script ran again", |cx| {
        let v = job.read(cx);
        v.text_tail(50).contains("sawit-second") && v.at_prompt()
    });
    cx.update(|cx| {
        assert_eq!(
            job.read(cx).cwd(),
            Some(home.clone()),
            "the script ran where its shell had gone"
        )
    });
}

/// A script an agent starts while the user types in a terminal opens behind it, and says where
/// it runs: app_run_script names that terminal, which the agent's terminal_read and
/// terminal_input need, since the terminal in front is still the user's. A script whose shell
/// can't start says why instead.
#[gpui_kit::test]
fn an_agents_script_says_where_it_runs(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let user = active(cx, &panel);
    let typing = cx.update(|cx| user.read(cx).focused_view().clone());
    ready(cx, &typing);
    start_cat(cx, &typing);
    let start = |cx: &mut TestAppContext, key: &str| {
        let tag = JobTag {
            key: key.into(),
            name: key.into(),
        };
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.start_job(
                    tag,
                    std::env::temp_dir(),
                    "sleep 30",
                    Some(Origin::Agent),
                    window,
                    cx,
                )
            })
        })
        .expect("the test window is open")
    };

    let (ix, pane) = start(cx, "dev").expect("the script starts");
    assert!(active(cx, &panel) == user, "the script came in front");
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.pane_index(ix, pane, cx), Some(0));
        assert_eq!(p.job_terminal("dev", cx), Some((ix, 0)));
    });
    let script = cx.update(|cx| panel.read(cx).tab(ix).read(cx).focused_view().clone());
    wait_until(cx, "the script runs", |cx| {
        script.read(cx).foreground_name().as_deref() == Some("sleep")
    });
    cx.update(|cx| panel.update(cx, |p, cx| p.input(Some(ix), Some(0), vec![0x03], cx)))
        .expect("the script's terminal takes the keys");
    wait_until(cx, "the script stops", |cx| script.read(cx).at_prompt());
    cx.update(|cx| {
        assert_eq!(typing.read(cx).foreground_name().as_deref(), Some("cat"));
    });

    cx.update(|cx| panel.update(cx, |p, _| p.fail_spawns = true));
    let failed = start(cx, "build");
    assert!(
        failed.is_err(),
        "a script with no shell started: {failed:?}"
    );
    cx.update(|cx| assert_eq!(panel.read(cx).job_terminal("build", cx), None));
}

#[gpui_kit::test]
fn a_script_in_a_split_is_still_found(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let tag = JobTag {
        key: "k".into(),
        name: "k".into(),
    };
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.start_job(
                tag,
                std::env::temp_dir(),
                "true",
                Some(Origin::User),
                window,
                cx,
            )
        })
    })
    .expect("the test window is open")
    .expect("the script starts");
    cx.run_until_parked();
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let (job_tab, job_pane, new_pane) = cx.update(|cx| {
        let p = panel.read(cx);
        let tab = p.tab(p.active_index()).read(cx);
        let panes = tab.state.layout.panes();
        assert_eq!(panes.len(), 2);
        assert_eq!(tab.state.focused, panes[1]);
        (p.active_index(), panes[0], panes[1])
    });
    assert_ne!(job_pane, new_pane);
    cx.update(|cx| assert!(panel.read(cx).job_status("k", cx).is_some()));
    let found = cx
        .update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.show_job("k", window, cx))
        })
        .expect("the test window is open");
    assert!(found);
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.active_index(), job_tab);
        assert_eq!(p.tab(job_tab).read(cx).state.focused, job_pane);
    });
}
