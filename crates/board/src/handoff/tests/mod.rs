//! What a handoff writes and asks: for one artboard, one screen, and a flow of two frames.

use std::path::Path;

use figcraft_doc::NodeId;
use serde_json::{Value, json};

use super::{folder, name, targets, write};
use crate::doc::BoardDoc;
use crate::ops;

fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
    let made = board.execute(command, &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

/// An artboard: a phone frame with a title and a button.
fn artboard(board: &mut BoardDoc, x: f64, name: &str) -> NodeId {
    let frame = make(
        board,
        "frame.create",
        json!({"x": x, "y": 0, "width": 393, "height": 852, "name": name, "props": {"fill": "#ffffff"}}),
    );
    make(
        board,
        "text.create",
        json!({"x": 16, "y": 16, "text": "Settings", "parent": frame, "name": "Title", "props": {"fontSize": 24, "fill": "#111827"}}),
    );
    make(
        board,
        "frame.create",
        json!({"x": 16, "y": 80, "width": 160, "height": 48, "parent": frame, "name": "Save button", "props": {"fill": "#1d4ed8", "cornerRadius": 8}}),
    );
    frame
}

/// A screen called `title` with a page in its folder under `screens`.
fn screen(board: &mut BoardDoc, screens: &Path, x: f64, title: &str) -> NodeId {
    let begun = ops::run(
        board,
        &json!({"op": "begin", "name": title, "kind": "screen", "frame": {"x": x, "y": 0, "width": 393, "height": 852}}),
    );
    let name = begun["screen"].as_str().unwrap();
    let folder = crate::screens::folder(screens, name);
    std::fs::create_dir_all(&folder).unwrap();
    let page = "<!doctype html><html><head><style>:root{--accent:#1d4ed8} h1{color:#111827;font-family:Inter}</style></head><body><h1>Profile</h1></body></html>";
    std::fs::write(folder.join("index.html"), page).unwrap();
    begun["frame"].as_str().unwrap().parse().unwrap()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn an_artboard_hands_off_its_picture_code_spec_and_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let screens = dir.path().join(".nyawit/screens");
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let frame = artboard(&mut board, 0.0, "Settings / Mobile");
    let picked = targets(&board.doc().unwrap(), &[frame]).unwrap();
    assert_eq!(name(&picked), "settings-mobile");
    let out = folder(&screens, &name(&picked));
    assert_eq!(out, dir.path().join(".nyawit/handoff/settings-mobile"));
    let handoff = write(&mut board, &picked, &screens, &out, &[]).unwrap();
    for file in [
        "design.png",
        "settings-mobile.html",
        "SettingsMobile.tsx",
        "SettingsMobile.module.css",
        "spec.md",
        "prompt.md",
    ] {
        assert!(out.join(file).is_file(), "no {file} in {:?}", handoff.files);
    }
    let png = std::fs::read(out.join("design.png")).unwrap();
    let picture = image::load_from_memory(&png).unwrap();
    assert_eq!((picture.width(), picture.height()), (786, 1704), "at 2×");
    let spec = read(&handoff.spec);
    for part in [
        "**Settings / Mobile**: artboard, 393 × 852, mobile",
        "**Title**: text “Settings”",
        "#1d4ed8",
        "Radii: 8px",
        "- Title: “Settings”",
        "Only the default state is drawn",
    ] {
        assert!(spec.contains(part), "{part} not in:\n{spec}");
    }
    let prompt = read(&handoff.prompt_file);
    assert_eq!(prompt, handoff.prompt);
    for part in [
        "Build the screen \"Settings / Mobile\" in this project, in its own stack",
        &out.join("design.png").display().to_string(),
        &out.join("spec.md").display().to_string(),
        &out.join("SettingsMobile.tsx").display().to_string(),
        "rather than pasting the exported code",
        "sawit-canvas-design",
        "Sawit's browser",
    ] {
        assert!(prompt.contains(part), "{part} not in:\n{prompt}");
    }
}

#[test]
fn a_screen_hands_off_its_html_and_source() {
    let dir = tempfile::tempdir().unwrap();
    let screens = dir.path().join(".nyawit/screens");
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let frame = screen(&mut board, &screens, 0.0, "Profile");
    let picked = targets(&board.doc().unwrap(), &[frame]).unwrap();
    assert_eq!(picked[0].screen.as_deref(), Some("profile"));
    let out = folder(&screens, &name(&picked));
    let handoff = write(&mut board, &picked, &screens, &out, &[]).unwrap();
    for file in [
        "profile.html",
        "Profile.tsx",
        "Profile.css",
        "spec.md",
        "prompt.md",
    ] {
        assert!(out.join(file).is_file(), "no {file} in {:?}", handoff.files);
    }
    let spec = read(&handoff.spec);
    assert!(
        spec.contains("**Profile**: HTML screen `profile`, 393 × 852, mobile"),
        "{spec}"
    );
    assert!(spec.contains("`--accent`: #1d4ed8"), "{spec}");
    let prompt = &handoff.prompt;
    assert!(
        prompt.contains("the screen \"Profile\" (HTML, screen \"profile\")"),
        "{prompt}"
    );
    assert!(
        prompt.contains(
            &crate::screens::folder(&screens, "profile")
                .display()
                .to_string()
        ),
        "{prompt}"
    );
}

#[test]
fn two_frames_hand_off_as_a_flow_in_canvas_order() {
    let dir = tempfile::tempdir().unwrap();
    let screens = dir.path().join(".nyawit/screens");
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    let second = screen(&mut board, &screens, 600.0, "Profile");
    let first = artboard(&mut board, 0.0, "Settings");
    let picked = targets(&board.doc().unwrap(), &[second, first]).unwrap();
    let order: Vec<&str> = picked.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(order, ["Settings", "Profile"], "left to right");
    assert_eq!(name(&picked), "settings-flow");
    let out = folder(&screens, &name(&picked));
    let handoff = write(&mut board, &picked, &screens, &out, &[]).unwrap();
    for file in [
        "design.png",
        "01-settings.png",
        "01-settings.html",
        "02-profile.html",
        "Settings.tsx",
        "Profile.tsx",
        "spec.md",
        "prompt.md",
    ] {
        assert!(out.join(file).is_file(), "no {file} in {:?}", handoff.files);
    }
    let prompt = &handoff.prompt;
    assert!(
        prompt.starts_with("Build the flow \"Settings\" → \"Profile\" in this project"),
        "{prompt}"
    );
    assert!(
        prompt.contains("- 1. the frame \"Settings\"")
            && prompt.contains("- 2. the screen \"Profile\""),
        "{prompt}"
    );
    let spec = read(&handoff.spec);
    assert!(
        spec.starts_with("# Settings → Profile: handoff spec"),
        "{spec}"
    );
}
