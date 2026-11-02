//! Work items: `begin`, `progress`, ops that name a `work`, `update`'s `append`, and the
//! commands agents may not run.

use figcraft_doc::NodeId;
use sawit_core::agents::work::{WORK_KEY, WORK_STEPS_KEY};
use serde_json::json;

use super::*;

/// Begins work `name` at (`x`, `y`), 300 × 200, and gives its id and frame.
fn begun(board: &mut BoardDoc, name: &str, x: f64, y: f64) -> (String, NodeId) {
    let frame = json!({"x": x, "y": y, "width": 300, "height": 200});
    let reply = ask(board, json!({"op": "begin", "name": name, "frame": frame}));
    let frame: NodeId = reply["frame"].as_str().unwrap().parse().unwrap();
    (reply["work"].as_str().unwrap().to_string(), frame)
}

/// The page box of layer `id`, `[x, y, width, height]`.
fn bounds(board: &BoardDoc, id: NodeId) -> [f64; 4] {
    let b = board.doc().unwrap().world_bounds(id).unwrap();
    [b.x0, b.y0, b.width(), b.height()]
}

/// `begin` makes a named frame, marked as the work's with its steps, in one undo step, and leaves
/// the selection as it is.
#[test]
fn begin_makes_a_named_frame_for_the_work() {
    let mut board = board();
    let sticky = send(
        &mut board,
        "board.sticky",
        json!({"x": 0, "y": 0, "text": "mine"}),
    );
    let mine = id_of(&sticky["result"]["id"]).unwrap();
    board.session.set_selection(vec![mine]);
    let steps = json!(["Laying out", "Writing copy"]);
    let frame = json!({"x": 400, "y": 40, "width": 320, "height": 240});
    let op = json!({"op": "begin", "name": "Pricing card", "frame": frame, "steps": steps});
    let reply = ask(&mut board, op);
    let work = reply["work"].as_str().unwrap();
    assert!(work.starts_with("w_"), "{reply}");
    assert_eq!(reply["name"], "Pricing card");
    assert_eq!(reply["steps"], steps);
    let frame: NodeId = reply["frame"].as_str().unwrap().parse().unwrap();
    let doc = board.doc().unwrap();
    let node = doc.node(frame).unwrap();
    assert_eq!(node.name, "Pricing card");
    assert_eq!(node.plugin_data[WORK_KEY], work);
    assert_eq!(node.plugin_data[WORK_STEPS_KEY], steps.to_string());
    assert_eq!(reply["page"], doc.page_of(frame).unwrap().to_string());
    assert_eq!(work_frame(&doc, work), Some(frame));
    assert_eq!(work_steps(&doc, frame), ["Laying out", "Writing copy"]);
    assert_eq!(bounds(&board, frame), [400.0, 40.0, 320.0, 240.0]);
    assert_eq!(
        board.session.selection(),
        [mine],
        "the user's selection stays"
    );
    send(&mut board, "edit.undo", json!({}));
    assert!(board.doc().unwrap().node(frame).is_none(), "one undo step");
    // Two works never share an id.
    let (one, _) = begun(&mut board, "One", 0.0, 0.0);
    let (two, _) = begun(&mut board, "Two", 0.0, 400.0);
    assert_ne!(one, two);
}

/// A frame `near` a layer goes beside it, on the side asked, as big as asked or 320 × 240.
#[test]
fn begin_places_a_frame_near_a_layer() {
    let mut board = board();
    add(
        &mut board,
        json!([{"type": "rectangle", "id": "hero", "x": 0, "y": 0, "width": 200, "height": 100}]),
    );
    let near =
        |side: &str| json!({"op": "begin", "name": side, "frame": {"near": "hero", "side": side}});
    let mut placed = |side: &str| {
        let reply = ask(&mut board, near(side));
        let frame = reply["frame"].as_str().unwrap().parse().unwrap();
        bounds(&board, frame)
    };
    assert_eq!(placed("right"), [248.0, 0.0, 320.0, 240.0]);
    assert_eq!(placed("below"), [0.0, 148.0, 320.0, 240.0]);
    assert_eq!(placed("left"), [-368.0, 0.0, 320.0, 240.0]);
    assert_eq!(placed("above"), [0.0, -288.0, 320.0, 240.0]);
}

