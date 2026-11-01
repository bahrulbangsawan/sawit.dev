//! What runs in the terminals: the coding agents a tab's logos and `app_state` show, which one
//! the user types to, the browser's notes sent to one, and what quitting would end.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext};
use sawit_core::terminal::{AgentKind, run_bytes, shell_quote};

use super::super::running::ENTER_AFTER;
use super::super::{JobTag, Origin, TerminalPanel};
use super::{active, click_tab, has_keyboard, open_tabs, selector, test_remote};
use crate::terminal::testing::{
    focus_terminal, init, open_panel, ready, scratch_dir, start, wait_until,
};
use crate::terminal::view::AGENT_CHECK;

/// Annotate's notes reach an agent as one message: one bracketed paste, its lines inside it,
/// then Return, in that agent's pane only, and the keyboard stays where it was. The agent here
/// is `head`, started under the name `claude` with bracketed paste on (as Claude Code turns
/// it on) and the terminal raw, so the bytes it writes to a file are those sent; it ends once
/// it has the message. Once it has exited, nothing is sent: its shell would run the notes.
#[gpui_kit::test]
fn notes_reach_an_agent_as_one_message(cx: &mut TestAppContext) {
    use sawit_core::terminal::AgentKind;
    cx.update(init);
    let (w, panel) = open_panel(cx);
    open_tabs(cx, &panel, 1);
    let (first, second) = cx.update(|cx| {
        let p = panel.read(cx);
        (
            p.tab(0).read(cx).focused_view().clone(),
            p.tab(1).read(cx).focused_view().clone(),
        )
    });
    ready(cx, &first);
    ready(cx, &second);
    // The panes look for their agent a moment after output: on the test's clock, now.
    let look = |cx: &mut TestAppContext| {
        cx.run_until_parked();
        cx.executor().advance_clock(AGENT_CHECK * 2);
        cx.run_until_parked();
    };
    look(cx);
    assert!(cx.update(|cx| panel.read(cx).local_agents(cx).is_empty()));

    let notes = "## Page\n\n- **Fix:** it's `broken`";
    let message = b"\x1b[200~## Page\r\r- **Fix:** it's `broken`\x1b[201~\r";
    let got = scratch_dir("agent-message").join("got");
    let agent = format!(
        "stty raw -echo; printf '\\033[?2004h'; (exec -a claude head -c {} > {})",
        message.len(),
        shell_quote(&got)
    );
    cx.update(|cx| second.read(cx).write(run_bytes(&agent)));
    wait_until(cx, "the agent runs, with bracketed paste on", |cx| {
        let v = second.read(cx);
        v.foreground_name().as_deref() == Some("head") && v.accepts_paste()
    });
    look(cx);
    let agents = cx.update(|cx| panel.read(cx).local_agents(cx));
    assert_eq!(agents.len(), 1, "the agent is known by the name it runs as");
    let agent = agents[0].clone();
    assert_eq!(agent.kind, AgentKind::ClaudeCode);
    assert!(agent.tab == cx.update(|cx| panel.read(cx).tab(1).downgrade()));

    // The user is in the browser, the first tab in front: the keyboard is in neither.
    click_tab(cx, w, &panel, 0);
    cx.update_window(w, |_, window, cx| {
        let away = cx.focus_handle();
        window.focus(&away, cx);
    })
    .expect("the test window is open");
    let send = |cx: &mut TestAppContext| {
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.send_to_agent(agent.pane, agent.kind, notes, window, cx)
            })
        })
        .expect("the test window is open")
    };
    assert_eq!(send(cx).map(|a| a.pane), Ok(agent.pane));
    cx.executor().advance_clock(ENTER_AFTER * 2);
    cx.run_until_parked();
    wait_until(cx, "the agent took the message and ended", |cx| {
        second.read(cx).foreground_job().is_none()
            && std::fs::read(&got).is_ok_and(|b| b.len() == message.len())
    });
    assert_eq!(std::fs::read(&got).unwrap(), message);
    assert!(
        active(cx, &panel) == cx.update(|cx| panel.read(cx).tab(1).clone()),
        "the agent's tab shows"
    );
    assert!(
        !has_keyboard(cx, w, &cx.update(|cx| panel.read(cx).tab(1).clone())),
        "the keyboard stays with the user"
    );
    cx.update(|cx| assert!(!first.read(cx).text_tail(50).contains("Fix")));

    // Exited, before the pane looked again: its shell is back, and gets nothing.
    let refused = send(cx);
    assert_eq!(
        refused.map(|a| a.pane),
        Err("Claude Code is no longer running there, so nothing was sent.".into())
    );
    cx.executor().advance_clock(ENTER_AFTER * 2);
    cx.run_until_parked();
    cx.update(|cx| assert!(!second.read(cx).text_tail(50).contains("Fix")));
    look(cx);
    assert!(cx.update(|cx| panel.read(cx).local_agents(cx).is_empty()));
}

