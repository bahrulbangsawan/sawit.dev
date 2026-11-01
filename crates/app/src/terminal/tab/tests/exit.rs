//! A pane whose process ends: a plain shell in a split closes its pane, anything else keeps
//! it with a banner; an ended SSH session reconnects.

use std::time::{Duration, Instant};

use gpui_kit::{App, AppContext as _, Focusable as _, TestAppContext};

use super::test_remote;
use crate::terminal::testing::{SHELLS, focus_terminal, init, open_panel, wait_until};
use crate::terminal::{JobTag, Origin, Remote};

/// Like `wait_until`, but lets the app handle what the shells sent
/// (output, their exit) while it waits.
fn run_until(cx: &mut TestAppContext, what: &str, ok: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + SHELLS;
    while !cx.update(|cx| ok(cx)) {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(20));
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn an_exited_shell_in_a_split_closes_its_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.views().count(), 2);
        t.focused_view().read(cx).write(b"exit\n".to_vec());
    });
    run_until(cx, "the exited pane closes", |cx| {
        tab.read(cx).views().count() == 1
    });
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.views.len(), 1, "the closed pane's terminal was kept");
        assert!(!t.focused_view().read(cx).exited);
        assert_eq!(panel.read(cx).tab_count(), 1);
    });
    let kept = cx
        .update_window(w, |_, window, cx| tab.read(cx).has_focus(window, cx))
        .unwrap();
    assert!(kept, "the keyboard did not go to the remaining pane");
}

#[gpui_kit::test]
fn an_exited_shell_in_a_background_tab_leaves_the_keyboard_alone(cx: &mut TestAppContext) {
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
        assert_eq!(p.tab_count(), 2);
        p.tab(p.active_index()).clone()
    });
    assert!(active != background, "the new tab is not the active one");
    let typing = cx.update(|cx| active.read(cx).focused_view().focus_handle(cx));
    let has_keyboard = |cx: &mut TestAppContext| {
        cx.update_window(w, |_, window, _| typing.is_focused(window))
            .unwrap()
    };
    assert!(has_keyboard(cx), "the new tab did not take the keyboard");

    cx.update(|cx| {
        background
            .read(cx)
            .focused_view()
            .read(cx)
            .write(b"exit\n".to_vec())
    });
    run_until(cx, "the background pane closes", |cx| {
        background.read(cx).views().count() == 1
    });
    cx.run_until_parked();
    assert!(
        has_keyboard(cx),
        "closing a background pane took the keyboard"
    );
}

#[gpui_kit::test]
fn an_exited_single_pane_keeps_its_tab(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    cx.update(|cx| view.read(cx).write(b"exit\n".to_vec()));
    run_until(cx, "the shell exits", |cx| view.read(cx).exited);
    cx.run_until_parked();
    cx.update(|cx| {
        let p = panel.read(cx);
        assert_eq!(p.tab_count(), 1);
        assert_eq!(p.tab(0).read(cx).views().count(), 1);
    });
}

#[gpui_kit::test]
fn an_exited_script_in_a_split_keeps_its_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let tab = cx.update(|cx| panel.read(cx).tab(0).clone());
    let view = cx.update(|cx| tab.read(cx).focused_view().clone());
    cx.update(|cx| {
        view.update(cx, |v, _| {
            v.job = Some(JobTag {
                key: "dev".into(),
                name: "dev".into(),
            });
            v.write(b"exit\n".to_vec());
        })
    });
    run_until(cx, "the script's shell exits", |cx| view.read(cx).exited);
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(tab.read(cx).views().count(), 2));
}

#[gpui_kit::test]
fn reconnecting_replaces_only_the_ended_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.open_ssh(test_remote(), Origin::User, window, cx)
        })
    })
    .unwrap()
    .expect("the session starts");
    cx.run_until_parked();
    let (ix, tab) = cx.update(|cx| {
        let p = panel.read(cx);
        (p.active_index(), p.tab(p.active_index()).clone())
    });
    let (pane, ended) = cx.update(|cx| {
        let t = tab.read(cx);
        (t.state.focused, t.focused_view().clone())
    });
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(tab.read(cx).views().count(), 2));

    cx.update(|cx| ended.read(cx).write(b"\n".to_vec()));
    run_until(cx, "the session ends", |cx| ended.read(cx).exited);
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(tab.read(cx).views().count(), 2));

    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.reconnect(ix, pane, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let t = tab.read(cx);
        assert_eq!(t.views().count(), 2);
        assert_eq!(t.state.focused, pane);
        let new = t.focused_view();
        assert!(*new != ended, "the pane still holds the ended session");
        let v = new.read(cx);
        assert!(!v.exited && v.remote.is_some());
    });
}

/// Reconnect connects with the server's settings as they are now: a port, user or key fixed
/// since the first connect, or a password saved since, is used.
#[gpui_kit::test]
fn reconnecting_uses_the_servers_settings_as_they_are_now(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let stale = Remote {
        launch: sawit_core::ssh::Launch {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "printf 'sawit-%s\\n' stale; read line".into()],
            env: Vec::new(),
        },
        ..test_remote()
    };
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.open_ssh(stale, Origin::User, window, cx))
    })
    .unwrap()
    .expect("the session starts");
    cx.run_until_parked();
    let (ix, tab) = cx.update(|cx| {
        let p = panel.read(cx);
        (p.active_index(), p.tab(p.active_index()).clone())
    });
    let (pane, ended) = cx.update(|cx| {
        let t = tab.read(cx);
        (t.state.focused, t.focused_view().clone())
    });
    cx.update(|cx| ended.read(cx).write(b"\n".to_vec()));
    run_until(cx, "the session ends", |cx| ended.read(cx).exited);

    // The server's settings changed since: its launch now says "fresh".
    cx.update(|cx| {
        panel.update(cx, |p, _| {
            p.set_remote_source(|key, _| {
                Ok(Remote {
                    key: key.into(),
                    name: "test".into(),
                    launch: sawit_core::ssh::Launch {
                        program: "/bin/sh".into(),
                        args: vec!["-c".into(), "printf 'sawit-%s\\n' fresh; read line".into()],
                        env: Vec::new(),
                    },
                })
            })
        })
    });
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.reconnect(ix, pane, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    let new = cx.update(|cx| tab.read(cx).focused_view().clone());
    assert!(new != ended, "the session didn't reconnect");
    wait_until(cx, "the new session prints", |cx| {
        let text = new.read(cx).text_tail(10);
        text.contains("sawit-fresh") || text.contains("sawit-stale")
    });
    cx.update(|cx| {
        assert!(
            new.read(cx).text_tail(10).contains("sawit-fresh"),
            "it reconnected with the old settings"
        )
    });
}

#[gpui_kit::test]
fn reconnecting_a_live_session_keeps_it(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.open_ssh(test_remote(), Origin::User, window, cx)
        })
    })
    .unwrap()
    .expect("the session starts");
    cx.run_until_parked();
    let (ix, tab) = cx.update(|cx| {
        let p = panel.read(cx);
        (p.active_index(), p.tab(p.active_index()).clone())
    });
    let (pane, live) = cx.update(|cx| {
        let t = tab.read(cx);
        (t.state.focused, t.focused_view().clone())
    });
    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| p.reconnect(ix, pane, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let t = tab.read(cx);
        assert!(
            *t.focused_view() == live,
            "reconnecting replaced a live session"
        );
        assert!(!live.read(cx).exited);
    });
}
