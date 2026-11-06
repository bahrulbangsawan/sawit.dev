//! Route frames: the `routes` op, and `begin` with `kind: "routes"`.

use figcraft_doc::{NodeId, NodeKind};
use serde_json::json;

use super::*;
use crate::routes::{WEB_KEY, web_of};

fn id(value: &Value) -> NodeId {
    value.as_str().unwrap().parse().unwrap()
}

/// Each route gets a frame per viewport, in rows, named `<route> · <device> <width>`, inside a section
/// named for the server, all in one undo step; each frame's plugin data names its page.
#[test]
fn routes_are_laid_out_in_a_section_named_for_the_server() {
    let mut board = board();
    let before = board.session.ds().unwrap().history.undo.len();
    let reply = ask(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:5173/", "routes": ["/", "/changelog"],
               "viewports": [1440, 768, 390]}),
    );
    assert_eq!(reply["name"], "localhost:5173");
    let frames = reply["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 6);
    let doc = board.doc().unwrap();
    let section = id(&reply["section"]);
    let node = doc.node(section).unwrap();
    assert!(matches!(node.kind, NodeKind::Section { .. }));
    assert_eq!(node.name, "localhost:5173");
    let names: Vec<String> = (frames.iter())
        .map(|f| doc.node(id(&f["id"])).unwrap().name.clone())
        .collect();
    assert_eq!(
        names,
        [
            "Home · Desktop 1440",
            "Home · Tablet 768",
            "Home · Phone 390",
            "/changelog · Desktop 1440",
            "/changelog · Tablet 768",
            "/changelog · Phone 390"
        ]
    );
    let second = doc.node(id(&frames[3]["id"])).unwrap();
    assert_eq!(second.parent, Some(section));
    let data = web_of(second).unwrap();
    assert_eq!(data.url, "http://localhost:5173/changelog");
    assert_eq!((data.viewport.w, data.viewport.h), (1440.0, 900.0));
    assert_eq!(data.route.as_deref(), Some("/changelog"));
    assert!(data.fit, "a route frame shows its whole page");
    // In the section, under the first row, which the 768 frame makes 1024 tall.
    let s = doc.world_bounds(section).unwrap();
    let b = doc.world_bounds(second.id).unwrap();
    assert_eq!((b.x0 - s.x0, b.y0 - s.y0), (120.0, 200.0 + 1024.0 + 200.0));
    assert!(b.y1 <= s.y1 && b.x1 <= s.x1);
    assert_eq!(board.session.ds().unwrap().history.undo.len(), before + 1);
    assert!(board.session.ds().unwrap().selection.is_empty());
}

#[test]
fn routes_take_chosen_viewports_and_a_place() {
    let mut board = board();
    let reply = ask(
        &mut board,
        json!({"op": "routes", "server": "http://127.0.0.1:3000", "routes": [{"path": "/about"}],
               "viewports": [390, {"w": 1280, "h": 720}, 390], "x": 500, "y": -40, "name": "Site"}),
    );
    let doc = board.doc().unwrap();
    let section = doc.world_bounds(id(&reply["section"])).unwrap();
    assert_eq!((section.x0, section.y0), (500.0, -40.0));
    let frames = reply["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2, "a viewport named twice is laid out once");
    assert_eq!(frames[1]["viewport"], json!({"w": 1280.0, "h": 720.0}));
    assert_eq!(frames[0]["url"], "http://127.0.0.1:3000/about");
}

/// A new section goes right of what the page holds.
#[test]
fn a_new_section_goes_beside_the_page() {
    let mut board = board();
    send(
        &mut board,
        "board.sticky",
        json!({"x": 0, "y": 50, "text": "hi"}),
    );
    let held = board
        .doc()
        .unwrap()
        .page_content_bounds(board.session.page().unwrap())
        .unwrap();
    let reply = ask(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:1", "routes": ["/"]}),
    );
    let section = board
        .doc()
        .unwrap()
        .world_bounds(id(&reply["section"]))
        .unwrap();
    assert_eq!((section.x0, section.y0), (held.x1 + 200.0, held.y0));
}

#[test]
fn remote_pages_and_dynamic_routes_are_refused() {
    let mut board = board();
    let remote = refusal(
        &mut board,
        json!({"op": "routes", "server": "https://example.com", "routes": ["/"]}),
    );
    assert!(remote.contains("example.com isn't on this Mac"), "{remote}");
    let elsewhere = refusal(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:3000",
               "routes": [{"path": "/", "url": "http://evil.com/"}]}),
    );
    assert!(elsewhere.contains("evil.com"), "{elsewhere}");
    for route in ["/blog/[slug]", "/posts/$id", "/users/:id"] {
        let dynamic = refusal(
            &mut board,
            json!({"op": "routes", "server": "http://localhost:3000", "routes": [route]}),
        );
        assert!(dynamic.contains("dynamic route"), "{dynamic}");
    }
    let bad = refusal(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:3000", "routes": ["about"]}),
    );
    assert!(bad.contains("starting with /"), "{bad}");
    let wide = refusal(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:3000", "routes": ["/"], "viewports": [9000]}),
    );
    assert!(wide.contains("200 to 3840"), "{wide}");
    assert!(elements(&mut board).is_empty(), "nothing was made");
}

