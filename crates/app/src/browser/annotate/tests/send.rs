//! The footer: its actions fit the panel, and the send button follows the agents.

use super::{agent, at};
use crate::browser::annotate::send::SendAction;
use crate::browser::annotate::{AnnotateEvent, AnnotatePanel};
use sawit_core::terminal::AgentKind;

/// The footer's actions fit the panel's width inside its inset: Clear on the console
/// errors' line, apart from Send and Copy on the line under it.
#[gpui_kit::test]
fn the_footer_fits_the_panel(cx: &mut gpui_kit::TestAppContext) {
    use crate::browser::panel::PANEL_WIDTH;
    use gpui_kit::{VisualTestContext, px, size};
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(PANEL_WIDTH), px(700.)), |window, cx| {
        let mut panel = AnnotatePanel::new(window, cx);
        panel.add(at("http://a/1", 1), false, window, cx);
        panel
    });
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    let [send, copy, clear] = ["annotate-send", "annotate-copy", "annotate-clear"].map(|s| {
        cx.debug_bounds(s)
            .unwrap_or_else(|| panic!("{s} isn't drawn"))
    });
    assert!(send.left() >= px(12.), "{send:?}");
    assert!(send.right() <= copy.left(), "Send runs into Copy");
    assert!(
        copy.right() <= px(PANEL_WIDTH - 12.),
        "Copy is clipped: {copy:?}"
    );
    assert!(
        clear.right() <= px(PANEL_WIDTH - 12.),
        "Clear is clipped: {clear:?}"
    );
    assert_eq!(send.top(), copy.top(), "Send and Copy wrapped");
    assert!(clear.bottom() <= send.top(), "Clear sits with Send");
}

/// The send button follows the agents running in the terminals: none, it opens one (with a
/// menu of the CLIs installed, when there are several); one, it sends to it; several, it sends
/// to the default and its menu lists them all. Without notes it is there, disabled, and says
/// why.
#[gpui_kit::test]
fn the_send_button_follows_the_agents(cx: &mut gpui_kit::TestAppContext) {
    use crate::browser::panel::PANEL_WIDTH;
    use AgentKind::{ClaudeCode, Codex, GeminiCli};
    use gpui_kit::{VisualTestContext, px, size};
    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(PANEL_WIDTH), px(700.)), AnnotatePanel::new);
    let panel = window.root(cx).unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.run_until_parked();
    let action = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.send_action());
    let drawn = |cx: &mut VisualTestContext| {
        [
            "annotate-send-one",
            "annotate-send-split",
            "annotate-open-one",
            "annotate-open-split",
            "annotate-send-none",
        ]
        .into_iter()
        .filter(|s| cx.debug_bounds(s).is_some())
        .collect::<Vec<_>>()
    };
    // Still looking for the CLIs, and no notes: disabled, with why.
    assert_eq!(action(&mut cx), SendAction::NoAgent { looking: true });
    assert_eq!(drawn(&mut cx), ["annotate-send-none"]);
    assert!(cx.debug_bounds("annotate-note").is_some(), "says why");

    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.add(at("http://a/1", 1), false, window, cx);
            p.set_installed(vec![], cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(action(&mut cx), SendAction::NoAgent { looking: false });
    assert!(
        cx.debug_bounds("annotate-note").is_some(),
        "says what to install"
    );

    // None running: Open, Claude Code first; a menu with two CLIs or more.
    cx.update(|_, cx| panel.update(cx, |p, cx| p.set_installed(vec![Codex], cx)));
    cx.run_until_parked();
    assert_eq!(action(&mut cx), SendAction::Open { kind: Codex });
    assert_eq!(drawn(&mut cx), ["annotate-open-one"]);
    cx.update(|_, cx| panel.update(cx, |p, cx| p.set_installed(vec![Codex, ClaudeCode], cx)));
    cx.run_until_parked();
    assert_eq!(action(&mut cx), SendAction::Open { kind: ClaudeCode });
    assert_eq!(drawn(&mut cx), ["annotate-open-split"]);
    assert!(
        cx.debug_bounds("annotate-note").is_none(),
        "nothing in the way"
    );

    // One running: Send to it, no menu.
    cx.update(|_, cx| {
        panel.update(cx, |p, cx| {
            p.set_agents(vec![agent(1, GeminiCli, false)], cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(action(&mut cx), SendAction::Send { default: 0 });
    assert_eq!(drawn(&mut cx), ["annotate-send-one"]);

    // Two: a split button, sending to the one in front.
    cx.update(|_, cx| {
        panel.update(cx, |p, cx| {
            p.set_agents(vec![agent(1, ClaudeCode, false), agent(2, Codex, true)], cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(action(&mut cx), SendAction::Send { default: 1 });
    assert_eq!(drawn(&mut cx), ["annotate-send-split"]);

    // Sending asks for that agent, which stays the default while it runs.
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = asked.clone();
    cx.update(|_, cx| {
        cx.subscribe(&panel, move |_, event: &AnnotateEvent, _| {
            if let AnnotateEvent::SendTo(agent) = event {
                seen.borrow_mut().push(agent.pane);
            }
        })
        .detach()
    });
    cx.update(|_, cx| panel.update(cx, |p, cx| p.send_to(1, cx)));
    assert_eq!(*asked.borrow(), [1]);
    cx.update(|_, cx| {
        panel.update(cx, |p, cx| {
            p.set_agents(
                vec![agent(1, ClaudeCode, false), agent(2, Codex, false)],
                cx,
            )
        })
    });
    assert_eq!(
        action(&mut cx),
        SendAction::Send { default: 0 },
        "the last used"
    );
    // It ended: forgotten.
    cx.update(|_, cx| panel.update(cx, |p, cx| p.set_agents(vec![agent(2, Codex, false)], cx)));
    cx.update(|_, cx| {
        panel.update(cx, |p, cx| {
            p.set_agents(
                vec![agent(1, ClaudeCode, false), agent(2, Codex, false)],
                cx,
            )
        })
    });
    assert_eq!(
        action(&mut cx),
        SendAction::Send { default: 1 },
        "the newest"
    );
}