/// What quitting or closing the window would end: commands in the foreground and live SSH
/// sessions. Shells at their prompt and a script's terminal between runs don't count.
#[gpui_kit::test]
fn running_lists_what_would_end_with_the_app(cx: &mut TestAppContext) {
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
    let job = active(cx, &panel);
    let job = cx.update(|cx| job.read(cx).focused_view().clone());
    wait_until(cx, "the script ran", |cx| {
        let v = job.read(cx);
        v.text_tail(50).contains("sawit-built") && v.at_prompt()
    });
    let running = |cx: &mut TestAppContext| cx.update(|cx| panel.read(cx).running(cx));
    assert_eq!(running(cx), Vec::<String>::new());

    cx.update_window(w, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.open_ssh(test_remote(), Origin::User, window, cx)
        })
    })
    .expect("the test window is open")
    .expect("the session starts");
    assert_eq!(running(cx), ["test (SSH)"]);

    start(cx, &shell, "sleep 30", "sleep");
    assert_eq!(running(cx), ["sleep", "test (SSH)"]);
}

/// Splits tab 0 into a row of three panes with a fourth under the last, and returns them
/// in reading order.
fn four_panes(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    panel: &Entity<TerminalPanel>,
) -> Vec<sawit_core::terminal::PaneId> {
    focus_terminal(cx, w, panel);
    cx.simulate_keystrokes(w, "cmd-d cmd-d cmd-shift-d");
    cx.run_until_parked();
    cx.update(|cx| {
        let t = panel.read(cx).tab(0).read(cx);
        let order = t.state.reading_order(t.area());
        assert_eq!(order.len(), 4);
        order
    })
}

/// Makes the panes of tab 0 look as if `agents` ran in them, in reading order.
fn fake_agents(
    cx: &mut TestAppContext,
    panel: &Entity<TerminalPanel>,
    panes: &[sawit_core::terminal::PaneId],
    agents: &[Option<AgentKind>],
) {
    cx.update(|cx| {
        let views: Vec<_> = panes
            .iter()
            .map(|pane| panel.read(cx).tab(0).read(cx).view(*pane).unwrap().clone())
            .collect();
        for (view, agent) in views.iter().zip(agents) {
            view.update(cx, |v, cx| v.fake_agent(*agent, cx));
        }
    });
    cx.run_until_parked();
}

/// Where the element tests know as `name` is drawn in window `w`, if it is.
fn bounds_of(
    cx: &mut TestAppContext,
    w: AnyWindowHandle,
    name: String,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let mut vcx = VisualTestContext::from_window(w, cx);
    vcx.run_until_parked();
    vcx.debug_bounds(selector(name))
}

