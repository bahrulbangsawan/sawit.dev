//! The agents' presence on the canvas: their ops, pointers, outlines, placeholders and chips
//! ([`model`]), and the components they build ([`work`]).

mod model;
mod work;

use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::TestAppContext;
use sawit_core::agents::AgentIdentity;
use serde_json::{Value, json};

use super::{FAILED_TIME, GhostState, PLACEHOLDER_LEAD, TICK};
use crate::agents::Look;
use crate::board::tests::{Scratch, next_frame, open_in_window};
use crate::workspace::Answer;

pub(super) fn look(id: &str) -> Look {
    crate::agents::Look::for_tests(id)
}

pub(super) fn codex() -> AgentIdentity {
    AgentIdentity {
        id: "mcp-1-2".into(),
        name: "Codex".into(),
        label: Some("shop".into()),
        version: None,
    }
}

/// The op's answer, once it has come.
pub(super) fn answered(answer: &mut oneshot::Receiver<Result<Value, String>>) -> Option<Value> {
    answer.try_recv().ok().flatten().and_then(Result::ok)
}

/// Runs the clock on in steps of the canvas's tick, as the app's time goes on.
pub(super) fn wait(cx: &mut TestAppContext, time: Duration) {
    let steps = (time.as_millis() / TICK.as_millis()).max(1) + 1;
    for _ in 0..steps {
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
    }
}

/// An agent's quick op answers at once, as it always has, and its pointer goes to what it
/// drew, in its colour, with what it did said to screen readers and listed in app_state.
#[gpui_kit::test]
fn a_quick_op_answers_at_once_and_shows_the_agent_there(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-quick-op");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let op = json!({"op": "add", "elements": [
            {"type": "sticky", "x": 40, "y": 60, "width": 100, "height": 100}]});
    let answer = view.update(cx, |view, cx| view.agent_op(op, Some(&codex()), cx));
    let Answer::Now(reply) = answer else {
        panic!("a quick op answers at once");
    };
    assert_eq!(reply["ok"], true, "{reply}");
    view.read_with(cx, |view, cx| {
        let agent = &view.agents.agents[0];
        assert_eq!(agent.look.label.as_ref(), "Codex");
        assert_eq!(agent.doing, "adding a sticky");
        // Outside the sticky's top-right corner.
        let (x, y) = agent.at.map(|at| at.point).unwrap();
        assert!(x > 40.0 + 99.0 && y == 60.0, "{x}, {y}");
        assert_eq!(
            view.agents.announcer.current.as_ref(),
            "Codex added a sticky"
        );
        let listed = view.describe_agents(cx);
        assert_eq!(listed[0]["surface"], "canvas");
        assert_eq!(listed[0]["action"], "adding a sticky");
    });
}

/// A slow op shows its placeholder at once, runs once that has been painted, and the
/// placeholder goes with the frame of its result.
#[gpui_kit::test]
fn a_slow_ops_placeholder_shows_until_its_result_lands(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-slow-op");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    let first = next_frame(cx, &view, None);
    let op = json!({"op": "mermaid", "source": "graph TD\nA-->B", "x": 0, "y": 0});
    let answer = view.update(cx, |view, cx| view.agent_op(op, Some(&codex()), cx));
    let Answer::Later(mut answer) = answer else {
        panic!("a slow op answers once it has run");
    };
    view.read_with(cx, |view, _| {
        assert_eq!(view.agents.ghosts.len(), 1, "the placeholder shows at once");
        assert!(matches!(view.agents.ghosts[0].state, GhostState::Waiting));
        assert_eq!(
            view.agents.queue.len(),
            1,
            "the op waits for it to be painted"
        );
    });
    cx.executor().advance_clock(PLACEHOLDER_LEAD);
    cx.run_until_parked();
    let reply = answered(&mut answer).expect("the op ran");
    assert_eq!(reply["ok"], true, "{reply}");
    // The frame of the diagram takes the placeholder away.
    let mut last = first;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while view.read_with(cx, |view, _| !view.agents.ghosts.is_empty()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the placeholder stayed"
        );
        last = next_frame(cx, &view, Some(&last));
    }
}

