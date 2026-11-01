//! `run` and an agent's new terminals: commands go only into a shell at its prompt, never into
//! the one the user types in, and leave the user's tab and keyboard alone.

use gpui_kit::{AppContext as _, TestAppContext};

use super::super::{JobTag, Origin};
use super::{active, click_tab, has_keyboard, open_tabs, test_remote};
use crate::terminal::testing::{focus_terminal, init, open_panel, ready, start_cat, wait_until};

/// While the user types in a terminal (here into `cat`, standing in for Claude Code), an
/// agent's command runs in another shell without switching tabs or taking the keyboard:
/// the rest of what the user types still goes where they were typing.
#[gpui_kit::test]
fn an_agents_command_leaves_the_typing_user_alone(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    click_tab(cx, w, &panel, 0);
    let (user, other) = cx.update(|cx| {
        let p = panel.read(cx);
        (p.tab(0).clone(), p.tab(1).read(cx).focused_view().clone())
    });
    let typing = cx.update(|cx| user.read(cx).focused_view().clone());
    ready(cx, &typing);
    ready(cx, &other);
    start_cat(cx, &typing);

    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.run("printf 'sawit-%s\\n' agent", Origin::Agent, window, cx)
        })
    })
    .expect("the test window is open")
    .expect("a shell runs it");
    cx.run_until_parked();
    wait_until(cx, "the agent's command ran in the other shell", |cx| {
        other.read(cx).text_tail(50).contains("sawit-agent")
    });
    assert!(active(cx, &panel) == user, "the agent switched tabs");
    assert!(has_keyboard(cx, w, &user), "the agent took the keyboard");
    cx.simulate_keystrokes(w, "x y x y");
    wait_until(cx, "the user's keys reach cat", |cx| {
        typing.read(cx).text_tail(50).contains("xyxy")
    });
    cx.update(|cx| assert!(!other.read(cx).text_tail(50).contains("xyxy")));
}

/// While the user types at a shell's prompt, a line they haven't sent yet, an agent's command
/// runs in another idle shell, else in a new tab behind theirs, and never in the user's: there
/// it would join their half-typed line and run with it on the agent's Enter, or, at an empty
/// prompt, run in front of them and take their next keys. Their line stays, and runs as typed.
#[gpui_kit::test]
fn an_agents_command_never_types_into_the_shell_the_user_types_in(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let user = active(cx, &panel);
    let typing = cx.update(|cx| user.read(cx).focused_view().clone());
    ready(cx, &typing);
    cx.simulate_keystrokes(w, "e c h o space x y x y");
    wait_until(cx, "the user's line shows what they typed", |cx| {
        typing.read(cx).text_tail(5).contains("echo xyxy")
    });
    let agent = |cx: &mut TestAppContext, word: &str| {
        let command = format!("printf 'sawit-%s\\n' {word}");
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.run(&command, Origin::Agent, window, cx))
        })
        .expect("the test window is open")
        .expect("a shell runs it")
    };

    // The user's shell is the only one, so the command gets a tab of its own.
    let (ix, pane) = agent(cx, "agent-first");
    assert_ne!(
        ix, 0,
        "the agent's command went into the shell the user types in"
    );
    let shell = cx
        .update(|cx| panel.read(cx).tab(ix).read(cx).view(pane).cloned())
        .expect("the agent's pane");
    wait_until(cx, "the agent's command ran, and its shell is idle", |cx| {
        let v = shell.read(cx);
        v.text_tail(50).contains("sawit-agent-first") && v.idle_shell()
    });
    // That shell is idle now, and takes the next command: no third tab.
    assert_eq!(
        agent(cx, "agent-second"),
        (ix, pane),
        "the next command didn't go to the idle shell"
    );
    wait_until(cx, "the next command ran", |cx| {
        shell.read(cx).text_tail(50).contains("sawit-agent-second")
    });
    cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 2));
    assert!(active(cx, &panel) == user, "the agent switched tabs");
    assert!(has_keyboard(cx, w, &user), "the agent took the keyboard");
    cx.update(|cx| {
        assert!(
            !typing.read(cx).text_tail(50).contains("agent"),
            "the agent's text reached the user's shell"
        )
    });

    cx.simulate_keystrokes(w, "enter");
    wait_until(cx, "the user's line ran as typed", |cx| {
        typing
            .read(cx)
            .text_tail(50)
            .lines()
            .any(|l| l.trim_end() == "xyxy")
    });
}

/// With the keyboard outside the terminal (an editor, say), an agent's command shows the
/// tab it runs in but leaves the keyboard where it is.
#[gpui_kit::test]
fn an_agents_command_shows_its_tab_but_leaves_the_keyboard(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    let target = cx.update(|cx| panel.read(cx).tab(0).clone());
    let shell = cx.update(|cx| target.read(cx).focused_view().clone());
    // Tab 1 is active and busy, so the command goes to tab 0.
    let busy = cx.update(|cx| panel.read(cx).tab(1).read(cx).focused_view().clone());
    ready(cx, &shell);
    ready(cx, &busy);
    start_cat(cx, &busy);
    // The keyboard is elsewhere in the window.
    let editor = cx.update(|cx| cx.focus_handle());
    cx.update_window(w, |_, window, cx| window.focus(&editor, cx))
        .expect("the test window is open");

    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.run("printf 'sawit-%s\\n' agent", Origin::Agent, window, cx)
        })
    })
    .expect("the test window is open")
    .expect("a shell runs it");
    cx.run_until_parked();
    wait_until(cx, "the agent's command ran", |cx| {
        shell.read(cx).text_tail(50).contains("sawit-agent")
    });
    assert!(
        active(cx, &panel) == target,
        "the tab it runs in isn't shown"
    );
    let kept = cx
        .update_window(w, |_, window, _| editor.is_focused(window))
        .expect("the test window is open");
    assert!(kept, "the agent took the keyboard");
}