/// A split tab shows every distinct agent running in its panes, focused or not, in the panes'
/// reading order, and past three, the first two and "+N". It keeps showing them while another
/// tab is in front.
#[gpui_kit::test]
fn the_tab_shows_each_agent_of_its_panes_in_reading_order(cx: &mut TestAppContext) {
    use AgentKind::*;
    cx.update(init);
    let (w, panel) = open_panel(cx);
    let panes = four_panes(cx, w, &panel);
    let logo = |id: &str| format!("term-agent-0-{id}");

    assert!(bounds_of(cx, w, "term-agents-0".into()).is_none());
    let plain = bounds_of(cx, w, "term-tab-0".into()).unwrap();
    fake_agents(
        cx,
        &panel,
        &panes,
        &[Some(Codex), Some(ClaudeCode), Some(Codex), None],
    );
    let codex = bounds_of(cx, w, logo("codex")).expect("Codex's logo");
    let claude = bounds_of(cx, w, logo("claude-code")).expect("Claude Code's logo");
    assert!(codex.origin.x < claude.origin.x, "in reading order");
    assert!(bounds_of(cx, w, logo("more")).is_none());
    let with_logos = bounds_of(cx, w, "term-tab-0".into()).unwrap();
    assert_eq!(
        with_logos.size.height, plain.size.height,
        "the tab keeps its height"
    );
    cx.update(|cx| {
        let agents: Vec<_> = panel
            .read(cx)
            .running_agents(cx)
            .iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(agents, [Codex, ClaudeCode, Codex]);
    });

    fake_agents(
        cx,
        &panel,
        &panes,
        &[
            Some(Codex),
            Some(ClaudeCode),
            Some(GeminiCli),
            Some(Opencode),
        ],
    );
    assert!(bounds_of(cx, w, logo("codex")).is_some());
    assert!(bounds_of(cx, w, logo("claude-code")).is_some());
    assert!(bounds_of(cx, w, logo("gemini")).is_none(), "past three");
    assert!(bounds_of(cx, w, logo("opencode")).is_none());
    assert!(bounds_of(cx, w, logo("more")).is_some(), "+2");

    // In the background, behind a new tab, the agents still show.
    open_tabs(cx, &panel, 1);
    assert_ne!(cx.update(|cx| panel.read(cx).active_index()), 0);
    assert!(bounds_of(cx, w, logo("codex")).is_some());

    // Once they all exit, the terminal glyph is back.
    fake_agents(cx, &panel, &panes, &[None, None, None, None]);
    assert!(bounds_of(cx, w, "term-agents-0".into()).is_none());
}

/// app_state reports each pane's agent by its id, the focused pane's as the tab's own, and
/// every distinct agent of the tab as its strip shows them.
#[gpui_kit::test]
fn describe_reports_the_agents(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let panes = cx.update(|cx| {
        let t = panel.read(cx).tab(0).read(cx);
        t.state.reading_order(t.area())
    });
    fake_agents(
        cx,
        &panel,
        &panes,
        &[Some(AgentKind::ClaudeCode), Some(AgentKind::Codex)],
    );
    cx.update(|cx| {
        let tabs = panel.read(cx).describe(cx);
        let tab = &tabs[0];
        // ⌘D focused the new pane, on the right.
        assert_eq!(tab["agent"], "codex");
        assert_eq!(tab["agents"], serde_json::json!(["claude-code", "codex"]));
        let agents: Vec<_> = tab["panes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["agent"].clone())
            .collect();
        assert_eq!(agents, [serde_json::json!("claude-code"), "codex".into()]);
    });
    fake_agents(cx, &panel, &panes, &[None, None]);
    cx.update(|cx| {
        let tabs = panel.read(cx).describe(cx);
        assert_eq!(tabs[0]["agent"], serde_json::Value::Null);
        assert_eq!(tabs[0]["agents"], serde_json::json!([]));
    });
}

/// The pane the user types in is the running agent that is `focused`, even in a tab the user
/// has left; with no pane ever typed in, the active tab's focused pane is.
#[gpui_kit::test]
fn running_agents_say_which_one_the_user_types_to(cx: &mut TestAppContext) {
    cx.update(init);
    let (w, panel) = open_panel(cx);
    focus_terminal(cx, w, &panel);
    cx.simulate_keystrokes(w, "cmd-d");
    cx.run_until_parked();
    let panes = cx.update(|cx| {
        let t = panel.read(cx).tab(0).read(cx);
        t.state.reading_order(t.area())
    });
    fake_agents(
        cx,
        &panel,
        &panes,
        &[Some(AgentKind::ClaudeCode), Some(AgentKind::Codex)],
    );
    let focused = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            panel
                .read(cx)
                .running_agents(cx)
                .iter()
                .filter(|r| r.focused)
                .map(|r| (r.kind, r.label.to_string()))
                .collect::<Vec<_>>()
        })
    };
    let typed = focused(cx);
    assert_eq!(typed.len(), 1);
    assert_eq!(
        typed[0].0,
        AgentKind::Codex,
        "⌘D gave the new pane the keyboard"
    );
    cx.simulate_keystrokes(w, "cmd-[");
    cx.run_until_parked();
    assert_eq!(focused(cx)[0].0, AgentKind::ClaudeCode);
}
