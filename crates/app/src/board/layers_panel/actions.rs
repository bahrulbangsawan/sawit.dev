//! What a click in the layers panel does to the file: the selection it makes, a layer's name, eye
//! and padlock, the pages, and which rows of the layer tree are open.

use figcraft_doc::NodeId;
use gpui_kit::Context;
use serde_json::json;

use super::PageAction;
use crate::board::BoardView;

impl BoardView {
    /// Selects the layer of row `row` of the layer tree as a click on it does (`select.set`): the
    /// layer alone; with `shift`, the rows from the one clicked last (the anchor) to this one, added
    /// to the selection with `cmd` too; with `cmd`, or `shift` and no anchor in the selection, the
    /// layer added to the selection or taken out. Typing on the canvas ends first.
    pub fn select_rows(&mut self, row: usize, shift: bool, cmd: bool, cx: &mut Context<Self>) {
        let rows = self.layer_rows();
        let Some(clicked) = rows.get(row).map(|row| row.id) else {
            return;
        };
        let Some(selection) = self.board().map(|board| board.session.selection()) else {
            return;
        };
        let anchor = (self.layer_anchor)
            .filter(|anchor| selection.contains(anchor))
            .and_then(|anchor| rows.iter().position(|row| row.id == anchor));
        let ids = match anchor {
            Some(from) if shift => {
                let mut ids = if cmd { selection } else { Vec::new() };
                for row in &rows[from.min(row)..=from.max(row)] {
                    if !ids.contains(&row.id) {
                        ids.push(row.id);
                    }
                }
                ids
            }
            _ if shift || cmd => {
                self.layer_anchor = Some(clicked);
                let mut ids = selection;
                match ids.iter().position(|id| *id == clicked) {
                    Some(at) => {
                        ids.remove(at);
                    }
                    None => ids.push(clicked),
                }
                ids
            }
            _ => {
                self.layer_anchor = Some(clicked);
                vec![clicked]
            }
        };
        self.select_layers(ids, cx);
    }

    /// Names layer `id` `name` (`object.rename`), as one undo step. An empty or blank name, or the
    /// name the layer has, changes nothing and makes no undo step (Review Focus 2).
    pub fn rename_layer(&mut self, id: NodeId, name: &str, cx: &mut Context<Self>) {
        let stored = self.board().and_then(|board| {
            let doc = board.doc().ok()?;
            doc.node(id).map(|node| node.name.clone())
        });
        if name.trim().is_empty() || stored.as_deref() == Some(name) {
            cx.notify();
            return;
        }
        self.choose("object.rename", json!({"ids": [id], "name": name}), cx);
    }

    /// Hides layer `id`, or shows it when it is hidden (`object.toggleVisible`): what a click on its
    /// row's eye does.
    pub fn toggle_visible(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.choose("object.toggleVisible", json!({"ids": [id]}), cx);
    }

    /// Locks layer `id`, or unlocks it when it is locked (`object.toggleLock`): what a click on its
    /// row's padlock does.
    pub fn toggle_locked(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.choose("object.toggleLock", json!({"ids": [id]}), cx);
    }

    /// Does `action` to the file's pages, through `page.add`, `page.rename`, `page.duplicate`,
    /// `page.delete` and `page.select`. A blank name, or the name the page has, renames nothing,
    /// and opening the page that is open does nothing.
    pub fn page_action(&mut self, action: PageAction, cx: &mut Context<Self>) {
        let (command, params) = match action {
            PageAction::Add => ("page.add", json!({})),
            PageAction::Rename(id, name) => {
                let stored = self.board().and_then(|board| {
                    let doc = board.doc().ok()?;
                    doc.node(id).map(|node| node.name.clone())
                });
                if name.trim().is_empty() || stored.as_deref() == Some(name.as_str()) {
                    cx.notify();
                    return;
                }
                ("page.rename", json!({"id": id, "name": name}))
            }
            PageAction::Duplicate(id) => ("page.duplicate", json!({"id": id})),
            PageAction::Delete(id) => ("page.delete", json!({"id": id})),
            PageAction::Select(id) => {
                if self.board().and_then(|board| board.session.page().ok()) == Some(id) {
                    return;
                }
                ("page.select", json!({"id": id}))
            }
        };
        self.choose(command, params, cx);
    }

    /// Selects `ids`, as a click in the panel does. Typing on the canvas ends first, through the
    /// canvas's one helper (ruling R-TEXTEDIT), so that what it typed is one undo step and the keys
    /// act on the layers again; then layers no longer there are left out. The panel shows the
    /// selection already, so it isn't revealed.
    pub(super) fn select_layers(&mut self, ids: Vec<NodeId>, cx: &mut Context<Self>) {
        if self.end_text_edit(Some("select.set"), cx) {
            self.run(
                |board| {
                    let doc = board.doc()?;
                    let ids: Vec<NodeId> = ids.into_iter().filter(|id| doc.contains(*id)).collect();
                    board.execute("select.set", &json!({"ids": ids}))
                },
                cx,
            );
        }
        self.layers.seen_selection = self
            .board()
            .map(|board| board.session.selection())
            .unwrap_or_default();
    }

    /// Opens row `id` of the layer tree, or closes it. Not while the filter filters, which shows
    /// what it finds open whatever is open (ruling P4-C5).
    pub(super) fn toggle_expanded(&mut self, id: NodeId, cx: &mut Context<Self>) {
        if !self.layer_filter.trim().is_empty() {
            return;
        }
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        cx.notify();
    }

    /// Closes every row but those that lead to the selection: what `layers.collapseAll` (⌥L,
    /// Object ▸ Collapse layers) asks of the panel (ruling P4-C9), as FigCraft does it.
    pub(in crate::board) fn collapse_layers(&mut self) {
        let selection = (self.board())
            .map(|board| board.session.selection())
            .unwrap_or_default();
        let open = self.reveal(&selection);
        self.expanded = open;
    }
}
