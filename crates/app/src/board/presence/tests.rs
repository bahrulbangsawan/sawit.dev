//! The agents' presence on the canvas: their ops, pointers, outlines, placeholders and chips.

use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use futures::channel::oneshot;
use gpui_kit::TestAppContext;
use sawit_core::agents::{AgentIdentity, GONE_AFTER};
use serde_json::{Value, json};

use super::edges::{Toward, edge_chip};
use super::marks::Mark;
use super::target::{Target, ghost_rect, target, union};
use super::{
    ANNOUNCE_GAP, EDGE_CHIP, EDGE_INSET, FAILED_TIME, GLIDE, GhostState, PLACEHOLDER_LEAD,
    Presence, TICK, TOUCHED_TIME,
};
use crate::agents::Look;
use crate::board::tests::{Scratch, next_frame, open_in_window};
use crate::workspace::Answer;

fn look(id: &str) -> Look {
    crate::agents::Look::for_tests(id)
}

fn codex() -> AgentIdentity {
    AgentIdentity {
        id: "mcp-1-2".into(),
        name: "Codex".into(),
        label: Some("shop".into()),
        version: None,
    }
}

/// The op's answer, once it has come.
fn answered(answer: &mut oneshot::Receiver<Result<Value, String>>) -> Option<Value> {
    answer.try_recv().ok().flatten().and_then(Result::ok)
}

