//! The tests of the store: the helpers the topics share.

use std::path::Path;

use serde_json::json;

use super::*;
use crate::doc::BoardDoc;

mod backups;
mod guard;
mod migration;
mod open;
mod writer;

/// A board with one sticky for each of `texts`.
fn board_with(texts: &[&str]) -> BoardDoc {
    let mut board = BoardDoc::new_board("Canvas").unwrap();
    for (n, text) in texts.iter().enumerate() {
        let sticky = json!({"x": n as f64 * 300.0, "y": 0, "text": text});
        board.execute("board.sticky", &sticky).unwrap();
    }
    board
}

fn bytes_with(texts: &[&str]) -> Vec<u8> {
    board_with(texts).to_bytes().unwrap()
}

fn blank() -> Vec<u8> {
    bytes_with(&[])
}

fn has_text(board: &BoardDoc, text: &str) -> bool {
    let doc = board.doc().unwrap();
    doc.nodes()
        .any(|n| n.kind.text().is_some_and(|t| t.characters == text))
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// How many layers are on the first page, not counting what is inside them.
fn top_level(board: &BoardDoc) -> usize {
    let doc = board.doc().unwrap();
    let page = doc.first_page().unwrap();
    doc.node(page).unwrap().children.len()
}
