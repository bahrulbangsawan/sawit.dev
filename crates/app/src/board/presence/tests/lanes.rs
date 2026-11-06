//! One session, several agents (item 30a): each lane an op names, and each of a session's open
//! works, is a cursor of its own, in its own colour, with its own name; the roster groups them.

use std::collections::HashSet;

use gpui_kit::{Entity, TestAppContext};
use serde_json::{Value, json};

use super::super::marks::Mark;
use super::codex;
use crate::board::BoardView;
use crate::board::tests::{Scratch, next_frame, open_in_window};
use crate::workspace::Answer;

fn run(view: &Entity<BoardView>, op: Value, cx: &mut TestAppContext) -> Value {
    match view.update(cx, |view, cx| view.agent_op(op, Some(&codex()), cx)) {
        Answer::Now(reply) => reply,
        Answer::Later(_) => panic!("a quick op answers at once"),
    }
}

/// The cursors painted now, with their colours.
fn cursors(view: &Entity<BoardView>, cx: &mut TestAppContext) -> Vec<((f64, f64), gpui_kit::Hsla)> {
    view.read_with(cx, |view, cx| {
        let ds = view.board().unwrap().session.ds().unwrap();
        let now = cx.background_executor().now();
        let marks = view
            .agents
            .marks(&ds.doc, &ds.view, ds.page, &Default::default(), now, false);
        marks
            .into_iter()
            .filter_map(|m| match m {
                Mark::Cursor { tip, color, .. } => Some((tip, color)),
                _ => None,
            })
            .collect()
    })
}

/// Four subagents of one Claude Code session, each with its own `agent` id, interleaving their
/// ops: four cursors in four colours, four names, and the session counted once with its four.
#[gpui_kit::test]
fn four_lanes_of_one_session_are_four_cursors(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-lanes");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let names = [
        "Header agent",
        "Hero agent",
        "Pricing agent",
        "Footer agent",
    ];
    for round in 0..2 {
        for (i, name) in names.iter().enumerate() {
            let lane = json!({"id": format!("lane-{i}"), "name": name});
            let x = i as f64 * 400.0;
            let op = json!({"op": "add", "lane": lane, "elements": [
                {"type": "sticky", "x": x, "y": round as f64 * 200.0, "width": 100, "height": 100}]});
            assert_eq!(run(&view, op, cx)["ok"], true);
        }
    }
    view.read_with(cx, |view, _| {
        let agents = &view.agents.agents;
        assert_eq!(agents.len(), 4, "a cursor per lane");
        let labels: HashSet<&str> = agents.iter().map(|a| a.look.label.as_ref()).collect();
        assert_eq!(labels, names.into_iter().collect());
        let session = &agents[0].look.session;
        assert!(agents.iter().all(|a| &a.look.session == session));
        assert_eq!(view.agents.lanes_of(session), 4);
    });
    let painted = cursors(&view, cx);
    assert_eq!(painted.len(), 4);
    let colours: HashSet<String> = painted.iter().map(|(_, c)| format!("{c:?}")).collect();
    assert_eq!(colours.len(), 4, "four colours");
}

/// A session without `agent` that has three works open at once: three cursors, one at each.
#[gpui_kit::test]
fn three_open_works_of_one_session_are_three_cursors(cx: &mut TestAppContext) {
    let tmp = Scratch::new("agent-work-lanes");
    cx.update(gpui_kit::init);
    let (_window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    let mut works = Vec::new();
    for i in 0..3 {
        let frame = json!({"x": i as f64 * 400.0, "y": 0, "width": 240, "height": 180});
        let reply = run(
            &view,
            json!({"op": "begin", "name": format!("Card {i}"), "frame": frame}),
            cx,
        );
        works.push(reply["work"].as_str().unwrap().to_string());
    }
    for work in &works {
        let op = json!({"op": "add", "work": work, "elements": [
            {"type": "text", "x": 10, "y": 10, "text": "Title"}]});
        assert_eq!(run(&view, op, cx)["ok"], true);
    }
    view.read_with(cx, |view, _| {
        let ids: HashSet<&str> = view
            .agents
            .works
            .iter()
            .map(|w| w.look.id.as_ref())
            .collect();
        assert_eq!(ids.len(), 3, "each open work its own agent");
        // The pointers that rest on the works: one at each.
        let at_works: HashSet<&str> = view
            .agents
            .agents
            .iter()
            .filter_map(|a| a.work.as_deref())
            .collect();
        assert_eq!(at_works.len(), 3);
    });
    assert!(cursors(&view, cx).len() >= 3);
}
