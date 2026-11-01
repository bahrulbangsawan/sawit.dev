//! The tests of the layers panel's model, and the files and layer trees they share.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::Session;
use figcraft_engine::params::id_of;
use serde_json::{Value, json};

use super::*;
use crate::doc::BoardDoc;

mod drop;
mod filter;
mod icons;
mod pages;
mod reveal;
mod rows;

/// A design file holding `doc`.
fn file(doc: Document) -> BoardDoc {
    BoardDoc {
        session: Session::with_document(doc),
    }
}

/// A 50 × 50 layer of `kind` called `name`, on top of `parent`'s layers.
fn add(doc: &mut Document, parent: NodeId, kind: NodeKind, name: &str) -> NodeId {
    doc.create(parent, kind, name, 0.0, 0.0, 50.0, 50.0)
        .unwrap()
}

fn section() -> NodeKind {
    NodeKind::Section {
        contents_hidden: false,
    }
}

fn component() -> NodeKind {
    NodeKind::Component {
        description: String::new(),
        props: BTreeMap::new(),
    }
}

/// The design doc's tree on a design file's first page: frame F holds A, B and the group G,
/// which holds C. G goes in first, so F's layers run G (the bottom), A, B (the top) and the
/// panel lists B, A, G.
struct Tree {
    board: BoardDoc,
    page: NodeId,
    f: NodeId,
    a: NodeId,
    b: NodeId,
    g: NodeId,
    c: NodeId,
}

fn tree() -> Tree {
    let mut doc = Document::new("Design");
    let page = doc.first_page().unwrap();
    let f = add(&mut doc, page, NodeKind::Frame, "F");
    let g = add(&mut doc, f, NodeKind::Group, "G");
    let c = add(&mut doc, g, NodeKind::Rectangle, "C");
    let a = add(&mut doc, f, NodeKind::Rectangle, "A");
    let b = add(&mut doc, f, NodeKind::Rectangle, "B");
    Tree {
        board: file(doc),
        page,
        f,
        a,
        b,
        g,
        c,
    }
}

impl Tree {
    fn doc(&self) -> Arc<Document> {
        self.board.doc().unwrap()
    }

    /// The rows with `expanded` open and `selection` selected.
    fn rows(&self, expanded: &[NodeId], selection: &[NodeId], filter: &str) -> Vec<LayerRow> {
        let expanded: HashSet<NodeId> = expanded.iter().copied().collect();
        rows(&self.doc(), self.page, &expanded, selection, filter)
    }

    /// Every branch open.
    fn all_rows(&self) -> Vec<LayerRow> {
        self.rows(&[self.f, self.g], &[], "")
    }

    fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.doc().node(id).unwrap().children.clone()
    }

    /// `drop_target` for what `rows` lists, dragging `dragged` over the row of `over`.
    fn target(
        &self,
        rows: &[LayerRow],
        dragged: &[NodeId],
        over: NodeId,
        pos: DropPos,
    ) -> Option<Reparent> {
        drop_on(&self.doc(), rows, dragged, over, pos)
    }

    /// Runs the move as the panel will.
    fn apply(&mut self, reparent: &Reparent) {
        apply(&mut self.board, reparent);
    }
}

fn drop_on(
    doc: &Document,
    rows: &[LayerRow],
    dragged: &[NodeId],
    over: NodeId,
    pos: DropPos,
) -> Option<Reparent> {
    let at = rows.iter().position(|row| row.id == over).unwrap();
    drop_target(doc, rows, dragged, at, pos)
}

fn apply(board: &mut BoardDoc, reparent: &Reparent) {
    let params = json!({
        "ids": reparent.ids,
        "parent": reparent.parent,
        "index": reparent.index,
    });
    board.execute("node.reparent", &params).unwrap();
}

fn ids(rows: &[LayerRow]) -> Vec<NodeId> {
    rows.iter().map(|row| row.id).collect()
}

fn outline(rows: &[LayerRow]) -> Vec<(NodeId, usize)> {
    rows.iter().map(|row| (row.id, row.depth)).collect()
}

fn set(ids: &[NodeId]) -> HashSet<NodeId> {
    ids.iter().copied().collect()
}

fn row(rows: &[LayerRow], id: NodeId) -> &LayerRow {
    rows.iter().find(|row| row.id == id).unwrap()
}

/// Runs `command` on `board` and returns the id it made.
fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
    let made = board.execute(command, &params).unwrap();
    id_of(&made["id"]).unwrap()
}