/// A slow op that fails leaves an error mark where its placeholder was, for a moment, and
/// says it couldn't finish.
#[gpui_kit::test]
fn a_failed_slow_ops_placeholder_shows_an_error_mark_then_goes(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-failed-op");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let op = json!({"op": "mermaid", "source": "this is not a diagram ((("});
    let answer = view.update(cx, |view, cx| view.agent_op(op, Some(&codex()), cx));
    let Answer::Later(mut answer) = answer else {
        panic!("a slow op answers once it has run");
    };
    cx.executor().advance_clock(PLACEHOLDER_LEAD);
    cx.run_until_parked();
    let reply = answered(&mut answer).expect("the op ran");
    assert_eq!(reply["ok"], false, "{reply}");
    view.read_with(cx, |view, _| {
        assert!(matches!(
            view.agents.ghosts[0].state,
            GhostState::Failed { .. }
        ));
        assert_eq!(
            view.agents.agents[0].doing,
            "couldn't finish drawing a diagram"
        );
    });
    wait(cx, FAILED_TIME);
    view.read_with(cx, |view, _| assert!(view.agents.ghosts.is_empty()));
}

/// `op`'s diagram, rendered and converted as the control op does it off the UI thread.
fn prepared(
    op: &Value,
) -> impl std::future::Future<Output = sawit_board::ops::PreparedDiagram> + Send + 'static {
    let op = op.clone();
    async move { sawit_board::ops::prepare_diagram(&op).expect("a mermaid op") }
}

/// An agent's diagram, rendered off the UI thread, shows its placeholder at once and lands
/// with the agent's pointer and what it did, as any slow op does.
#[gpui_kit::test]
fn a_diagram_rendered_off_the_ui_thread_shows_its_placeholder_then_lands(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-diagram-op");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    let first = next_frame(cx, &view, None);
    let op = json!({"op": "mermaid", "source": "graph TD\nA-->B", "x": 0, "y": 0});
    let prepare = prepared(&op);
    let answer = view.update(cx, |view, cx| {
        view.agent_diagram_op(op, Some(&codex()), prepare, cx)
    });
    let Answer::Later(mut answer) = answer else {
        panic!("a diagram answers once it is drawn");
    };
    view.read_with(cx, |view, _| {
        assert_eq!(view.agents.ghosts.len(), 1, "the placeholder shows at once");
        assert!(matches!(view.agents.ghosts[0].state, GhostState::Waiting));
    });
    cx.run_until_parked();
    let reply = answered(&mut answer).expect("the diagram was drawn");
    assert_eq!(reply["ok"], true, "{reply}");
    view.read_with(cx, |view, _| {
        let agent = &view.agents.agents[0];
        assert_eq!(agent.look.label.as_ref(), "Codex");
        assert!(agent.at.is_some(), "the pointer went to the diagram");
    });
    let mut last = first;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while view.read_with(cx, |view, _| !view.agents.ghosts.is_empty()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the placeholder stayed"
        );
        last = next_frame(cx, &view, Some(&last));
    }
}

/// A diagram rendered off the UI thread that can't be drawn leaves the error mark, as a slow
/// op that fails does.
#[gpui_kit::test]
fn a_diagram_that_cant_be_drawn_off_the_ui_thread_shows_an_error_mark(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-diagram-failed");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let op = json!({"op": "mermaid", "source": "this is not a diagram ((("});
    let prepare = prepared(&op);
    let answer = view.update(cx, |view, cx| {
        view.agent_diagram_op(op, Some(&codex()), prepare, cx)
    });
    let Answer::Later(mut answer) = answer else {
        panic!("a diagram answers once it is drawn");
    };
    cx.run_until_parked();
    let reply = answered(&mut answer).expect("the op ran");
    assert_eq!(reply["ok"], false, "{reply}");
    view.read_with(cx, |view, _| {
        assert!(matches!(
            view.agents.ghosts[0].state,
            GhostState::Failed { .. }
        ));
        assert_eq!(
            view.agents.agents[0].doing,
            "couldn't finish drawing a diagram"
        );
    });
}

/// With agents' cursors turned off, a slow op runs at once, with no placeholder.
#[gpui_kit::test]
fn with_cursors_off_a_slow_op_runs_at_once(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-cursors-off");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    cx.update(crate::agents::toggle);
    let op = json!({"op": "mermaid", "source": "graph TD\nA-->B"});
    let answer = view.update(cx, |view, cx| view.agent_op(op, None, cx));
    assert!(matches!(answer, Answer::Now(_)));
    view.read_with(cx, |view, _| assert!(view.agents.ghosts.is_empty()));
}
