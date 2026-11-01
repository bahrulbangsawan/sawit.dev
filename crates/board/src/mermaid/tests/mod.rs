//! The converter's tests: what each type of diagram becomes, and what the `mermaid` op draws.

mod edges;
mod kinds;
mod nodes;
mod op;
mod svg;

use figcraft_doc::NodeId;
use sawit_core::mermaid::render;
use serde_json::{Value, json};

use super::*;
use crate::skeleton::End;
use crate::{BoardDoc, ops};

/// The size Pele sets a flowchart's labels in, which they come onto the board in.
const PELE_SIZE: f64 = 16.0;

const FLOW: &str = include_str!("../../../tests/fixtures/mermaid/flow.mmd");
const SEQUENCE: &str = include_str!("../../../tests/fixtures/mermaid/sequence.mmd");
const PIE: &str = include_str!("../../../tests/fixtures/mermaid/pie.mmd");

fn board() -> BoardDoc {
    BoardDoc::new_board("Canvas").unwrap()
}

/// The skeletons of `source`, rendered as the op renders it.
fn skeletons(source: &str) -> Vec<Skeleton> {
    let diagram = render(source, &Palette::default()).unwrap();
    to_skeletons(&diagram).unwrap()
}

fn of_kind<'a>(items: &'a [Skeleton], kind: &str) -> Vec<&'a Skeleton> {
    items.iter().filter(|item| item.kind == kind).collect()
}

fn label(item: &Skeleton) -> Option<&str> {
    item.label.as_ref().map(|label| label.text.as_str())
}

fn point_count(item: &Skeleton) -> usize {
    item.points.as_ref().map_or(0, Vec::len)
}

/// The label of the node skeleton that `end` joins.
fn joins<'a>(items: &'a [Skeleton], end: &Option<End>) -> &'a str {
    let id = end.as_ref().and_then(|end| end.id.as_deref());
    let id = id.expect("an end that joins a node");
    let node = items.iter().find(|item| item.id.as_deref() == Some(id));
    label(node.expect("the id of a node")).expect("a node with a label")
}

/// Runs the mermaid op on `board` and checks that it answered with what it added.
fn draw(board: &mut BoardDoc, op: Value) -> Vec<NodeId> {
    let reply = ops::run(board, &op);
    assert_eq!(reply["ok"], json!(true), "{op}: {reply}");
    let ids: Vec<NodeId> = reply["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|id| id.as_str().expect("an id string").parse().unwrap())
        .collect();
    assert_eq!(reply["added"], json!(ids.len()), "{reply}");
    assert!(!ids.is_empty(), "{reply}");
    ids
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// The skeletons of a diagram whose SVG is `svg` (a flowchart, as far as the converter knows).
fn converted(svg: &str) -> Vec<Skeleton> {
    let diagram = Diagram {
        svg: svg.into(),
        width: 100.0,
        height: 100.0,
        kind: "flowchart".into(),
    };
    to_skeletons(&diagram).unwrap()
}
