//! The keyboard's and the pointer's tests: the keys held, the files they run on, and the helpers
//! the topics share.

use figcraft_doc::NodeId;
use figcraft_engine::{PointerEvent, PointerKind, Session, UiRequest, vectoredit};
use serde_json::json;

use super::*;
use crate::BoardDoc;

mod keys;
mod pointer;
mod text;
mod vector_edit;

const NONE: Mods = Mods {
    shift: false,
    alt: false,
    cmd: false,
    ctrl: false,
};
const CMD: Mods = Mods { cmd: true, ..NONE };
const SHIFT: Mods = Mods {
    shift: true,
    ..NONE
};

fn board() -> BoardDoc {
    BoardDoc::new_board("Canvas").unwrap()
}

/// A new sticky reading `text`.
fn sticky(board: &mut BoardDoc, text: &str) -> NodeId {
    let made = board
        .execute("board.sticky", &json!({"x": 0, "y": 0, "text": text}))
        .unwrap();
    figcraft_engine::params::id_of(&made["id"]).unwrap()
}

/// What sticky `id` reads.
fn sticky_text(board: &BoardDoc, id: NodeId) -> String {
    let doc = board.doc().unwrap();
    let text = figcraft_engine::board::text_of(&doc, id).unwrap();
    let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
    text.characters.clone()
}

fn editing(board: &BoardDoc) -> bool {
    board.session.ds().unwrap().text_edit.is_some()
}

/// The text edit in progress.
fn text_edit(board: &BoardDoc) -> figcraft_engine::TextEdit {
    board.session.ds().unwrap().text_edit.clone().unwrap()
}

fn select(board: &mut BoardDoc, id: NodeId) {
    board.execute("select.set", &json!({"ids": [id]})).unwrap();
}

fn tool(board: &BoardDoc) -> &'static str {
    board.session.tool.tool.id()
}

fn page_is_empty(board: &BoardDoc) -> bool {
    let page = board.session.page().unwrap();
    board.doc().unwrap().node(page).unwrap().children.is_empty()
}

/// The points and segments of path `id`.
fn network(board: &BoardDoc, id: NodeId) -> figcraft_doc::VectorNetwork {
    vectoredit::network(&board.doc().unwrap(), id)
        .unwrap()
        .clone()
}

/// A new, empty design file.
fn design() -> BoardDoc {
    BoardDoc {
        session: Session::with_document(figcraft_doc::Document::new("Design")),
    }
}

/// A pointer event of `kind` at page point (`x`, `y`), at 100%.
fn at(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
    PointerEvent::at(kind, x, y, 1.0)
}

/// Sends `events` to the tools, one after another, and what they asked of the view.
fn send(board: &mut BoardDoc, events: &[PointerEvent]) -> Vec<UiRequest> {
    let mut requests = Vec::new();
    for event in events {
        requests.extend(pointer(board, event).unwrap());
    }
    requests
}

/// The top-level layers of the page on show.
fn layers(board: &BoardDoc) -> Vec<NodeId> {
    let page = board.session.page().unwrap();
    board.doc().unwrap().node(page).unwrap().children.clone()
}

/// A click of the tool chosen at page point (`x`, `y`): a press and its release.
fn click(board: &mut BoardDoc, x: f64, y: f64) {
    send(
        board,
        &[at(PointerKind::Down, x, y), at(PointerKind::Up, x, y)],
    );
}

/// A new 100 × 80 rectangle at page point (0, 0), and only it selected.
fn rectangle(board: &mut BoardDoc) -> NodeId {
    let made = board
        .execute(
            "shape.rectangle",
            &json!({"x": 0, "y": 0, "width": 100, "height": 80}),
        )
        .unwrap();
    let rect = figcraft_engine::params::id_of(&made["id"]).unwrap();
    select(board, rect);
    rect
}