#[test]
fn begin_says_what_it_needs() {
    let mut board = board();
    let frame = json!({"x": 0, "y": 0});
    let no_name = refusal(&mut board, json!({"op": "begin", "frame": frame}));
    assert!(
        no_name.starts_with("begin: it needs the component's `name`"),
        "{no_name}"
    );
    let no_frame = refusal(&mut board, json!({"op": "begin", "name": "Card"}));
    assert!(no_frame.contains("it needs a `frame`"), "{no_frame}");
    let half = refusal(
        &mut board,
        json!({"op": "begin", "name": "Card", "frame": {"x": 1}}),
    );
    assert!(half.contains("`x` and `y`"), "{half}");
    let screen = json!({"op": "begin", "name": "Home", "frame": frame, "kind": "screen"});
    assert!(refusal(&mut board, screen).contains("isn't built yet"));
    let nowhere = json!({"op": "begin", "name": "Card", "frame": {"near": "nope"}});
    assert!(refusal(&mut board, nowhere).contains("no layer has the id “nope”"));
    let steps = json!({"op": "begin", "name": "Card", "frame": frame, "steps": [1]});
    assert!(refusal(&mut board, steps).contains("`steps` must be a list of words"));
    assert!(elements(&mut board).is_empty(), "nothing was made");
}

/// What an op that names a work adds goes into the work's frame, placed from its corner, and an
/// update moves from the corner too; with `append`, text grows at its end.
#[test]
fn ops_of_a_work_go_into_its_frame() {
    let mut board = board();
    let (work, frame) = begun(&mut board, "Card", 400.0, 100.0);
    let title = json!({"type": "text", "id": "title", "x": 16, "y": 12, "text": "Pro plan"});
    let note = json!({"type": "sticky", "x": 40, "y": 60, "text": "Seats"});
    let reply = ask(
        &mut board,
        json!({"op": "add", "work": work, "elements": [title, note]}),
    );
    assert_eq!(reply["work"], json!(work));
    let ids: Vec<NodeId> = (reply["ids"].as_array().unwrap().iter())
        .map(|id| id.as_str().unwrap().parse().unwrap())
        .collect();
    let doc = board.doc().unwrap();
    for id in &ids {
        assert_eq!(doc.node(*id).unwrap().parent, Some(frame), "in the frame");
    }
    assert_eq!(bounds(&board, ids[0])[..2], [416.0, 112.0]);
    let moved = json!({"op": "update", "work": work, "elements": [{"id": "title", "x": 20}]});
    ask(&mut board, moved);
    assert_eq!(bounds(&board, ids[0])[..2], [420.0, 112.0]);
    let more = json!({"id": "title", "append": " for teams of five."});
    ask(
        &mut board,
        json!({"op": "update", "work": work, "elements": [more]}),
    );
    assert_eq!(listed_text(&board, ids[0]), "Pro plan for teams of five.");
    let set = json!({"id": "title", "text": "Team", "append": " plan"});
    ask(&mut board, json!({"op": "update", "elements": [set]}));
    assert_eq!(listed_text(&board, ids[0]), "Team plan");
    // A work that isn't there changes nothing.
    let gone = json!({"op": "add", "work": "w_nope", "elements": [{"type": "sticky"}]});
    assert!(refusal(&mut board, gone).contains("work “w_nope” isn't on the canvas"));
    let wrong = json!({"op": "delete", "work": 3, "ids": ["title"]});
    assert!(refusal(&mut board, wrong).contains("`work` must be the id"));
}