/// An agent's new terminal (terminal_run with new_tab, a Run panel script, an SSH
/// session) opens beside the tab the user types in, without switching to it or taking the
/// keyboard.
#[gpui_kit::test]
fn an_agents_new_terminals_open_behind_the_typing_user(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let user = active(cx, &panel);
    let check = |cx: &mut TestAppContext, what: &str| {
        cx.run_until_parked();
        assert!(active(cx, &panel) == user, "{what} switched tabs");
        assert!(has_keyboard(cx, w, &user), "{what} took the keyboard");
    };
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.run_in_new(None, "true", Origin::Agent, window, cx)
        })
    })
    .expect("the test window is open")
    .expect("a new shell opens");
    check(cx, "a new tab");
    let tag = JobTag {
        key: "dev".into(),
        name: "dev".into(),
    };
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.start_job(
                tag,
                std::env::temp_dir(),
                "true",
                Some(Origin::Agent),
                window,
                cx,
            )
        })
    })
    .expect("the test window is open")
    .expect("the script starts");
    check(cx, "a script");
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.open_ssh(test_remote(), Origin::Agent, window, cx)
        })
    })
    .expect("the test window is open")
    .expect("the session starts");
    check(cx, "an SSH session");
    cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 4));
}

/// When no shell can start for a command, `run` says why instead of naming a terminal: an
/// agent would otherwise read the terminal that was active (cat's) as its command's output.
#[gpui_kit::test]
fn run_says_when_no_shell_can_start(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let busy = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    ready(cx, &busy);
    start_cat(cx, &busy);
    cx.update(|cx| panel.update(cx, |p, _| p.fail_spawns = true));

    let ran = cx
        .update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run("printf 'sawit-%s\\n' lost", Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open");
    assert!(ran.is_err(), "run named terminal {ran:?}");
    cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 1));
    cx.update(|cx| assert!(!busy.read(cx).text_tail(50).contains("lost")));
}

/// The pane `run` typed into, by its place in the tab (as app_state lists panes), is what
/// `terminal_run` reports and reads back, rather than the tab's focused pane.
#[gpui_kit::test]
fn run_names_the_pane_it_typed_into(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let (first, second) = cx.update(|cx| {
        let tab = panel.read(cx).tab(0).read(cx);
        let panes = tab.state.layout.panes();
        (
            tab.view(panes[0]).unwrap().clone(),
            tab.view(panes[1]).unwrap().clone(),
        )
    });
    ready(cx, &first);
    ready(cx, &second);
    // The user types in the second pane, busy with cat; the agent's command goes to the
    // first, which doesn't take the tab's focus while the user types.
    start_cat(cx, &second);
    let (ix, pane) = cx
        .update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run("printf 'sawit-%s\\n' here", Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("a shell runs it");
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!((ix, p.pane_index(ix, pane, cx)), (0, Some(0)));
        assert_ne!(
            p.tab(0).read(cx).state.focused,
            pane,
            "the agent took the focus"
        );
    });
    wait_until(cx, "the command ran in the first pane", |cx| {
        panel
            .read(cx)
            .text(Some(0), Some(0), 50, cx)
            .is_ok_and(|t| t.contains("sawit-here"))
    });
}

/// `run` (terminal_run without new_tab, the Tools page's Run in Terminal, a preview's Run)
/// types into the focused pane while its shell is at its prompt, and never into a program
/// running there: with `cat` in the foreground, the command runs in a new shell tab and `cat`
/// gets nothing. Each command prints a word that its own line doesn't contain.
#[gpui_kit::test]
fn run_types_only_into_a_shell_at_its_prompt(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let run = |cx: &mut TestAppContext, commands: &str| {
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.run(commands, Origin::User, window, cx))
        })
        .expect("the test window is open")
        .expect("a shell runs it");
    };
    let first = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    wait_until(cx, "the shell is at its prompt", |cx| {
        first.read(cx).at_prompt()
    });

    run(cx, "printf 'sawit-%s\\n' idle-ran");
    cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 1));
    wait_until(cx, "the shell at its prompt runs the command", |cx| {
        first.read(cx).text_tail(50).contains("sawit-idle-ran")
    });

    run(cx, "cat");
    wait_until(cx, "cat runs in the foreground", |cx| {
        let v = first.read(cx);
        v.foreground_job().is_some() && !v.at_prompt()
    });
    run(cx, "printf 'sawit-%s\\n' busy-ran");
    let fresh = cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.tab_count(), 2, "the command opens a new shell tab");
        assert_eq!(p.active_index(), 1);
        p.tab(1).read(cx).focused_view().clone()
    });
    wait_until(cx, "the new shell runs the command", |cx| {
        fresh.read(cx).text_tail(50).contains("sawit-busy-ran")
    });
    cx.update(|cx| {
        let busy = first.read(cx);
        assert!(
            !busy.text_tail(50).contains("busy-ran"),
            "cat got the command"
        );
        assert!(busy.foreground_job().is_some(), "cat still runs");
    });
}
