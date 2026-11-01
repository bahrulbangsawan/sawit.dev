//! The tests of the menus, and the boards and the walks over menu nodes they share.

use figcraft_doc::NodeId;
use serde_json::json;

use super::*;
use crate::BoardDoc;

mod context_menu;
mod enabled;
mod main_menu;
mod shortcuts;

/// A new, empty board: nothing selected.
fn board() -> BoardDoc {
    BoardDoc::new_board("Canvas").unwrap()
}

/// Makes a sticky at `x` on `board`, which selects it, as `board.sticky` selects what it makes.
fn sticky(board: &mut BoardDoc, x: i32) -> NodeId {
    let made = board
        .execute("board.sticky", &json!({"x": x, "y": 0, "text": "hi"}))
        .unwrap();
    NodeId(made["id"].as_u64().unwrap())
}

/// A new board with one sticky on it, selected.
fn board_with_sticky() -> BoardDoc {
    let mut board = board();
    sticky(&mut board, 0);
    board
}

/// Every item in `nodes`, however deep, as (id, label), in menu order.
fn items(nodes: &[MenuNode]) -> Vec<(&'static str, &str)> {
    let mut found = Vec::new();
    for node in nodes {
        match node {
            MenuNode::Item { id, label, .. } => found.push((*id, label.as_str())),
            MenuNode::Submenu { children, .. } => found.extend(items(children)),
            MenuNode::Separator => {}
        }
    }
    found
}

/// The ids of `items(nodes)`.
fn ids(nodes: &[MenuNode]) -> Vec<&'static str> {
    items(nodes).into_iter().map(|(id, _)| id).collect()
}

/// The children of the submenu called `label` among `nodes`.
fn children_of<'a>(nodes: &'a [MenuNode], label: &str) -> &'a [MenuNode] {
    nodes
        .iter()
        .find_map(|node| match node {
            MenuNode::Submenu {
                label: name,
                children,
            } if name == label => Some(children.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no submenu called {label} in {nodes:?}"))
}

/// The labels of the submenus directly in `nodes`.
fn submenu_labels(nodes: &[MenuNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter_map(|node| match node {
            MenuNode::Submenu { label, .. } => Some(label.as_str()),
            _ => None,
        })
        .collect()
}

/// The item for `id`, wherever it sits in `nodes`.
fn item<'a>(nodes: &'a [MenuNode], id: &str) -> &'a MenuNode {
    fn find<'a>(nodes: &'a [MenuNode], wanted: &str) -> Option<&'a MenuNode> {
        nodes.iter().find_map(|node| match node {
            MenuNode::Item { id, .. } if *id == wanted => Some(node),
            MenuNode::Submenu { children, .. } => find(children, wanted),
            _ => None,
        })
    }
    find(nodes, id).unwrap_or_else(|| panic!("no item {id} in {nodes:?}"))
}

/// The label of the item for `id`.
fn label_of<'a>(nodes: &'a [MenuNode], id: &str) -> &'a str {
    match item(nodes, id) {
        MenuNode::Item { label, .. } => label,
        _ => unreachable!(),
    }
}

/// The shortcut of the item for `id`.
fn shortcut_of<'a>(nodes: &'a [MenuNode], id: &str) -> Option<&'a str> {
    match item(nodes, id) {
        MenuNode::Item { shortcut, .. } => shortcut.as_deref(),
        _ => unreachable!(),
    }
}

/// The nodes as a list of labels, with `-` for a separator and `Name ▸` for a submenu (its
/// children are not listed).
fn outline(nodes: &[MenuNode]) -> Vec<String> {
    nodes
        .iter()
        .map(|node| match node {
            MenuNode::Item { label, .. } => label.clone(),
            MenuNode::Submenu { label, .. } => format!("{label} ▸"),
            MenuNode::Separator => "-".into(),
        })
        .collect()
}

/// `nodes` and every list of nodes inside them, the outermost first.
fn lists(nodes: &[MenuNode]) -> Vec<&[MenuNode]> {
    let mut found = vec![nodes];
    for node in nodes {
        if let MenuNode::Submenu { children, .. } = node {
            found.extend(lists(children));
        }
    }
    found
}

fn assert_none_hidden(source: &str, ids: Vec<&str>) {
    assert!(!ids.is_empty(), "{source} lists no command at all");
    for id in ids {
        assert!(!is_hidden(id), "{source} lists the hidden command {id}");
    }
}