/// Runs the clock on in steps of the canvas's tick, as the app's time goes on.
fn wait(cx: &mut TestAppContext, time: Duration) {
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
        assert_eq!(agent.at.map(|at| at.point), Some((40.0, 60.0)));
        assert_eq!(view.agents.announcement.as_ref(), "Codex added a sticky");
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

/// A board with two stickies, and the same board after `op`, run as an agent's op runs.
fn before_after(op: Value) -> (Arc<Document>, Arc<Document>, NodeId) {
    let mut board = sawit_board::BoardDoc::new_board("Board").unwrap();
    let add = json!({"op": "add", "elements": [
        {"type": "sticky", "id": "a", "x": 0, "y": 0, "width": 100, "height": 100},
        {"type": "sticky", "id": "b", "x": 400, "y": 0, "width": 100, "height": 100},
    ]});
    assert_eq!(sawit_board::ops::run(&mut board, &add)["ok"], true);
    let before = board.document_itself().unwrap();
    let reply = sawit_board::ops::run(&mut board, &op);
    assert_eq!(reply["ok"], true, "{reply}");
    let after = board.document_itself().unwrap();
    let page = board.session.ds().unwrap().page;
    (before, after, page)
}

/// An agent's pointer goes to the top-left corner of what it adds, and the new layers are
/// outlined.
#[test]
fn the_pointer_goes_to_what_an_add_drew() {
    let (before, after, page) = before_after(json!({"op": "add", "elements": [
        {"type": "sticky", "x": 200, "y": 300, "width": 100, "height": 100},
        {"type": "sticky", "x": 320, "y": 300, "width": 100, "height": 100},
    ]}));
    let target = target(&before, &after, page).expect("a target");
    assert_eq!(target.point, (200.0, 300.0));
    assert_eq!(target.rects.len(), 2, "{:?}", target.rects);
}

/// Many layers at once (a diagram's) are outlined as one box around them all.
#[test]
fn many_layers_are_outlined_as_one_box() {
    let stickies: Vec<Value> = (0..12)
        .map(
            |i| json!({"type": "sticky", "x": 1000 + i * 120, "y": 0, "width": 100, "height": 100}),
        )
        .collect();
    let (before, after, page) = before_after(json!({"op": "add", "elements": stickies}));
    let target = target(&before, &after, page).expect("a target");
    assert_eq!(target.point, (1000.0, 0.0));
    let [[x0, y0, x1, _]] = target.rects[..] else {
        panic!("one box: {:?}", target.rects);
    };
    assert_eq!((x0, y0), (1000.0, 0.0));
    assert!(x1 >= 1000.0 + 11.0 * 120.0 + 100.0 - 1.0, "{x1}");
}

/// For an update it goes to the centre of what changed: one layer's bounds, or, for several,
/// the centre of them all, as a selection's.
#[test]
fn the_pointer_goes_to_the_centre_of_what_an_update_changed() {
    let (before, after, page) =
        before_after(json!({"op": "update", "elements": [{"id": "b", "x": 600, "y": 200}]}));
    let one = target(&before, &after, page).expect("a target");
    assert_eq!(one.rects.len(), 1);
    let [x0, y0, x1, y1] = one.rects[0];
    assert_eq!(one.point, ((x0 + x1) / 2.0, (y0 + y1) / 2.0));
    assert!(
        x0 >= 599.0 && y0 >= 199.0,
        "where it moved to: {:?}",
        one.rects[0]
    );

    let (before, after, page) = before_after(json!({"op": "update", "elements": [
            {"id": "a", "x": 0, "y": 100}, {"id": "b", "x": 400, "y": 100}]}));
    let both = target(&before, &after, page).expect("a target");
    assert_eq!(both.rects.len(), 2);
    let [x0, _, x1, _] = union(&both.rects).unwrap();
    assert_eq!(both.point.0, (x0 + x1) / 2.0);
}

/// A deletion leaves no layer to outline: the pointer goes where the layers were.
#[test]
fn the_pointer_goes_where_a_deletion_took_layers_off() {
    let (before, after, page) = before_after(json!({"op": "delete", "ids": ["b"]}));
    let target = target(&before, &after, page).expect("a target");
    assert!(target.rects.is_empty());
    assert!(target.point.0 > 400.0, "{:?}", target.point);
    assert_eq!(
        super::target::target(&before, &before, page),
        None,
        "nothing changed"
    );
}

/// The pointer glides from where it was to its new place, eased out, and jumps there under
/// reduced motion; the first time, it shows where it is.
#[test]
fn pointers_glide_between_targets() {
    let mut presence = Presence::default();
    let page = NodeId(1);
    let t0 = Instant::now();
    let a = look("a");
    let at = |point| Target {
        point,
        rects: vec![],
    };
    presence.saw(&a, "adding a sticky", t0);
    presence.point_at(&a, page, at((0.0, 0.0)), t0);
    assert_eq!(presence.agents[0].point(t0, false), Some((0.0, 0.0)));
    let t1 = t0 + Duration::from_secs(1);
    presence.point_at(&a, page, at((100.0, 0.0)), t1);
    let half = presence.agents[0].point(t1 + GLIDE / 2, false).unwrap();
    assert!(half.0 > 50.0 && half.0 < 100.0, "eased out: {half:?}");
    assert_eq!(
        presence.agents[0].point(t1 + GLIDE, false),
        Some((100.0, 0.0))
    );
    assert_eq!(presence.agents[0].point(t1, true), Some((100.0, 0.0)));
    assert!(presence.animating(t1, false));
    assert!(!presence.animating(t1 + TOUCHED_TIME, false));
}

/// An agent quiet for 30 s goes, and so does its pointer.
#[test]
fn presence_expires() {
    let mut presence = Presence::default();
    let t0 = Instant::now();
    presence.saw(&look("a"), "adding a sticky", t0);
    assert!(presence.prune(t0 + Duration::from_secs(29)));
    assert!(!presence.prune(t0 + GONE_AFTER));
    assert!(presence.agents.is_empty());
}

/// A placeholder shows from when its op comes; it goes with the frame of the document its op
/// made, or after a moment if that never comes, and a failed op's shows its error mark for a
/// moment.
#[test]
fn placeholders_land_or_fail() {
    let mut presence = Presence::default();
    let page = NodeId(1);
    let t0 = Instant::now();
    let a = look("a");
    let ghost = presence.add_ghost(&a, page, [10.0, 20.0, 330.0, 220.0], t0);
    assert_eq!(presence.spot("a").map(|s| s.point), Some((10.0, 20.0)));
    let view = ViewState {
        vw: 800.0,
        vh: 600.0,
        ..ViewState::default()
    };
    let ghosts = |p: &Presence| {
        p.marks(&view, page, t0, false)
            .into_iter()
            .filter(|m| matches!(m, Mark::Ghost { .. }))
            .count()
    };
    assert_eq!(ghosts(&presence), 1);
    assert!(presence.animating(t0, false), "it shimmers");
    assert!(
        !presence.animating(t0 + GLIDE, true),
        "not under reduced motion"
    );
    let doc = Arc::new(Document::default());
    presence.land(ghost, doc.clone(), t0);
    assert!(presence.prune(t0 + Duration::from_millis(100)));
    assert_eq!(ghosts(&presence), 1, "until its frame comes");
    presence.rendered(&Arc::new(Document::default()));
    assert_eq!(ghosts(&presence), 1, "another document's frame");
    presence.rendered(&doc);
    assert_eq!(ghosts(&presence), 0);

    let failed = presence.add_ghost(&a, page, [0.0, 0.0, 10.0, 10.0], t0);
    presence.fail(failed, true, t0);
    assert!(
        presence
            .marks(&view, page, t0, false)
            .iter()
            .any(|m| matches!(m, Mark::Ghost { failed: true, .. }))
    );
    presence.prune(t0 + FAILED_TIME);
    assert_eq!(ghosts(&presence), 0, "the error mark goes after a moment");
}

/// An agent at work off screen gets a chip just inside the edge nearest it, pointing its way.
#[test]
fn off_screen_agents_get_a_chip_at_the_edge() {
    let size = (800.0, 600.0);
    assert_eq!(edge_chip((400.0, 300.0), size), None, "on screen");
    let (at, toward) = edge_chip((2000.0, 300.0), size).unwrap();
    assert_eq!(toward, Toward::Right);
    assert_eq!(
        at,
        (800.0 - EDGE_INSET - EDGE_CHIP.0, 300.0 - EDGE_CHIP.1 / 2.0)
    );
    let (at, toward) = edge_chip((400.0, -900.0), size).unwrap();
    assert_eq!((toward, at.1), (Toward::Up, EDGE_INSET));
    assert_eq!(edge_chip((-50.0, 10.0), size).unwrap().1, Toward::Left);
}

/// The same agent is announced once in a while, two agents each in turn.
#[test]
fn announcements_are_polite() {
    let mut presence = Presence::default();
    let t0 = Instant::now();
    presence.announce(&look("a"), "added a sticky", t0);
    assert_eq!(presence.announcement.as_ref(), "Agent a added a sticky");
    presence.announce(&look("a"), "moved a layer", t0 + Duration::from_millis(500));
    assert_eq!(presence.announcement.as_ref(), "Agent a added a sticky");
    presence.announce(
        &look("b"),
        "drew a diagram",
        t0 + Duration::from_millis(600),
    );
    assert_eq!(presence.announcement.as_ref(), "Agent b drew a diagram");
    presence.announce(&look("a"), "moved a layer", t0 + ANNOUNCE_GAP);
    assert_eq!(presence.announcement.as_ref(), "Agent a moved a layer");
}

/// A slow op's placeholder covers what it will draw, else the corner it names, else the
/// middle of the view.
#[test]
fn placeholders_go_where_the_result_will() {
    let add = json!({"op": "add", "elements": [
            {"type": "rectangle", "x": 10, "y": 20, "width": 100, "height": 50}]});
    assert_eq!(
        ghost_rect(&add, (0.0, 0.0), None),
        [10.0, 20.0, 110.0, 70.0]
    );
    let mermaid = json!({"op": "mermaid", "source": "graph TD", "x": 500, "y": 40});
    let beside = Some((900.0, 0.0));
    assert_eq!(
        ghost_rect(&mermaid, (0.0, 0.0), beside),
        [500.0, 40.0, 820.0, 240.0]
    );
    let anywhere = json!({"op": "mermaid", "source": "graph TD"});
    assert_eq!(
        ghost_rect(&anywhere, (0.0, 0.0), beside),
        [900.0, 0.0, 1220.0, 200.0]
    );
    let image = json!({"op": "command", "command": "image.place",
            "params": {"path": "/a.png", "x": 1, "y": 2, "width": 30, "height": 40}});
    assert_eq!(ghost_rect(&image, (0.0, 0.0), None), [1.0, 2.0, 31.0, 42.0]);
    let batch = json!({"op": "command", "command": "command.batch", "params": {"steps": []}});
    assert_eq!(
        ghost_rect(&batch, (100.0, 100.0), None),
        [-60.0, 0.0, 260.0, 200.0]
    );
}
