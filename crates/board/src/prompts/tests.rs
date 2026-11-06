//! The dialogs of the commands that need a value: what they ask, the params they make, and that
//! the engine runs those params.

use serde_json::json;

use super::*;
use figcraft_doc::{Document, NodeId};
use figcraft_engine::Session;

use crate::BoardDoc;

fn design_file() -> BoardDoc {
    BoardDoc {
        session: Session::with_document(Document::new("Design")),
    }
}

fn rectangle(board: &mut BoardDoc, name: &str, x: f64) -> NodeId {
    let params =
        json!({"type": "rectangle", "name": name, "x": x, "y": 0, "width": 100, "height": 100});
    let made = board.execute("node.create", &params).unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

fn values(prompt: &Prompt) -> Vec<String> {
    prompt.fields.iter().map(|f| f.value.clone()).collect()
}

#[test]
fn every_prompt_command_has_a_dialog_once_there_is_another_page() {
    let mut board = design_file();
    board
        .execute("page.add", &json!({"name": "Second"}))
        .unwrap();
    for id in PROMPTS {
        let prompt = prompt(&board.session, id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(prompt.command, *id);
        assert!(!prompt.fields.is_empty(), "{id}");
    }
    assert!(prompt(&board.session, "edit.undo").is_none());
    assert!(is_prompt("text.link") && !is_prompt("file.export"));
}

#[test]
fn move_to_page_needs_another_page_and_moves_the_layers_there() {
    let mut board = design_file();
    assert!(
        prompt(&board.session, "object.moveToPage").is_none(),
        "one page: nowhere to go"
    );
    // A new page is the one on show: the layer made there can move back to the first.
    board
        .execute("page.add", &json!({"name": "Second"}))
        .unwrap();
    let r = rectangle(&mut board, "R", 0.0);
    let ask = prompt(&board.session, "object.moveToPage").unwrap();
    let FieldKind::Choice(pages) = &ask.fields[0].kind else {
        panic!("{ask:?}");
    };
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].1, "Page 1");
    let params = params(&ask, &values(&ask), &[r]).unwrap();
    board.execute(ask.command, &params).unwrap();
    let doc = board.doc().unwrap();
    assert_eq!(doc.node(doc.page_of(r).unwrap()).unwrap().name, "Page 1");
}

#[test]
fn batch_rename_renames_every_selected_layer() {
    let mut board = design_file();
    let a = rectangle(&mut board, "Box", 0.0);
    let b = rectangle(&mut board, "Box", 300.0);
    let ask = prompt(&board.session, "object.batchRename").unwrap();
    // Nothing to find and no name: refused.
    assert!(params(&ask, &values(&ask), &[a, b]).is_err());
    let mut typed = values(&ask);
    typed[2] = "Card $n".into();
    let params = params(&ask, &typed, &[a, b]).unwrap();
    board.execute(ask.command, &params).unwrap();
    let doc = board.doc().unwrap();
    let mut names = vec![
        doc.node(a).unwrap().name.clone(),
        doc.node(b).unwrap().name.clone(),
    ];
    names.sort();
    assert_eq!(names, ["Card 1", "Card 2"]);
}

#[test]
fn numbers_are_checked_and_nudge_amounts_are_kept() {
    let mut board = design_file();
    let ask = prompt(&board.session, "pref.nudge").unwrap();
    assert_eq!(values(&ask), ["1", "10"]);
    for bad in [["x", "10"], ["0", "10"], ["1", "1e9"]] {
        let typed: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
        assert!(params(&ask, &typed, &[]).is_err(), "{bad:?}");
    }
    let typed = vec!["2".to_string(), "20".to_string()];
    let params = params(&ask, &typed, &[]).unwrap();
    assert_eq!(params, json!({"small": 2.0, "big": 20.0}));
    board.execute(ask.command, &params).unwrap();
    assert_eq!(
        values(&prompt(&board.session, "pref.nudge").unwrap()),
        ["2", "20"]
    );
}

#[test]
fn a_link_needs_a_url_and_the_profile_reads_the_files() {
    let board = design_file();
    let link = prompt(&board.session, "text.link").unwrap();
    assert!(params(&link, &[" ".into()], &[]).is_err());
    assert_eq!(
        params(&link, &["https://sawit.dev".into()], &[]).unwrap(),
        json!({"url": "https://sawit.dev"})
    );
    let profile = prompt(&board.session, "file.colorProfile").unwrap();
    assert_eq!(values(&profile), ["srgb", "true"]);
    let typed = vec!["displayP3".to_string(), "false".to_string()];
    assert_eq!(
        params(&profile, &typed, &[]).unwrap(),
        json!({"profile": "displayP3", "convert": false})
    );
    assert!(params(&profile, &["cmyk".into(), "true".into()], &[]).is_err());
}