/// `begin {kind: "routes"}` makes a section, as a work's, with the routes' steps; the `routes`
/// op with its work fills that section and grows it to fit.
#[test]
fn a_routes_work_fills_its_section() {
    let mut board = board();
    let begun = ask(
        &mut board,
        json!({"op": "begin", "name": "localhost:5173", "kind": "routes"}),
    );
    assert_eq!(begun["kind"], "routes");
    assert_eq!(
        begun["steps"],
        json!(["Finding routes", "Adding frames", "Rendering"])
    );
    let section = id(&begun["frame"]);
    let doc = board.doc().unwrap();
    assert!(matches!(
        doc.node(section).unwrap().kind,
        NodeKind::Section { .. }
    ));
    let work = begun["work"].as_str().unwrap();
    let reply = ask(
        &mut board,
        json!({"op": "routes", "work": work, "server": "http://localhost:5173",
               "routes": ["/", "/a", "/b"], "viewports": [390]}),
    );
    assert_eq!(reply["section"], begun["frame"]);
    assert_eq!(reply["work"], work);
    let doc = board.doc().unwrap();
    let b = doc.world_bounds(section).unwrap();
    assert_eq!(
        (b.x1 - b.x0, b.y1 - b.y0),
        (390.0 + 240.0, 320.0 + 3.0 * 844.0 + 2.0 * 200.0)
    );
    let inside = doc.node(section).unwrap().children.len();
    assert_eq!(inside, 3);
    // A component's work has no section to fill.
    let component = ask(
        &mut board,
        json!({"op": "begin", "name": "Card", "frame": {"x": 0, "y": 0}}),
    );
    let refused = refusal(
        &mut board,
        json!({"op": "routes", "work": component["work"], "server": "http://localhost:1",
               "routes": ["/"]}),
    );
    assert!(refused.contains("isn't a routes work"), "{refused}");
    assert!(
        doc.node(id(&reply["frames"][0]["id"]))
            .unwrap()
            .plugin_data
            .contains_key(WEB_KEY)
    );
}

/// With no viewports named, each route gets one desktop frame, 1440 × 900.
#[test]
fn routes_are_desktop_frames_by_default() {
    let mut board = board();
    let reply = ask(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:5173", "routes": ["/", "/pricing"]}),
    );
    let frames = reply["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2);
    let doc = board.doc().unwrap();
    for frame in frames {
        assert_eq!(frame["viewport"], json!({"w": 1440.0, "h": 900.0}));
        let node = doc.node(id(&frame["id"])).unwrap();
        assert_eq!((node.width, node.height), (1440.0, 900.0));
        assert!(node.name.ends_with("· Desktop 1440"), "{}", node.name);
    }
}

/// A frame fitted to its page grows or shrinks, the rows under it move with its bottom, and the
/// section keeps its room under them. The fit is no undo step: the user's undo and redo stay.
#[test]
fn a_frame_fitted_to_its_page_moves_the_rows_under_it() {
    let mut board = board();
    let reply = ask(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:5173", "routes": ["/", "/pricing"]}),
    );
    let frames = reply["frames"].as_array().unwrap();
    let (first, second) = (id(&frames[0]["id"]), id(&frames[1]["id"]));
    let section = id(&reply["section"]);
    let doc = board.doc().unwrap();
    let (top, below, s_before) = (
        doc.world_bounds(first).unwrap(),
        doc.world_bounds(second).unwrap(),
        doc.world_bounds(section).unwrap(),
    );
    let gap = below.y0 - top.y1;
    let undo = board.session.ds().unwrap().history.undo.len();
    assert!(crate::ops::fit_to_page(&mut board, first, 3000.0).unwrap());
    let doc = board.doc().unwrap();
    let grown = doc.world_bounds(first).unwrap();
    assert_eq!((grown.y0, grown.y1 - grown.y0), (top.y0, 3000.0));
    let under = doc.world_bounds(second).unwrap();
    assert_eq!(under.y0, grown.y1 + gap);
    let s = doc.world_bounds(section).unwrap();
    assert_eq!(s.y0, s_before.y0);
    assert_eq!(
        s.y1 - under.y1,
        s_before.y1 - below.y1,
        "the room under the rows stays"
    );
    assert_eq!(
        board.session.ds().unwrap().history.undo.len(),
        undo,
        "no undo step"
    );
    // The same height again: nothing to do.
    assert!(!crate::ops::fit_to_page(&mut board, first, 3000.0).unwrap());
    // A shorter page shrinks it back, and the rows come back up.
    assert!(crate::ops::fit_to_page(&mut board, first, 900.0).unwrap());
    let doc = board.doc().unwrap();
    assert_eq!(doc.world_bounds(first).unwrap(), top);
    assert_eq!(doc.world_bounds(second).unwrap(), below);
    assert_eq!(doc.world_bounds(section).unwrap(), s_before);
}

/// An undo after a fit takes back the user's last step, not the fit, and the redo stays.
#[test]
fn a_fit_keeps_the_undo_and_redo_of_the_user() {
    let mut board = board();
    let reply = ask(
        &mut board,
        json!({"op": "routes", "server": "http://localhost:5173", "routes": ["/"]}),
    );
    let frame = id(&reply["frames"][0]["id"]);
    send(
        &mut board,
        "board.sticky",
        json!({"x": 5000, "y": 0, "text": "a"}),
    );
    board.session.undo().unwrap();
    let (undo, redo) = board.session.undo_depth();
    assert_eq!(redo, 1);
    assert!(crate::ops::fit_to_page(&mut board, frame, 2000.0).unwrap());
    assert_eq!(board.session.undo_depth(), (undo, redo));
}
