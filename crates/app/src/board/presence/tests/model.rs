//! What the canvas shows of agents, worked out without a window: where pointers go and glide,
//! placeholders, the chips at the edges, the names beside free pointers, and announcements.

use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use sawit_core::agents::GONE_AFTER;
use serde_json::{Value, json};

use super::super::announce::{Announcer, HOLD};
use super::super::edges::{Toward, edge_of};
use super::super::labels::{kept, pill_rect};
use super::super::marks::Mark;
use super::super::target::{Target, ghost_rect, target, union};
use super::super::{ANNOUNCE_GAP, EDGE_INSET, FAILED_TIME, GLIDE, Presence, TOUCHED_TIME};
use super::look;

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
    let [_, y0, x1, _] = union(&target.rects).unwrap();
    assert_eq!(target.point, (x1, y0), "the top-right corner of both");
    assert_eq!(y0, 300.0);
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
    let [[x0, y0, x1, _]] = target.rects[..] else {
        panic!("one box: {:?}", target.rects);
    };
    assert_eq!((x0, y0), (1000.0, 0.0));
    assert!(x1 >= 1000.0 + 11.0 * 120.0 + 100.0 - 1.0, "{x1}");
    assert_eq!(target.point, (x1, 0.0));
}

/// For an update it goes outside the top-right corner of what changed: one layer's, or the box
/// around several.
#[test]
fn the_pointer_goes_to_the_corner_of_what_an_update_changed() {
    let (before, after, page) =
        before_after(json!({"op": "update", "elements": [{"id": "b", "x": 600, "y": 200}]}));
    let one = target(&before, &after, page).expect("a target");
    assert_eq!(one.rects.len(), 1);
    let [x0, y0, x1, y1] = one.rects[0];
    assert_eq!(one.point, (x1, y0));
    assert!(y1 > y0);
    assert!(
        x0 >= 599.0 && y0 >= 199.0,
        "where it moved to: {:?}",
        one.rects[0]
    );

    let (before, after, page) = before_after(json!({"op": "update", "elements": [
            {"id": "a", "x": 0, "y": 100}, {"id": "b", "x": 400, "y": 100}]}));
    let both = target(&before, &after, page).expect("a target");
    assert_eq!(both.rects.len(), 2);
    let [_, y0, x1, _] = union(&both.rects).unwrap();
    assert_eq!(both.point, (x1, y0));
}