/// The text of text layer `id`.
fn listed_text(board: &BoardDoc, id: NodeId) -> String {
    let doc = board.doc().unwrap();
    doc.node(id)
        .unwrap()
        .kind
        .text()
        .unwrap()
        .characters
        .clone()
}

/// A work's frame on another page than the one on show still takes its ops, and the page on
/// show stays.
#[test]
fn a_work_on_another_page_is_built_without_showing_it() {
    let mut board = board();
    let (work, frame) = begun(&mut board, "Card", 0.0, 0.0);
    let first = board.session.page().unwrap();
    send(&mut board, "page.add", json!({"name": "Two"}));
    let two = board.session.page().unwrap();
    assert_ne!(first, two);
    let note = json!({"type": "sticky", "x": 10, "y": 10, "text": "here"});
    let reply = ask(
        &mut board,
        json!({"op": "add", "work": work, "elements": [note]}),
    );
    let id: NodeId = reply["ids"][0].as_str().unwrap().parse().unwrap();
    assert_eq!(board.doc().unwrap().node(id).unwrap().parent, Some(frame));
    assert_eq!(board.session.page().unwrap(), two, "the page on show stays");
}

/// A diagram drawn for a work lands in its frame, from its corner.
#[test]
fn a_diagram_of_a_work_goes_into_its_frame() {
    let mut board = board();
    let (work, frame) = begun(&mut board, "Flow", 500.0, 0.0);
    let op = json!({"op": "mermaid", "work": work, "source": "graph TD\nA-->B", "x": 10, "y": 10});
    let reply = ask(&mut board, op);
    let doc = board.doc().unwrap();
    let inside = (reply["ids"].as_array().unwrap().iter())
        .filter_map(|id| id.as_str()?.parse::<NodeId>().ok())
        .filter(|id| doc.node(*id).is_some_and(|n| n.parent == Some(frame)))
        .count();
    assert!(inside >= 2, "the boxes are in the frame: {reply}");
}

/// With the app closed, progress checks the work and its status, and says nothing shows it.
#[test]
fn progress_with_the_app_closed_says_nothing_shows_it() {
    let mut board = board();
    let (work, _) = begun(&mut board, "Card", 0.0, 0.0);
    let before = board.doc().unwrap();
    let op = json!({"op": "progress", "work": work, "step": "Styling", "status": "working"});
    let reply = ask(&mut board, op);
    assert_eq!(reply["shown"], false);
    assert_eq!(reply["status"], "working");
    assert_eq!(reply["note"], PROGRESS_UNSHOWN);
    assert!(
        std::sync::Arc::ptr_eq(&before, &board.doc().unwrap()),
        "nothing changed"
    );
    let unknown = refusal(&mut board, json!({"op": "progress", "work": "w_0"}));
    assert!(
        unknown.starts_with("progress: work “w_0” isn't on the canvas"),
        "{unknown}"
    );
    let wrong = json!({"op": "progress", "work": work, "status": "finished"});
    assert!(refusal(&mut board, wrong).contains("working, done or failed"));
}

/// Agents may not move the user's view or page, in a batch either; other commands go.
#[test]
fn agents_may_not_move_the_view_or_the_page() {
    let command =
        |id: &str, params: Value| json!({"op": "command", "command": id, "params": params});
    let zoom = agent_refusal(&command("view.zoomIn", json!({}))).unwrap();
    assert!(zoom.starts_with("view.zoomIn moves the view"), "{zoom}");
    assert!(agent_refusal(&command("page.select", json!({"index": 0}))).is_some());
    let steps = json!({"commands": [
        {"command": "board.sticky", "params": {}},
        {"command": "page.select", "params": {"index": 1}},
    ]});
    assert!(agent_refusal(&command("command.batch", steps)).is_some());
    assert_eq!(agent_refusal(&command("board.sticky", json!({}))), None);
    assert_eq!(agent_refusal(&command("page.list", json!({}))), None);
    assert_eq!(agent_refusal(&json!({"op": "get"})), None);
}
