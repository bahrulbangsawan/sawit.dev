//! The replay engine: the script's order (parent → child, master → duplicate, reading order),
//! the budgets and the catch-up, lanes that play at once, a change that snaps a replay (undo),
//! the drawn copy, and Fast and Off.

use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId};
use sawit_board::BoardDoc;
use serde_json::{Value, json};

use super::script::{Kind, OP_BUDGET, script};
use super::{Replays, Speed};
use crate::agents::Look;

fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
    let made = board.execute(command, &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

/// A board's document and page before and after `build` runs on it.
fn diff(build: impl FnOnce(&mut BoardDoc)) -> (Arc<Document>, Arc<Document>, NodeId) {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let before = board.doc().unwrap();
    build(&mut board);
    let page = board.session.ds().unwrap().page;
    (before, board.doc().unwrap(), page)
}

/// A frame with a title and three cards in a row, each a copy of the first.
fn cards(board: &mut BoardDoc) -> (NodeId, NodeId, Vec<NodeId>) {
    let frame = make(
        board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 800, "height": 400, "name": "Plans"}),
    );
    let title = make(
        board,
        "text.create",
        json!({"x": 24, "y": 24, "text": "Plans", "parent": frame}),
    );
    let mut cards = Vec::new();
    for i in 0..3 {
        // Made right to left, so the script has to put them in reading order.
        let x = 24.0 + (2 - i) as f64 * 250.0;
        let card = make(
            board,
            "frame.create",
            json!({"x": x, "y": 100, "width": 220, "height": 260, "name": "Card", "parent": frame}),
        );
        make(
            board,
            "text.create",
            json!({"x": 16, "y": 16, "text": format!("Plan {i}"), "parent": card}),
        );
        cards.push(card);
    }
    cards.reverse();
    (frame, title, cards)
}

#[test]
fn the_script_goes_parent_first_master_first_in_reading_order() {
    let mut made = None;
    let (before, after, page) = diff(|b| made = Some(cards(b)));
    let (frame, title, cards) = made.unwrap();
    let tasks = script(&before, &after, page);
    let at = |id: NodeId| tasks.iter().position(|t| t.node == id).unwrap();
    assert_eq!(tasks[0].node, frame, "the parent first");
    assert!(at(title) < at(cards[0]), "the title is above the cards");
    assert!(
        at(cards[0]) < at(cards[1]) && at(cards[1]) < at(cards[2]),
        "left to right"
    );
    assert_eq!(tasks[at(cards[0])].kind, Kind::Frame, "the master is drawn");
    assert!(
        matches!(tasks[at(cards[1])].kind, Kind::Duplicate { .. }),
        "the rest are copies"
    );
    assert!(matches!(tasks[at(cards[2])].kind, Kind::Duplicate { .. }));
    // A copy's text is retyped after the copy lands; the copy's shapes come with it.
    let copy_text = tasks
        .iter()
        .filter(|t| matches!(t.kind, Kind::Text { retype: true, .. }))
        .count();
    assert_eq!(copy_text, 2);
}

#[test]
fn an_op_plays_within_its_budget_and_a_busy_lane_catches_up() {
    let (before, after, page) = diff(|b| {
        for i in 0..40 {
            make(
                b,
                "frame.create",
                json!({"x": i * 120, "y": 0, "width": 100, "height": 100}),
            );
        }
    });
    let total: Duration = script(&before, &after, page).iter().map(|t| t.time).sum();
    assert!(total <= OP_BUDGET + Duration::from_millis(1), "{total:?}");
    let now = Instant::now();
    let mut replays = Replays::default();
    let look = Look::for_tests("a");
    let play = |r: &mut Replays, at: Instant| {
        r.play(&look, &before, &after, page, None, (0.0, 0.0), at, false)
    };
    play(&mut replays, now);
    let end = |r: &Replays| r.lanes[0].tasks.last().unwrap().end;
    let first = end(&replays) - now;
    // A second op while it plays: what is left goes at 2×, the new op after it at 2×.
    play(&mut replays, now);
    assert!(end(&replays) - now < first * 2, "sped up");
    play(&mut replays, now);
    play(&mut replays, now);
    // The third time it snaps: only the newest op is left.
    assert!(
        replays.lanes[0].tasks.len() <= 40,
        "{}",
        replays.lanes[0].tasks.len()
    );
}