/// A deletion leaves no layer to outline: the pointer goes where the layers were.
#[test]
fn the_pointer_goes_where_a_deletion_took_layers_off() {
    let (before, after, page) = before_after(json!({"op": "delete", "ids": ["b"]}));
    let target = target(&before, &after, page).expect("a target");
    assert!(target.rects.is_empty());
    assert!(target.point.0 > 400.0, "{:?}", target.point);
    assert_eq!(
        super::super::target::target(&before, &before, page),
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
    presence.point_at(&a, page, at((0.0, 0.0)), None, t0);
    assert_eq!(presence.agents[0].point(t0, false), Some((0.0, 0.0)));
    let t1 = t0 + Duration::from_secs(1);
    presence.point_at(&a, page, at((100.0, 0.0)), None, t1);
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
    assert_eq!(presence.spot("a").map(|s| s.point), Some((330.0, 20.0)));
    let blank = Document::default();
    let view = ViewState {
        vw: 800.0,
        vh: 600.0,
        ..ViewState::default()
    };
    let ghosts = |p: &Presence| {
        p.marks(&blank, &view, page, t0, false)
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
            .marks(&blank, &view, page, t0, false)
            .iter()
            .any(|m| matches!(m, Mark::Ghost { failed: true, .. }))
    );
    presence.prune(t0 + FAILED_TIME);
    assert_eq!(ghosts(&presence), 0, "the error mark goes after a moment");
}

/// An agent at work off screen is past the edge it leaves the canvas by, and agents past one
/// edge share one chip there, kept clear of the toolbar.
#[test]
fn off_screen_agents_share_a_chip_per_edge() {
    let size = (800.0, 600.0);
    assert_eq!(edge_of((400.0, 300.0), size), None, "on screen");
    assert_eq!(edge_of((2000.0, 300.0), size), Some((Toward::Right, 300.0)));
    assert_eq!(edge_of((400.0, -900.0), size), Some((Toward::Up, 400.0)));
    assert_eq!(edge_of((-50.0, 10.0), size).unwrap().0, Toward::Left);

    let mut presence = Presence::default();
    let page = NodeId(1);
    let t0 = Instant::now();
    let at = |point| Target {
        point,
        rects: vec![],
    };
    let (a, b, c) = (look("a"), look("b"), look("c"));
    presence.point_at(&a, page, at((2000.0, 100.0)), None, t0);
    presence.point_at(
        &b,
        page,
        at((2500.0, -200.0)),
        None,
        t0 + Duration::from_millis(10),
    );
    presence.point_at(&c, page, at((0.0, 5000.0)), None, t0);
    let view = ViewState {
        vw: 800.0,
        vh: 600.0,
        ..ViewState::default()
    };
    let edges = presence.edges(&view, page, size, t0 + Duration::from_millis(20));
    assert_eq!(edges.len(), 2, "{edges:?}");
    let right = &edges[0];
    assert_eq!(right.toward, Toward::Right);
    let ids: Vec<&str> = right.agents.iter().map(|(l, ..)| l.id.as_ref()).collect();
    assert_eq!(ids, ["b", "a"], "both on one chip, the most recent first");
    let below = &edges[1];
    assert_eq!(below.toward, Toward::Down);
    assert!(
        below.along >= EDGE_INSET && below.along <= size.0,
        "{}",
        below.along
    );
}

/// Free pointers' names: a pill beside the tip, and of two that would cover each other, the more
/// recent agent's (painted last) is the one kept.
#[test]
fn a_name_that_would_cover_another_is_left_out() {
    let one = pill_rect((100.0, 100.0), 40.0);
    assert_eq!(one, [111.0, 105.0, 165.0, 123.0]);
    // As long as a pill is, it is at most 120 px.
    assert_eq!(pill_rect((0.0, 0.0), 400.0)[2], 131.0);
    let near = pill_rect((110.0, 108.0), 40.0);
    let far = pill_rect((400.0, 100.0), 40.0);
    assert_eq!(kept(&[one, near, far]), [false, true, true]);
}

/// Each change is said in turn: two agents at once don't talk over each other, each sentence is
/// held a moment, and an agent's ops outside a component are said at most once per gap.
#[test]
fn announcements_take_turns() {
    let mut said = Announcer::default();
    let t0 = Instant::now();
    let (a, b) = (look("a").id, look("b").id);
    said.say(&a, "A started Card".into(), t0);
    said.say(&b, "B started Nav".into(), t0);
    said.say(&a, "A finished Card".into(), t0);
    assert_eq!(said.current.as_ref(), "A started Card");
    assert!(!said.tick(t0 + HOLD / 2), "held");
    assert!(said.tick(t0 + HOLD));
    assert_eq!(
        said.current.as_ref(),
        "B started Nav",
        "the other agent's turn"
    );
    said.tick(t0 + HOLD * 2);
    assert_eq!(said.current.as_ref(), "A finished Card");
    assert!(!said.pending());
    let later = t0 + HOLD * 3;
    said.op(&a, "A added a sticky".into(), later);
    said.op(
        &a,
        "A moved a layer".into(),
        later + Duration::from_millis(500),
    );
    assert_eq!(said.current.as_ref(), "A added a sticky");
    assert!(!said.pending(), "the second op was too soon to say");
    said.op(&a, "A moved a layer".into(), later + ANNOUNCE_GAP);
    assert!(said.pending() || said.current.as_ref() == "A moved a layer");
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
