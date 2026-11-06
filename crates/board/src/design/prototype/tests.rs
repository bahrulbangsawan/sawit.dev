//! Prototyping: interactions, connections, flows and the player.

use serde_json::json;

use super::*;
use crate::design::lists::testing::{apply, design_file};

fn frame(board: &mut crate::BoardDoc, name: &str, x: f64) -> NodeId {
    let made = board
        .execute(
            "frame.create",
            &json!({"x": x, "y": 0, "width": 200, "height": 400, "name": name}),
        )
        .unwrap();
    figcraft_engine::params::id_of(&made["id"]).unwrap()
}

fn button(board: &mut crate::BoardDoc, parent: NodeId) -> NodeId {
    let made = board
        .execute(
            "node.create",
            &json!({"type": "rectangle", "x": 20, "y": 20, "width": 80, "height": 30, "parent": parent}),
        )
        .unwrap();
    figcraft_engine::params::id_of(&made["id"]).unwrap()
}

#[test]
fn an_interaction_is_added_edited_and_drawn_as_a_connection() {
    let mut board = design_file();
    let (home, detail) = (
        frame(&mut board, "Home", 0.0),
        frame(&mut board, "Detail", 400.0),
    );
    let go = button(&mut board, home);
    apply(&mut board, &add(go, Some(detail)));
    let doc = board.doc().unwrap();
    let rows = interactions(&doc, go);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].trigger.as_str(), rows[0].action.as_str()),
        ("onClick", "navigate")
    );
    assert_eq!(rows[0].dest, Some(detail));
    let page = doc.page_of(go).unwrap();
    let names: Vec<String> = destinations(&doc, page).into_iter().map(|d| d.1).collect();
    assert_eq!(names, ["Home", "Detail"]);
    let drawn = links(&doc, page);
    assert_eq!(drawn.len(), 1);
    assert_eq!((drawn[0].from, drawn[0].to), (go, detail));

    apply(&mut board, &set_trigger(go, 0, "onHover"));
    apply(&mut board, &set_transition(go, 0, "dissolve"));
    apply(&mut board, &set_duration(go, 0, 450.0).unwrap());
    apply(&mut board, &set_action(go, 0, "back", None, None).unwrap());
    let rows = interactions(&board.doc().unwrap(), go);
    assert_eq!(rows[0].trigger, "onHover");
    assert_eq!(rows[0].transition, "dissolve");
    assert_eq!(rows[0].duration_ms, 450);
    assert_eq!(rows[0].action, "back");
    assert!(
        set_action(go, 0, "navigate", None, None).is_none(),
        "navigate needs a frame"
    );
    assert!(set_duration(go, 0, -1.0).is_none());
    apply(&mut board, &remove(go, 0));
    assert!(interactions(&board.doc().unwrap(), go).is_empty());
}

#[test]
fn the_player_follows_clicks_and_goes_back() {
    let mut board = design_file();
    let (home, detail) = (
        frame(&mut board, "Home", 0.0),
        frame(&mut board, "Detail", 400.0),
    );
    let go = button(&mut board, home);
    let back = button(&mut board, detail);
    apply(&mut board, &add(go, Some(detail)));
    apply(&mut board, &add(back, None));
    apply(&mut board, &add_flow(home));
    let doc = board.doc().unwrap();
    let page = doc.page_of(home).unwrap();
    assert_eq!(flows(&doc, page).len(), 1);
    let mut player = Player::new(&doc, go).unwrap();
    assert_eq!(player.frame, home);
    assert_eq!(player.click(&doc, 199.0, 399.0), Played::Nothing);
    assert_eq!(player.click(&doc, 30.0, 30.0), Played::Moved);
    assert_eq!(player.frame, detail);
    assert_eq!(player.click(&doc, 430.0, 30.0), Played::Moved);
    assert_eq!(player.frame, home);
    apply(&mut board, &remove_flow(home));
    assert!(flows(&board.doc().unwrap(), page).is_empty());
}
