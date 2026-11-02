//! UI screens: `begin` with `kind: "screen"`, and the `screen` op.

use figcraft_doc::NodeId;
use serde_json::json;

use super::*;
use crate::screens::{SCREEN_KEY, screen_of};

/// A screen's frame is a work's frame that names its folder and viewport; its name is free of
/// the screens on the canvas and of the folders the caller says are taken.
#[test]
fn a_screen_begins_with_a_folder_name_of_its_own() {
    let mut board = board();
    let begin = |taken: serde_json::Value| {
        json!({"op": "begin", "name": "Sign in", "kind": "screen", "taken": taken,
               "frame": {"x": 0, "y": 0, "width": 390, "height": 844}})
    };
    let first = ask(&mut board, begin(json!([])));
    assert_eq!(first["screen"], "sign-in");
    assert_eq!(first["kind"], "screen");
    assert_eq!(first["viewport"], json!({"w": 390.0, "h": 844.0}));
    let frame: NodeId = first["frame"].as_str().unwrap().parse().unwrap();
    let doc = board.doc().unwrap();
    let node = doc.node(frame).unwrap();
    assert!(node.plugin_data.contains_key(SCREEN_KEY));
    let data = screen_of(node).unwrap();
    assert_eq!((data.screen.as_str(), data.viewport.w), ("sign-in", 390.0));
    let second = ask(&mut board, begin(json!(["sign-in-2"])));
    assert_eq!(second["screen"], "sign-in-3", "{second}");
    let size =
        json!({"op": "begin", "name": "Home", "kind": "screen", "frame": {"x": 0, "y": 900}});
    assert_eq!(
        ask(&mut board, size)["viewport"],
        json!({"w": 1440.0, "h": 900.0})
    );
}

/// The `screen` op finds a screen by its work, its folder or its frame's name, lists them all,
/// and says what there is when the name is none.
#[test]
fn screens_are_found_by_work_or_name() {
    let mut board = board();
    let begin = json!({"op": "begin", "name": "Pricing page", "kind": "screen",
                       "frame": {"x": 0, "y": 0, "width": 800, "height": 600}});
    let made = ask(&mut board, begin);
    let work = made["work"].as_str().unwrap();
    for op in [
        json!({"op": "screen", "work": work}),
        json!({"op": "screen", "screen": "pricing-page"}),
        json!({"op": "screen", "screen": "Pricing Page"}),
    ] {
        let found = ask(&mut board, op);
        assert_eq!(found["screen"], "pricing-page", "{found}");
        assert_eq!(found["frame"], made["frame"]);
        assert_eq!(found["viewport"], json!({"w": 800.0, "h": 600.0}));
    }
    let all = ask(&mut board, json!({"op": "screen"}));
    assert_eq!(all["screens"].as_array().unwrap().len(), 1);
    let none = refusal(&mut board, json!({"op": "screen", "screen": "login"}));
    assert!(none.contains("the canvas has pricing-page"), "{none}");
    let card = json!({"op": "begin", "name": "Card", "frame": {"x": 0, "y": 700}});
    let card = ask(&mut board, card)["work"].as_str().unwrap().to_string();
    let component = refusal(&mut board, json!({"op": "screen", "work": card}));
    assert!(
        component.contains("is a component, not a screen"),
        "{component}"
    );
}

/// An export paints each screen's picture in its frame, on a copy: the board itself, its undo
/// history and its selection stay as they were.
#[test]
fn exports_paint_screens_pictures_in_their_frames() {
    let mut board = board();
    let begin = json!({"op": "begin", "name": "Home", "kind": "screen",
                       "frame": {"x": 0, "y": 0, "width": 40, "height": 20}});
    let frame: NodeId = ask(&mut board, begin)["frame"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mut red = image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]));
    red.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    let mut png = std::io::Cursor::new(Vec::new());
    red.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let before = board.doc().unwrap();
    let undo = board.session.ds().unwrap().history.undo.len();
    let reply = crate::ops::export_with_pictures(
        &board,
        &json!({"op": "export_png"}),
        &[(frame, png.into_inner())],
    );
    let bytes = {
        use base64::Engine as _;
        let b64 = reply["png_base64"].as_str().expect("a PNG");
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap()
    };
    let exported = image::load_from_memory(&bytes).unwrap().to_rgba8();
    let (w, h) = exported.dimensions();
    let middle = exported.get_pixel(w / 2, h / 2);
    assert!(
        middle[0] > 200 && middle[1] < 60 && middle[2] < 60,
        "{middle:?}"
    );
    assert!(
        std::sync::Arc::ptr_eq(&before, &board.doc().unwrap()),
        "the board is as it was"
    );
    assert_eq!(board.session.ds().unwrap().history.undo.len(), undo);
}
