//! What an agent does to a tab or a pane by its place: types into it, reads it and closes it,
//! and how `app_state` describes the panes.

use gpui_kit::{AppContext as _, TestAppContext};
use sawit_core::terminal::run_bytes;

use super::super::Origin;
use super::{click_tab, open_tabs};
use crate::terminal::testing::{
    focus_terminal, init, open_panel, ready, scratch_dir, start, start_cat, wait_until,
};

/// An agent's terminal_close ends what runs there, so without its confirm it is refused
/// while a program runs in the tab or pane, or the user is typing in it, naming why; shells
/// waiting at their prompt that nobody types in close as before.
#[gpui_kit::test]
fn closing_asks_while_a_program_runs_or_the_user_types(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    click_tab(cx, w, &panel, 0);
    let other = cx.update(|cx| panel.read(cx).tab(1).read(cx).focused_view().clone());
    ready(cx, &other);
    let refusal = |cx: &mut TestAppContext, ix: usize, pane: Option<usize>| {
        cx.update_window(w, |_, window, cx| {
            panel.read(cx).close_refusal(ix, pane, window, cx)
        })
        .expect("the test window is open")
    };

    assert_eq!(
        refusal(cx, 1, None),
        Ok(None),
        "an idle shell nobody types in"
    );
    let typing = refusal(cx, 0, None).unwrap().expect("the user types there");
    assert!(typing.contains("the user is typing in it"), "{typing}");
    assert!(typing.contains("confirm: true"), "{typing}");

    start(cx, &other, "sleep 30", "sleep");
    let busy = refusal(cx, 1, Some(0)).unwrap().expect("sleep runs there");
    assert!(
        busy.starts_with("Terminal 1, pane 0 wasn't closed"),
        "{busy}"
    );
    assert!(busy.contains("(sleep)"), "{busy}");
    assert!(!busy.contains("typing"), "{busy}");
    assert!(refusal(cx, 4, None).is_err());
}

/// Commands an agent types (`terminal_input`) mark the shell as used, so opening the
/// window's first project moves it there with a `cd` instead of replacing it.
#[gpui_kit::test]
fn an_agents_input_keeps_its_shell_when_the_first_project_opens(cx: &mut TestAppContext) {
    cx.update(init);
    let (_w, panel) = open_panel(cx);
    let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
    wait_until(cx, "the shell is at its prompt", |cx| {
        view.read(cx).at_prompt()
    });
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.input(Some(0), None, run_bytes("printf 'sawit-%s\\n' typed"), cx)
        })
    })
    .expect("the shell takes input");
    wait_until(cx, "the shell ran what the agent typed", |cx| {
        let v = view.read(cx);
        v.text_tail(50).contains("sawit-typed") && v.at_prompt()
    });
    let project = scratch_dir("agent-input-project");
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            p.set_projects(vec![project.clone()], Some(project.clone()), cx)
        })
    });
    cx.update(|cx| {
        let tab = panel.read(cx).tab(0).read(cx);
        assert!(
            *tab.focused_view() == view,
            "the shell the agent used was replaced"
        );
    });
    wait_until(cx, "the shell moved to the project", |cx| {
        view.read(cx).cwd().as_ref() == Some(&project)
    });
}