#[test]
fn four_lanes_of_one_session_play_at_once() {
    let (before, after, page) = diff(|b| {
        make(
            b,
            "frame.create",
            json!({"x": 0, "y": 0, "width": 100, "height": 100}),
        );
    });
    let now = Instant::now();
    let mut replays = Replays::default();
    for i in 0..4 {
        let look = Look::for_tests(&format!("s/lane-{i}"));
        replays.play(&look, &before, &after, page, None, (0.0, 0.0), now, false);
    }
    let queues = replays.queues(now);
    assert_eq!(queues.len(), 4, "a queue per lane");
    assert!(queues.iter().all(|(_, n)| *n == 1));
    // Not one after another: every lane is playing a moment later.
    let soon = now + Duration::from_millis(200);
    for (id, _) in &queues {
        assert!(replays.pointer(id, soon).is_some(), "{id} plays");
    }
}

#[test]
fn the_drawn_copy_hides_what_isnt_placed_and_snaps_on_undo() {
    let mut frame = None;
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let before = board.doc().unwrap();
    frame.replace(make(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 100, "height": 100}),
    ));
    let after = board.doc().unwrap();
    let page = board.session.ds().unwrap().page;
    let now = Instant::now();
    let mut replays = Replays::default();
    let look = Look::for_tests("a");
    replays.play(
        &look,
        &before,
        &after,
        page,
        None,
        (500.0, 500.0),
        now,
        false,
    );
    let drawn = replays.document(&after, &[], now).expect("a copy is drawn");
    assert!(
        !drawn.node(frame.unwrap()).unwrap().visible,
        "not placed yet"
    );
    assert!(
        after.node(frame.unwrap()).unwrap().visible,
        "the document is whole"
    );
    assert!(
        replays
            .document(&after, &[], now + Duration::from_secs(5))
            .is_none(),
        "done"
    );
    // Undo while it plays: the document no longer has the frame as the op left it.
    replays.play(
        &look,
        &before,
        &after,
        page,
        None,
        (500.0, 500.0),
        now,
        false,
    );
    assert!(replays.document(&before, &[], now).is_none(), "cancelled");
    assert!(!replays.active(now));
}

#[test]
fn fast_halves_the_play_off_plays_nothing_and_reduced_motion_only_fades() {
    let (before, after, page) = diff(|b| {
        make(
            b,
            "frame.create",
            json!({"x": 0, "y": 0, "width": 100, "height": 100}),
        );
        make(
            b,
            "text.create",
            json!({"x": 0, "y": 200, "text": "Hello there"}),
        );
    });
    let now = Instant::now();
    let look = Look::for_tests("a");
    let length = |speed: Speed, reduce: bool| {
        let mut r = Replays {
            speed,
            ..Replays::default()
        };
        r.play(&look, &before, &after, page, None, (0.0, 0.0), now, reduce);
        r.lanes
            .first()
            .and_then(|l| l.tasks.last())
            .map(|t| t.end - now)
    };
    let full = length(Speed::Full, false).unwrap();
    let fast = length(Speed::Fast, false).unwrap();
    assert!(
        (fast.as_secs_f64() - full.as_secs_f64() / 2.0).abs() < 0.01,
        "{full:?} {fast:?}"
    );
    assert_eq!(length(Speed::Off, false), None);
    assert_eq!(
        length(Speed::Full, true),
        Some(std::time::Duration::from_millis(150)),
        "reduced motion: a short fade only"
    );
}