/// While the user types in one pane of a split tab (into `cat`, standing in for Claude Code)
/// and an agent's command runs in the other, the tab's focused pane is the user's. So keys and
/// reads for that tab must name their pane: a ctrl-c meant for the agent's command would
/// otherwise interrupt the user's program. Named, it stops the agent's command, and `cat`
/// keeps running.
#[gpui_kit::test]
fn a_split_tab_takes_keys_only_for_a_named_pane(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let (agents, users) = cx.update(|cx| {
        let tab = panel.read(cx).tab(0).read(cx);
        let panes = tab.state.layout.panes();
        (
            tab.view(panes[0]).unwrap().clone(),
            tab.view(panes[1]).unwrap().clone(),
        )
    });
    ready(cx, &agents);
    ready(cx, &users);
    start_cat(cx, &users);
    let (ix, pane) = cx
        .update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.run("sleep 30", Origin::Agent, window, cx))
        })
        .expect("the test window is open")
        .expect("a shell runs it");
    let pane = cx.update(|cx| panel.read(cx).pane_index(ix, pane, cx));
    assert_eq!((ix, pane), (0, Some(0)), "the command went elsewhere");
    wait_until(cx, "the agent's sleep runs", |cx| {
        agents.read(cx).foreground_name().as_deref() == Some("sleep")
    });

    let interrupt = |cx: &mut TestAppContext, pane: Option<usize>| {
        cx.update(|cx| panel.update(cx, |p, cx| p.input(Some(0), pane, vec![0x03], cx)))
    };
    let sent = interrupt(cx, None);
    assert!(
        sent.as_ref().is_err_and(|e| e.contains("pane")),
        "keys went to the split tab's focused pane: {sent:?}"
    );
    let read = cx.update(|cx| panel.read(cx).text(Some(0), None, 10, cx));
    assert!(
        read.as_ref().is_err_and(|e| e.contains("pane")),
        "the split tab's focused pane was read: {read:?}"
    );

    interrupt(cx, Some(0)).expect("the agent's pane takes the keys");
    wait_until(cx, "the agent's sleep stops", |cx| {
        agents.read(cx).at_prompt()
    });
    cx.update(|cx| {
        assert_eq!(
            users.read(cx).foreground_name().as_deref(),
            Some("cat"),
            "the user's program was interrupted"
        )
    });
}

#[gpui_kit::test]
fn describe_lists_panes(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    cx.update(|cx| {
        let tabs = panel.read(cx).describe(cx);
        let panes = tabs[0]["panes"].as_array().expect("a panes list");
        assert_eq!(panes.len(), 2);
        let focused = panes.iter().filter(|p| p["focused"] == true).count();
        assert_eq!(focused, 1);
    });
}

#[gpui_kit::test]
fn a_bad_pane_index_is_an_error(cx: &mut TestAppContext) {
    cx.update(init);
    let (_w, panel) = open_panel(cx);
    cx.update(|cx| {
        assert_eq!(
            panel.read(cx).text(Some(0), Some(5), 10, cx),
            Err("Terminal 0 has no pane 5; panes are 0 to 0.".to_string())
        );
    });
}

/// An agent's terminal_rename names a tab as Rename Tab does, and answers the name the strip
/// shows: a tab of one pane is named itself, its pane named or not, as that pane's own Rename
/// does there; a blank name clears it, and a tab or pane that isn't there is an error.
#[gpui_kit::test]
fn an_agent_names_a_tab_and_a_blank_name_clears_it(cx: &mut TestAppContext) {
    cx.update(init);
    let (_w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    let rename = |cx: &mut TestAppContext, ix: usize, pane: Option<usize>, name: &str| {
        cx.update(|cx| panel.update(cx, |p, cx| p.rename(ix, pane, name, cx)))
    };
    let label = |cx: &mut TestAppContext, ix: usize| {
        cx.update(|cx| panel.read(cx).tab(ix).read(cx).label(cx).to_string())
    };
    let before = label(cx, 1);

    let named = rename(cx, 1, None, "  dev server ").expect("tab 1 is there");
    assert_eq!(named["terminal"], 1);
    assert_eq!(named["named"], "tab");
    assert_eq!(named["name"], "dev server");
    assert_eq!(label(cx, 1), "dev server");
    assert_ne!(label(cx, 0), "dev server", "only tab 1 was named");

    let by_pane = rename(cx, 1, Some(0), "tests").expect("its pane 0 is there");
    assert_eq!(by_pane["named"], "tab", "a tab of one pane is named itself");
    assert_eq!(label(cx, 1), "tests");

    let cleared = rename(cx, 1, None, "").expect("tab 1 is there");
    assert_eq!(cleared["name"], before.as_str());
    assert_eq!(label(cx, 1), before);

    assert!(rename(cx, 5, None, "x").is_err());
    assert!(rename(cx, 1, Some(3), "x").is_err());
}
