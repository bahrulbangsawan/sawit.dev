//! The layers panel (spec "Layers panel", D11): a column [`WIDTH`] wide on the canvas's left that
//! lists the file's pages and the open page's layers, and changes them. `sawit_board::layers`
//! makes its rows and works out where a dragged layer lands; this draws them, and runs what a
//! click, a name typed or a drop asks for.
//!
//! - **Pages.** The page list comes first, under a header that folds it away and adds a page. A
//!   click opens a page, a double-click renames it, and its context menu renames, duplicates or
//!   deletes it ([`PageAction`]). A divider (an empty page named with a leading hyphen) is a line.
//! - **Filter.** Under the pages, a field filters the layers by name. What it finds is listed under
//!   the layers that hold it, open, without changing which layers are open (ruling P4-C5): their
//!   arrows do nothing while it filters, and clearing it gives the tree back as it was. A filter
//!   that finds nothing says so.
//! - **Layer tree.** Then the open page's layers, top of the stacking order first, [`ROW`] high and
//!   indented [`INDENT`] for each layer that holds them, in a list that draws only the rows in view.
//!   A row has its arrow, its kind's icon and its name, and its padlock and eye while it is
//!   hovered, or while the layer is locked or hidden.
//!   - A click selects the row's layer, ⇧ the rows from the last one clicked, ⌘ adds it or takes it
//!     out ([`BoardView::select_rows`]). The canvas keeps the keyboard, so its keys act on what the
//!     panel selected, and typing on the canvas ends.
//!   - A double-click types a new name in the row: Enter, or the keyboard going elsewhere, keeps
//!     it, Escape drops it, and an empty name keeps the old one ([`BoardView::rename_layer`]).
//!   - A drag moves the layers to where a line between rows, or a frame round a row, shows: next to
//!     a layer or into it, as one undo step ([`BoardView::drop_rows`]). A drag of a selected row
//!     moves the whole selection. Where a drop would change nothing, or the layer can't go, no mark
//!     shows and a drop does nothing.
//!   - Hovering a row outlines its layer on the canvas (`ds.hover`), and leaving the rows ends it
//!     (ruling P4-C12).
//! - **Following the canvas.** A selection made anywhere but in the panel opens the layers that hold
//!   it, once, so that a row closed by hand stays closed, and the list scrolls to it. `layers.rename`
//!   (⌘R, F2, Object ▸ Rename) and a double-click on a frame's name on the canvas
//!   (`canvas.renameLabel`) show the panel and start typing in the layer's row (ruling P4-C11), and
//!   `layers.collapseAll` (⌥L) closes every row but those that lead to the selection (P4-C9).
//!
//! Its rows are read behind the engine's guard: a panic in reading them leaves the panel empty and
//! is logged, with no toast (D15). Every change runs through [`BoardView::choose`], so a change the
//! engine refuses changes nothing and says why in a toast.

use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::guard;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::layers::{self, DropPos, LayerIcon, LayerRow, PageRow, Reparent};
use serde_json::{Value, json};

use super::BoardView;

/// The panel's width (spec D11).
pub(super) const WIDTH: f32 = 240.;
/// The height of a row of the page list and of the layer tree.
const ROW: f32 = 28.;
/// How far a layer's row is indented for each layer that holds it.
const INDENT: f32 = 12.;
/// The room left of a row's contents at depth 0.
const ROW_PAD: f32 = 8.;
/// The width of the box of a row's arrow.
const ARROW: f32 = 14.;
/// The height of the page list's header.
const HEADER: f32 = 32.;
/// How many pages the page list shows before it scrolls.
const PAGES_SHOWN: f32 = 6.;
/// The thickness of the mark that shows where a drop lands.
const DROP_MARK: f32 = 2.;
/// The page list's header.
const PAGES_TITLE: &str = "Pages";
/// What the filter field says while it is empty, as FigCraft's says it.
const FILTER_HINT: &str = "Find layers";
/// What the tree says on a page with no layers.
const NO_LAYERS: &str = "No layers on this page";
/// What the tree says when the filter finds nothing, as FigCraft's says it.
const NO_MATCHES: &str = "No matching layers";

/// The eye of a layer that shows: a click hides it.
const SHOWN_ICON: Lucide = Lucide::Eye;
/// The eye of a hidden layer: a click shows it.
const HIDDEN_ICON: Lucide = Lucide::EyeOff;
/// The padlock of a locked layer: a click unlocks it.
const LOCKED_ICON: Lucide = Lucide::Lock;
/// The padlock of a layer that isn't locked: a click locks it.
const UNLOCKED_ICON: Lucide = Lucide::LockOpen;
/// The arrow of an open row, and of the open page list.
const OPEN_ICON: Lucide = Lucide::ChevronDown;
/// The arrow of a closed row, and of the folded page list.
const CLOSED_ICON: Lucide = Lucide::ChevronRight;
/// The button that adds a page.
const ADD_PAGE_ICON: Lucide = Lucide::Plus;
/// The picture in the filter field.
const FILTER_ICON: Lucide = Lucide::Search;

/// Every icon the panel shows but the layers' own ([`layer_icon`]), for the test that they are
/// all embedded.
#[cfg(test)]
const ICONS: &[Lucide] = &[
    SHOWN_ICON,
    HIDDEN_ICON,
    LOCKED_ICON,
    UNLOCKED_ICON,
    OPEN_ICON,
    CLOSED_ICON,
    ADD_PAGE_ICON,
    FILTER_ICON,
];

/// What the page list does to the file's pages ([`BoardView::page_action`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PageAction {
    /// Adds a page after the others, and opens it.
    Add,
    /// Renames a page. An empty name, or the name it has, changes nothing.
    Rename(NodeId, String),
    /// Copies a page and its layers right after it, and opens the copy.
    Duplicate(NodeId),
    /// Deletes a page. The file keeps one page that isn't a divider: the engine refuses to delete
    /// the last one. When the page was open, the first one opens.
    Delete(NodeId),
    /// Opens a page.
    Select(NodeId),
}

/// A drag of layers in the layer tree.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LayerDrag {
    /// The layers dragged: the selection when the row dragged is in it, else the row's layer.
    ids: Vec<NodeId>,
    /// The row the drag is over and where on it, when a drop there moves the layers.
    over: Option<(usize, DropPos)>,
}

/// What the layers panel keeps from one frame to the next.
#[derive(Default)]
pub(super) struct LayersPanel {
    /// The rows as last made, with what they were made from.
    read: Option<Read>,
    /// The selection the panel has shown: one that differs was made elsewhere, and is revealed.
    seen_selection: Vec<NodeId>,
    /// The page the panel showed last: on another, the list starts at the top.
    seen_page: Option<NodeId>,
    /// The list's scroll.
    scroll: UniformListScrollHandle,
    /// The layer whose row the list scrolls to with the next frame.
    scroll_to: Option<NodeId>,
    /// The layer whose row the pointer is over.
    hovered: Option<NodeId>,
    /// The page list is folded away.
    pages_closed: bool,
    /// The page whose name is being typed in its row.
    renaming_page: Option<NodeId>,
    /// The field a name is typed in, made with the frame the rename starts in, which has the
    /// window.
    field: Option<RenameField>,
    /// The filter's field, made with the first frame the panel shows in.
    filter: Option<FilterField>,
}

/// The rows of the layer tree, and what they were made from.
struct Read {
    /// The document, kept so that no other one can take its address.
    doc: Arc<Document>,
    page: NodeId,
    expanded: HashSet<NodeId>,
    selection: Vec<NodeId>,
    filter: String,
    rows: Rc<Vec<LayerRow>>,
}

/// What a name is being typed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Renamed {
    Layer(NodeId),
    Page(NodeId),
}

/// The field a name is typed in, in its row, and what hears it.
struct RenameField {
    target: Renamed,
    input: Entity<InputState>,
    _events: Subscription,
}

/// The filter's field, and what hears it change.
struct FilterField {
    input: Entity<InputState>,
    _changed: Subscription,
}

/// What a drag in the layer tree carries: the canvas it started in, and the row dragged. The
/// layers it moves are the canvas's own ([`LayerDrag`]).
#[derive(Clone, Copy, Debug)]
struct DraggedRow {
    view: EntityId,
    row: NodeId,
}

/// What follows the pointer while layers are dragged: the name of the layer, or how many there are.
struct DragLabel(SharedString);

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .text_xs()
            .shadow_md()
            .child(self.0.clone())
    }
}

/// The Lucide icon that pictures a layer of `icon`'s kind: the one the toolbar's tool for it shows,
/// where it has one.
fn layer_icon(icon: LayerIcon) -> Lucide {
    match icon {
        LayerIcon::Frame => Lucide::Frame,
        LayerIcon::Section => Lucide::SquareDashed,
        LayerIcon::Group => Lucide::Group,
        LayerIcon::Component => Lucide::Component,
        LayerIcon::Instance => Lucide::Diamond,
        LayerIcon::Text => Lucide::Type,
        LayerIcon::Rectangle => Lucide::Square,
        LayerIcon::Ellipse => Lucide::Circle,
        LayerIcon::Vector => Lucide::PenTool,
        LayerIcon::Line => Lucide::Slash,
        LayerIcon::Image => Lucide::Image,
        LayerIcon::Sticky => Lucide::StickyNote,
        LayerIcon::Shape => Lucide::Shapes,
        LayerIcon::Connector => Lucide::Spline,
        LayerIcon::Ink => Lucide::PenLine,
        LayerIcon::Stamp => Lucide::Stamp,
        LayerIcon::Table => Lucide::Table,
        LayerIcon::Other => Lucide::Slice,
    }
}

/// Where a drop on a row lands, `rel` being how far down the row the pointer is (0 at its top, 1
/// at its bottom): into the row's layer in the middle half of the row when it takes layers
/// (`into`), else above it in the top half and below it in the bottom half, as in FigCraft.
fn drop_pos(rel: f32, into: bool) -> DropPos {
    if into && (0.25..0.75).contains(&rel) {
        DropPos::Into
    } else if rel < 0.5 {
        DropPos::Above
    } else {
        DropPos::Below
    }
}

/// The ids in `data.ids` of a UI request.
pub(super) fn ids_in(data: &Value) -> Vec<NodeId> {
    let ids = data.get("ids").and_then(Value::as_array);
    ids.into_iter()
        .flatten()
        .filter_map(figcraft_engine::params::id_of)
        .collect()
}

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

    /// Drops the layers being dragged ([`Self::drag`](BoardView)) on row `over` of the layer tree,
    /// at `pos`: next to the row's layer or into it, as `sawit_board::layers::drop_target` works
    /// it out, through `node.reparent`, as one undo step. Where nothing may be dropped, or a drop
    /// would leave the layers where they are, it does nothing. The drag ends.
    pub fn drop_rows(&mut self, over: usize, pos: DropPos, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        cx.notify();
        let rows = self.layer_rows();
        let Some(doc) = self.board().and_then(|board| board.doc().ok()) else {
            return;
        };
        let target = guard::run("ui.dropTarget", || {
            Ok(layers::drop_target(&doc, &rows, &drag.ids, over, pos))
        });
        let Ok(Some(Reparent { ids, parent, index })) = target else {
            return;
        };
        let params = json!({"ids": ids, "parent": parent, "index": index});
        self.choose("node.reparent", params, cx);
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

    /// The rows of the open page's layer tree, as they show: made again only when the document,
    /// the page, the open layers, the selection or the filter is another than the last time, behind
    /// the engine's guard, so a panic costs the panel its rows, not the app (D15). The layers the
    /// filter finds show open without being added to [`Self::expanded`](BoardView) (ruling P4-C5).
    fn layer_rows(&mut self) -> Rc<Vec<LayerRow>> {
        let Some(ds) = self.board().and_then(|board| board.session.ds().ok()) else {
            return Rc::default();
        };
        if let Some(read) = &self.layers.read
            && Arc::ptr_eq(&read.doc, &ds.doc)
            && read.page == ds.page
            && read.selection == ds.selection
            && read.filter == self.layer_filter
            && read.expanded == self.expanded
        {
            return read.rows.clone();
        }
        let (doc, page, selection) = (ds.doc.clone(), ds.page, ds.selection.clone());
        let made = guard::run("ui.layers", || {
            Ok(layers::rows(
                &doc,
                page,
                &self.expanded,
                &selection,
                &self.layer_filter,
            ))
        });
        let rows = Rc::new(made.unwrap_or_default());
        self.layers.read = Some(Read {
            doc,
            page,
            expanded: self.expanded.clone(),
            selection,
            filter: self.layer_filter.clone(),
            rows: rows.clone(),
        });
        rows
    }

    /// The rows of the page list, behind the engine's guard.
    fn page_rows(&self) -> Vec<PageRow> {
        let Some(ds) = self.board().and_then(|board| board.session.ds().ok()) else {
            return Vec::new();
        };
        guard::run("ui.pages", || Ok(layers::pages(&ds.doc, ds.page))).unwrap_or_default()
    }

    /// The layers to open so that every one of `ids` has a row, behind the engine's guard.
    fn reveal(&self, ids: &[NodeId]) -> HashSet<NodeId> {
        let Some(doc) = self.board().and_then(|board| board.doc().ok()) else {
            return HashSet::new();
        };
        guard::run("ui.reveal", || Ok(layers::reveal(&doc, ids))).unwrap_or_default()
    }

    /// Selects `ids`, as a click in the panel does. Typing on the canvas ends first, so that what it
    /// typed is one undo step and the keys act on the layers again; then layers no longer there
    /// are left out. The panel shows the selection already, so it isn't revealed.
    fn select_layers(&mut self, ids: Vec<NodeId>, cx: &mut Context<Self>) {
        self.run(
            |board| {
                if board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()) {
                    board.execute("text.edit.end", &json!({}))?;
                }
                let doc = board.doc()?;
                let ids: Vec<NodeId> = ids.into_iter().filter(|id| doc.contains(*id)).collect();
                board.execute("select.set", &json!({"ids": ids}))
            },
            cx,
        );
        self.layers.seen_selection = self
            .board()
            .map(|board| board.session.selection())
            .unwrap_or_default();
    }

    /// Opens row `id` of the layer tree, or closes it. Not while the filter filters, which shows
    /// what it finds open whatever is open (ruling P4-C5).
    fn toggle_expanded(&mut self, id: NodeId, cx: &mut Context<Self>) {
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
    pub(super) fn collapse_layers(&mut self) {
        let selection = (self.board())
            .map(|board| board.session.selection())
            .unwrap_or_default();
        let open = self.reveal(&selection);
        self.expanded = open;
    }

    /// Starts typing a new name in the row of the top one of `ids` that the tree lists: what
    /// `layers.rename` (⌘R, F2, Object ▸ Rename) and a double-click on a frame's name on the canvas
    /// (`canvas.renameLabel`) ask of the panel, and a double-click on a row does (ruling P4-C11).
    /// The panel shows, the layers holding `ids` open, the layer is selected alone and its row
    /// scrolled to, and its field takes the keyboard with the next frame. A name being typed in
    /// another row is kept first. With several ids only that one is renamed.
    pub(super) fn start_rename(&mut self, ids: &[NodeId], cx: &mut Context<Self>) {
        self.keep_typed_name(cx);
        self.show_layers = true;
        let open = self.reveal(ids);
        self.expanded.extend(open);
        let rows = self.layer_rows();
        let Some(id) = rows.iter().map(|row| row.id).find(|id| ids.contains(id)) else {
            cx.notify();
            return;
        };
        let selection = (self.board())
            .map(|board| board.session.selection())
            .unwrap_or_default();
        if selection != [id] {
            self.select_layers(vec![id], cx);
        }
        self.layer_anchor = Some(id);
        self.renaming = Some(id);
        self.layers.scroll_to = Some(id);
        cx.notify();
    }

    /// Starts typing a new name in page `id`'s row. A name being typed in another row is kept
    /// first.
    fn start_page_rename(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.keep_typed_name(cx);
        self.renaming = None;
        self.layers.pages_closed = false;
        self.layers.renaming_page = Some(id);
        cx.notify();
    }

    /// Ends the rename going on, if any, keeping what was typed: another one starts.
    fn keep_typed_name(&mut self, cx: &mut Context<Self>) {
        if let Some(field) = self.layers.field.take() {
            let typed = field.input.read(cx).value().to_string();
            self.finish_rename(field.target, &typed, cx);
        }
    }

    /// Ends the rename of `target`, keeping what was typed: Enter in its field, or the field losing
    /// the keyboard. The canvas takes the keyboard back from the field that had it.
    fn commit_rename(&mut self, target: Renamed, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.layers.field.take_if(|field| field.target == target) else {
            return;
        };
        let typed = field.input.read(cx).value().to_string();
        let had_keyboard = field.input.focus_handle(cx).is_focused(window);
        drop(field);
        self.finish_rename(target, &typed, cx);
        if had_keyboard {
            window.focus(&self.focus, cx);
        }
    }

    /// Ends the rename going on, dropping what was typed: Escape in its field. The canvas takes the
    /// keyboard back.
    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.layers.field.take() else {
            return;
        };
        self.end_rename_of(field.target);
        if field.input.focus_handle(cx).is_focused(window) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The rename of `target` is over, and `typed` is the name typed: the layer or the page takes
    /// it, unless it is blank or the name it has.
    fn finish_rename(&mut self, target: Renamed, typed: &str, cx: &mut Context<Self>) {
        self.end_rename_of(target);
        match target {
            Renamed::Layer(id) => self.rename_layer(id, typed, cx),
            Renamed::Page(id) => self.page_action(PageAction::Rename(id, typed.into()), cx),
        }
    }

    /// No name is being typed for `target` any more.
    fn end_rename_of(&mut self, target: Renamed) {
        match target {
            Renamed::Layer(id) if self.renaming == Some(id) => self.renaming = None,
            Renamed::Page(id) if self.layers.renaming_page == Some(id) => {
                self.layers.renaming_page = None;
            }
            Renamed::Layer(_) | Renamed::Page(_) => {}
        }
    }

    /// Before the panel is drawn: a drag ended elsewhere is over; a selection made elsewhere opens
    /// the layers holding it; the rename field is made for the rename that started, or goes with
    /// the one that ended; and the list scrolls to the row that asked for it. The tree's rows.
    fn settle_layers_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Rc<Vec<LayerRow>> {
        // GPUI ends a drag on the button's release wherever it is, dropped on a row or not.
        if self.drag.is_some() && !cx.has_active_drag() {
            self.drag = None;
        }
        let Some((page, selection)) = (self.board())
            .and_then(|board| board.session.ds().ok())
            .map(|ds| (ds.page, ds.selection.clone()))
        else {
            return Rc::default();
        };
        if self.layers.seen_page.replace(page) != Some(page) {
            self.layers.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        let revealed = selection != self.layers.seen_selection;
        if revealed {
            let open = self.reveal(&selection);
            self.expanded.extend(open);
            self.layers.seen_selection = selection;
        }
        let rows = self.layer_rows();
        self.settle_rename_field(&rows, window, cx);
        let first_selected = || rows.iter().find(|row| row.selected).map(|row| row.id);
        let scroll_to =
            (self.layers.scroll_to.take()).or_else(|| revealed.then(first_selected).flatten());
        if let Some(index) = scroll_to.and_then(|id| rows.iter().position(|row| row.id == id)) {
            self.layers
                .scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
        rows
    }

    /// Makes the field for the rename that started, with the layer's or the page's name in it,
    /// selected, and the keyboard; drops the field of a rename that ended. The rename of a layer
    /// no longer listed, or of a page no longer there, ends as it is, as in FigCraft.
    fn settle_rename_field(
        &mut self,
        rows: &[LayerRow],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.board().and_then(|board| board.doc().ok());
        let named = |id: NodeId| {
            let node = doc.as_ref()?.node(id)?;
            Some(node.name.clone())
        };
        if let Some(id) = self.renaming
            && !rows.iter().any(|row| row.id == id)
        {
            self.renaming = None;
        }
        if let Some(id) = self.layers.renaming_page
            && !doc.as_ref().is_some_and(|doc| doc.pages.contains(&id))
        {
            self.layers.renaming_page = None;
        }
        let wanted =
            (self.renaming.map(Renamed::Layer)).or(self.layers.renaming_page.map(Renamed::Page));
        if self.layers.field.as_ref().map(|field| field.target) == wanted {
            return;
        }
        if let Some(old) = self.layers.field.take()
            && old.input.focus_handle(cx).is_focused(window)
        {
            window.focus(&self.focus, cx);
        }
        let Some(target) = wanted else {
            return;
        };
        let (Renamed::Layer(id) | Renamed::Page(id)) = target;
        let Some(name) = named(id) else {
            self.end_rename_of(target);
            return;
        };
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(name, window, cx);
            input
        });
        let events = cx.subscribe_in(
            &input,
            window,
            move |view, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    view.commit_rename(target, window, cx);
                }
                InputEvent::Change | InputEvent::Focus => {}
            },
        );
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.layers.field = Some(RenameField {
            target,
            input,
            _events: events,
        });
    }

    /// The filter's field, made the first time the panel is drawn. What is typed in it is the
    /// filter; a filter set otherwise shows in it, unless it is being typed in. Escape empties it.
    fn filter_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let input = match &self.layers.filter {
            Some(filter) => filter.input.clone(),
            None => {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(FILTER_HINT)
                        .clean_on_escape()
                });
                let changed =
                    cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            let typed = input.read(cx).value().to_string();
                            if view.layer_filter != typed {
                                view.layer_filter = typed;
                                cx.notify();
                            }
                        }
                    });
                self.layers.filter = Some(FilterField {
                    input: input.clone(),
                    _changed: changed,
                });
                input
            }
        };
        let shown = input.read(cx).value();
        if shown != self.layer_filter.as_str() && !input.focus_handle(cx).is_focused(window) {
            let filter = self.layer_filter.clone();
            input.update(cx, |input, cx| input.set_value(filter, window, cx));
        }
        input
    }

    /// The pointer went onto row `id` (`hovered`) or off it: the canvas outlines its layer
    /// (`ds.hover`) while it is on it. Off the rows, the outline ends, unless the canvas has
    /// outlined another layer since (ruling P4-C12).
    fn hover_row(&mut self, id: NodeId, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.layers.hovered = Some(id);
        } else if self.layers.hovered == Some(id) {
            self.layers.hovered = None;
        } else {
            return;
        }
        let hover = self.layers.hovered;
        if let Some(ds) = self
            .board_mut()
            .and_then(|board| board.session.ds_mut().ok())
            && (hover.is_some() || ds.hover == Some(id))
        {
            ds.hover = hover;
        }
        cx.notify();
    }

    /// A click on the row of layer `id`: the canvas takes the keyboard, so that its keys act on
    /// what the panel selects; a double-click starts a rename, a click selects
    /// ([`Self::select_rows`]).
    fn click_row(
        &mut self,
        id: NodeId,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if event.click_count() >= 2 {
            self.start_rename(&[id], cx);
            return;
        }
        let Some(row) = self.layer_rows().iter().position(|row| row.id == id) else {
            return;
        };
        let modifiers = event.modifiers();
        self.select_rows(row, modifiers.shift, modifiers.platform, cx);
    }

    /// A click on page `id`'s row: a double-click starts a rename, a click opens the page, unless
    /// it is a `divider`. The canvas takes the keyboard.
    fn click_page(
        &mut self,
        id: NodeId,
        divider: bool,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if event.click_count() >= 2 {
            self.start_page_rename(id, cx);
        } else if !divider {
            self.page_action(PageAction::Select(id), cx);
        }
    }

    /// A drag starts from the row of layer `row`: it carries the selection when the row is in it,
    /// else the row's layer, as FigCraft's panel drags. What follows the pointer.
    fn begin_drag(&mut self, row: NodeId, cx: &mut Context<Self>) -> SharedString {
        let selection = (self.board())
            .map(|board| board.session.selection())
            .unwrap_or_default();
        let ids = if selection.contains(&row) {
            selection
        } else {
            vec![row]
        };
        let label = match ids.len() {
            1 => (self.layer_rows().iter())
                .find(|shown| shown.id == row)
                .map_or_else(|| "1 layer".into(), |shown| shown.name.clone().into()),
            n => format!("{n} layers").into(),
        };
        self.drag = Some(LayerDrag { ids, over: None });
        cx.notify();
        label
    }

    /// The drag moved, over row `row` or elsewhere (`event.bounds` are the row's): over the row, the
    /// mark shows where a drop there lands, if it moves the layers; off it, the row's mark goes.
    fn drag_over(&mut self, row: usize, event: &DragMoveEvent<DraggedRow>, cx: &mut Context<Self>) {
        let Some(ids) = self.drag.as_ref().map(|drag| drag.ids.clone()) else {
            return;
        };
        let at = event.event.position;
        let bounds = event.bounds;
        let over = if bounds.contains(&at) {
            let rel = f32::from(at.y - bounds.origin.y) / f32::from(bounds.size.height);
            let rows = self.layer_rows();
            let doc = self.board().and_then(|board| board.doc().ok());
            let landing = doc.and_then(|doc| {
                guard::run("ui.dropTarget", || {
                    let target = |pos| layers::drop_target(&doc, &rows, &ids, row, pos);
                    let pos = drop_pos(rel, target(DropPos::Into).is_some());
                    Ok(target(pos).map(|_| pos))
                })
                .ok()
                .flatten()
            });
            landing.map(|pos| (row, pos))
        } else if self
            .drag
            .as_ref()
            .and_then(|drag| drag.over)
            .map(|(at, _)| at)
            == Some(row)
        {
            None
        } else {
            return;
        };
        if let Some(drag) = &mut self.drag
            && drag.over != over
        {
            drag.over = over;
            cx.notify();
        }
    }

    /// The drag was dropped on a row: the layers go where its mark showed, if one did. A drag from
    /// another canvas's panel drops nothing here.
    fn drop_dragged(&mut self, dragged: &DraggedRow, cx: &mut Context<Self>) {
        if dragged.view != cx.entity_id() {
            return;
        }
        match self.drag.as_ref().and_then(|drag| drag.over) {
            Some((row, pos)) => self.drop_rows(row, pos, cx),
            None => {
                self.drag = None;
                cx.notify();
            }
        }
    }

    /// The rows of the layer tree in `range`, as the list draws them.
    fn render_layer_rows(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = self.layer_rows();
        range
            .filter_map(|index| Some(self.render_layer_row(index, rows.get(index)?, cx)))
            .collect()
    }

    /// Row `index` of the layer tree, `row`.
    fn render_layer_row(&self, index: usize, row: &LayerRow, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let theme = cx.theme();
        let (muted, selected_bg, hover_bg, mark) = (
            theme.muted_foreground,
            theme.list_active,
            theme.list_hover,
            theme.drag_border,
        );
        let hovered = self.layers.hovered == Some(id);
        let outlined = (self.board())
            .and_then(|board| board.session.ds().ok())
            .is_some_and(|ds| ds.hover == Some(id));
        let left = ROW_PAD + row.depth as f32 * INDENT;
        let filtering = !self.layer_filter.trim().is_empty();
        let arrow = div()
            .id(("layers-arrow", id.0))
            .debug_selector(move || format!("layers-arrow-{}", id.0))
            .flex_none()
            .w(px(ARROW))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .when(row.has_children, |arrow| {
                let icon = if row.expanded { OPEN_ICON } else { CLOSED_ICON };
                arrow
                    .child(Icon::new(icon).xsmall().text_color(muted))
                    // Under a filter the arrows only show what it found inside (ruling P4-C5).
                    .when(!filtering, |arrow| {
                        arrow.on_click(cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.toggle_expanded(id, cx);
                        }))
                    })
            });
        let field = (self.layers.field.as_ref())
            .filter(|field| field.target == Renamed::Layer(id))
            .map(|field| field.input.clone());
        let name = match field {
            Some(input) => rename_field(&input, cx),
            None => {
                // Frames and sections on the page stand out, as in FigCraft.
                let top =
                    row.depth == 0 && matches!(row.icon, LayerIcon::Frame | LayerIcon::Section);
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(top, |name| name.font_medium())
                    .when(!row.visible, |name| name.text_color(muted))
                    .child(row.name.clone())
                    .into_any_element()
            }
        };
        let toggles = h_flex()
            .flex_none()
            .child(if hovered || row.locked {
                let icon = if row.locked {
                    LOCKED_ICON
                } else {
                    UNLOCKED_ICON
                };
                let tip = if row.locked { "Unlock" } else { "Lock" };
                Button::new(("layers-lock", id.0))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .tooltip(tip)
                    .debug_selector(move || format!("layers-lock-{}", id.0))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.toggle_locked(id, cx);
                    }))
                    .into_any_element()
            } else {
                div().size_5().into_any_element()
            })
            .child(if hovered || !row.visible {
                let icon = if row.visible { SHOWN_ICON } else { HIDDEN_ICON };
                let tip = if row.visible { "Hide" } else { "Show" };
                Button::new(("layers-eye", id.0))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .tooltip(tip)
                    .debug_selector(move || format!("layers-eye-{}", id.0))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.toggle_visible(id, cx);
                    }))
                    .into_any_element()
            } else {
                div().size_5().into_any_element()
            });
        let landing = (self.drag.as_ref())
            .and_then(|drag| drag.over)
            .filter(|(over, _)| *over == index)
            .map(|(_, pos)| {
                let line = div().absolute().left(px(left)).right_0().h(px(DROP_MARK));
                match pos {
                    DropPos::Above => line.top_0().bg(mark),
                    DropPos::Below => line.bottom_0().bg(mark),
                    DropPos::Into => div()
                        .absolute()
                        .inset_0()
                        .border(px(DROP_MARK))
                        .border_color(mark)
                        .rounded_sm(),
                }
            });
        let this = cx.entity().downgrade();
        h_flex()
            .id(("layers-row", id.0))
            .debug_selector(move || format!("layers-row-{}", id.0))
            .relative()
            .w_full()
            .h(px(ROW))
            .pl(px(left))
            .pr_1()
            .gap_1()
            .when(row.selected, |row| row.bg(selected_bg))
            .when(!row.selected && (hovered || outlined), |row| {
                row.bg(hover_bg)
            })
            .child(arrow)
            .child(Icon::new(layer_icon(row.icon)).xsmall().text_color(muted))
            .child(name)
            .child(toggles)
            .children(landing)
            .on_hover(cx.listener(move |view, hovered: &bool, _, cx| {
                view.hover_row(id, *hovered, cx);
            }))
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                view.click_row(id, event, window, cx);
            }))
            .on_drag(
                DraggedRow {
                    view: cx.entity_id(),
                    row: id,
                },
                move |dragged: &DraggedRow, _, _, cx| {
                    let label = this
                        .update(cx, |view, cx| view.begin_drag(dragged.row, cx))
                        .unwrap_or_default();
                    cx.new(|_| DragLabel(label))
                },
            )
            .on_drag_move(
                cx.listener(move |view, event: &DragMoveEvent<DraggedRow>, _, cx| {
                    view.drag_over(index, event, cx);
                }),
            )
            .on_drop(cx.listener(move |view, dragged: &DraggedRow, _, cx| {
                view.drop_dragged(dragged, cx);
            }))
            .into_any_element()
    }

    /// The page list's header: its title, which folds the list away or opens it, and the button
    /// that adds a page.
    fn render_pages_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let closed = self.layers.pages_closed;
        h_flex()
            .flex_shrink_0()
            .w_full()
            .h(px(HEADER))
            .px_2()
            .gap_1()
            .child(
                h_flex()
                    .id("layers-pages-toggle")
                    .debug_selector(|| "layers-pages-toggle".into())
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_1()
                    .cursor_pointer()
                    .child(
                        Icon::new(if closed { CLOSED_ICON } else { OPEN_ICON })
                            .xsmall()
                            .text_color(muted),
                    )
                    .child(div().text_xs().font_medium().child(PAGES_TITLE))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.layers.pages_closed = !view.layers.pages_closed;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("layers-add-page")
                    .ghost()
                    .xsmall()
                    .icon(ADD_PAGE_ICON)
                    .tooltip("Add page")
                    .debug_selector(|| "layers-add-page".into())
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.page_action(PageAction::Add, cx);
                    })),
            )
            .into_any_element()
    }

    /// The page list, which scrolls past [`PAGES_SHOWN`] pages.
    fn render_pages(&self, pages: &[PageRow], cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .id("layers-pages")
            .debug_selector(|| "layers-pages".into())
            .flex_shrink_0()
            .w_full()
            .max_h(px(PAGES_SHOWN * ROW))
            .overflow_y_scroll()
            .children(pages.iter().map(|page| self.render_page(page, cx)))
            .into_any_element()
    }

    /// The row of `page`: its name, a line for a divider, or the field its name is typed in. Its
    /// context menu renames, duplicates and deletes it.
    fn render_page(&self, page: &PageRow, cx: &mut Context<Self>) -> AnyElement {
        let (id, divider) = (page.id, page.divider);
        let theme = cx.theme();
        let (selected_bg, hover_bg, border) = (theme.list_active, theme.list_hover, theme.border);
        let field = (self.layers.field.as_ref())
            .filter(|field| field.target == Renamed::Page(id))
            .map(|field| field.input.clone());
        let content = match field {
            Some(input) => rename_field(&input, cx),
            None if divider => div().flex_1().h(px(1.)).bg(border).into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(page.name.clone())
                .into_any_element(),
        };
        let this = cx.entity().downgrade();
        h_flex()
            .id(("layers-page", id.0))
            .debug_selector(move || format!("layers-page-{}", id.0))
            .flex_shrink_0()
            .w_full()
            .h(px(ROW))
            .px_3()
            .gap_2()
            .when(page.current, |row| row.bg(selected_bg).font_medium())
            .when(!page.current && !divider, |row| {
                row.hover(move |row| row.bg(hover_bg))
            })
            .child(content)
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                view.click_page(id, divider, event, window, cx);
            }))
            .context_menu(move |menu, _, _| page_menu(menu, this.clone(), id))
            .into_any_element()
    }

    /// The layer tree: the rows in view of the list, or a line that says there are none.
    fn render_tree(&self, rows: &[LayerRow], cx: &mut Context<Self>) -> AnyElement {
        if rows.is_empty() {
            let hint = if self.layer_filter.trim().is_empty() {
                NO_LAYERS
            } else {
                NO_MATCHES
            };
            return div()
                .debug_selector(|| "layers-empty".into())
                .px_3()
                .py_2()
                .text_color(cx.theme().muted_foreground)
                .child(hint)
                .into_any_element();
        }
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                uniform_list(
                    "layers-rows",
                    rows.len(),
                    cx.processor(|view, range: Range<usize>, window, cx| {
                        view.render_layer_rows(range, window, cx)
                    }),
                )
                .track_scroll(&self.layers.scroll)
                .size_full(),
            )
            .into_any_element()
    }
}

/// The field a name is typed in, in place of a row's name. Escape in it drops what was typed;
/// a press in it stays its own, and doesn't select the row or start a drag.
fn rename_field(input: &Entity<InputState>, cx: &mut Context<BoardView>) -> AnyElement {
    div()
        .debug_selector(|| "layers-rename".into())
        .flex_1()
        .min_w_0()
        .on_action(cx.listener(|view, _: &Escape, window, cx| view.cancel_rename(window, cx)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(Input::new(input).xsmall())
        .into_any_element()
}

/// The context menu of page `id`'s row: rename, duplicate and delete it.
fn page_menu(menu: PopupMenu, view: WeakEntity<BoardView>, id: NodeId) -> PopupMenu {
    let act = |action: fn(&mut BoardView, NodeId, &mut Context<BoardView>)| {
        let view = view.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| action(view, id, cx));
        }
    };
    menu.item(
        PopupMenuItem::new("Rename page").on_click(act(|view, id, cx| {
            view.start_page_rename(id, cx);
        })),
    )
    .item(
        PopupMenuItem::new("Duplicate page").on_click(act(|view, id, cx| {
            view.page_action(PageAction::Duplicate(id), cx);
        })),
    )
    .item(
        PopupMenuItem::new("Delete page").on_click(act(|view, id, cx| {
            view.page_action(PageAction::Delete(id), cx);
        })),
    )
}

/// The layers panel of `view`'s canvas: the page list under its header, the filter, then the layer
/// tree.
pub(super) fn render_layers_panel(
    view: &mut BoardView,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) -> impl IntoElement + use<> {
    let rows = view.settle_layers_panel(window, cx);
    let pages = (!view.layers.pages_closed).then(|| view.page_rows());
    let filter = view.filter_input(window, cx);
    let header = view.render_pages_header(cx);
    let pages = pages.map(|pages| view.render_pages(&pages, cx));
    let tree = view.render_tree(&rows, cx);
    let theme = cx.theme();
    v_flex()
        .id("board-layers-panel")
        .debug_selector(|| "board-layers-panel".into())
        .flex_shrink_0()
        .w(px(WIDTH))
        .h_full()
        .overflow_hidden()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_sm()
        .child(header)
        .children(pages)
        .child(
            div()
                .flex_shrink_0()
                .w_full()
                .px_2()
                .py_1p5()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div().debug_selector(|| "layers-filter".into()).child(
                        Input::new(&filter).small().cleanable(true).prefix(
                            Icon::new(FILTER_ICON)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        ),
                    ),
                ),
        )
        .child(tree)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;

    use figcraft_doc::{Document, NodeId, NodeKind};
    use gpui_kit::{
        AppContext as _, AssetSource as _, Entity, Modifiers, MouseButton, MouseDownEvent,
        MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, point, px, size,
    };
    use sawit_board::BoardDoc;
    use sawit_board::layers::{DropPos, LayerIcon};
    use serde_json::{Value, json};

    use super::super::tests::{Scratch, click, focus_canvas, run_op};
    use super::super::{BoardView, design_panel, top_bar};
    use super::{ICONS, LayerDrag, NO_LAYERS, NO_MATCHES, PageAction, WIDTH, drop_pos, layer_icon};

    /// Plan 4's design file: frame F holds the group G, which holds C, then A and B on top of it.
    /// F's layers run G (the bottom), A, B (the top), so the panel lists F, then B, A and G under
    /// it, and C under G.
    struct Tree {
        f: NodeId,
        g: NodeId,
        c: NodeId,
        a: NodeId,
        b: NodeId,
    }

    /// A 50 × 50 layer of `kind` called `name`, on top of `parent`'s layers.
    fn add(doc: &mut Document, parent: NodeId, kind: NodeKind, name: &str) -> NodeId {
        doc.create(parent, kind, name, 0.0, 0.0, 50.0, 50.0)
            .unwrap()
    }

    /// Writes `doc` at `path` as a canvas file.
    fn write(path: &Path, doc: Document) {
        let board = BoardDoc {
            session: figcraft_engine::Session::with_document(doc),
        };
        std::fs::write(path, board.to_bytes().unwrap()).unwrap();
    }

    /// Writes Plan 4's design file at `path`, and returns its layers.
    fn design_file(path: &Path) -> Tree {
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let f = add(&mut doc, page, NodeKind::Frame, "F");
        let g = add(&mut doc, f, NodeKind::Group, "G");
        let c = add(&mut doc, g, NodeKind::Rectangle, "C");
        let a = add(&mut doc, f, NodeKind::Rectangle, "A");
        let b = add(&mut doc, f, NodeKind::Rectangle, "B");
        write(path, doc);
        Tree { f, g, c, a, b }
    }

    /// The canvas of the file at `path`, opened as the workspace opens it, with no window.
    fn open(cx: &mut TestAppContext, path: &Path) -> Entity<BoardView> {
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        cx.new(|cx| BoardView::open(path.to_path_buf(), cx))
    }

    /// The canvas of the file at `path` in a 1000 × 700 window of its own, which has the keyboard,
    /// as the one the user works in does, drawn: wide enough for a design file's two panels and a
    /// canvas between them.
    fn open_in_window(
        cx: &mut TestAppContext,
        path: &Path,
    ) -> (Entity<BoardView>, VisualTestContext) {
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        let path = path.to_path_buf();
        let window = cx.open_window(size(px(1000.), px(700.)), |_, cx| BoardView::open(path, cx));
        let view = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        (view, cx)
    }

    /// Changes `view` with `f`, and draws the window again.
    fn change(
        view: &Entity<BoardView>,
        cx: &mut VisualTestContext,
        f: impl FnOnce(&mut BoardView, &mut gpui_kit::Context<BoardView>),
    ) {
        view.update(cx, |view, cx| {
            f(view, cx);
            cx.notify();
        });
        cx.run_until_parked();
    }

    /// A debug selector made at run time: tests only, so leaking it is fine.
    fn selector(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    /// The debug selector of the row of layer `id`.
    fn row(id: NodeId) -> &'static str {
        selector(format!("layers-row-{}", id.0))
    }

    /// Whether the window drew something with debug selector `selector`.
    fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        cx.debug_bounds(selector).is_some()
    }

    /// The middle of what the window drew with debug selector `selector`.
    fn middle(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
        let drawn = cx.debug_bounds(selector);
        drawn
            .unwrap_or_else(|| panic!("{selector} isn't drawn"))
            .center()
    }

    /// Presses and releases the left button at `at` twice: a double-click.
    fn double_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
        let (button, modifiers) = (MouseButton::Left, Modifiers::default());
        for click_count in [1, 2] {
            cx.simulate_event(MouseDownEvent {
                position: at,
                button,
                modifiers,
                click_count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position: at,
                button,
                modifiers,
                click_count,
            });
        }
        cx.run_until_parked();
    }

    fn selection(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<NodeId> {
        view.read_with(cx, |view, _| view.board().unwrap().session.selection())
    }

    /// How many undo steps the canvas's file has.
    fn undo_steps(view: &Entity<BoardView>, cx: &TestAppContext) -> usize {
        view.read_with(cx, |view, _| view.board().unwrap().session.undo_depth().0)
    }

    fn name_of(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> String {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            doc.node(id).unwrap().name.clone()
        })
    }

    /// Layer `id`'s eye and padlock: whether it is shown, and whether it is locked.
    fn shown_and_locked(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> (bool, bool) {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            let node = doc.node(id).unwrap();
            (node.visible, node.locked)
        })
    }

    /// The layers of `id`, bottom first.
    fn children(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> Vec<NodeId> {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            doc.node(id).unwrap().children.clone()
        })
    }

    /// The file's pages, and the one open.
    fn pages(view: &Entity<BoardView>, cx: &TestAppContext) -> (Vec<NodeId>, NodeId) {
        view.read_with(cx, |view, _| {
            let session = &view.board().unwrap().session;
            (
                session.doc().unwrap().pages.clone(),
                session.page().unwrap(),
            )
        })
    }

    fn toast(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
        view.read_with(cx, |view, _| {
            view.toast.as_ref().map(|toast| toast.to_string())
        })
    }

    /// What `ds.hover` holds: the layer the canvas outlines as hovered.
    fn hovered(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<NodeId> {
        view.read_with(cx, |view, _| {
            view.board().unwrap().session.ds().unwrap().hover
        })
    }

    fn tool(view: &Entity<BoardView>, cx: &TestAppContext) -> &'static str {
        view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
    }

    fn command(command: &str) -> Value {
        json!({"op": "command", "command": command})
    }

    #[gpui_kit::test]
    fn click_selects_and_shift_selects_range(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-range");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let view = open(cx, &path);
        // F open: the rows are F, B, A and G.
        view.update(cx, |view, cx| {
            view.expanded.insert(t.f);
            view.select_rows(0, false, false, cx);
        });
        assert_eq!(selection(&view, cx), [t.f]);
        // ⇧: the rows from the one clicked last to this one.
        view.update(cx, |view, cx| view.select_rows(2, true, false, cx));
        assert_eq!(selection(&view, cx), [t.f, t.b, t.a]);
        // Up the list as well, from a row clicked alone.
        view.update(cx, |view, cx| {
            view.select_rows(3, false, false, cx);
            view.select_rows(1, true, false, cx);
        });
        let picked: HashSet<NodeId> = selection(&view, cx).into_iter().collect();
        assert_eq!(picked, HashSet::from([t.b, t.a, t.g]));
        assert_eq!(toast(&view, cx), None);
    }

    #[gpui_kit::test]
    fn cmd_click_toggles(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-toggle");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let view = open(cx, &path);
        view.update(cx, |view, cx| {
            view.expanded.insert(t.f);
            view.select_rows(1, false, false, cx);
            view.select_rows(3, false, true, cx);
        });
        assert_eq!(selection(&view, cx), [t.b, t.g]);
        view.update(cx, |view, cx| view.select_rows(1, false, true, cx));
        assert_eq!(selection(&view, cx), [t.g]);
        // ⇧ with no row clicked before in the selection toggles too.
        view.update(cx, |view, cx| {
            view.layer_anchor = None;
            view.select_rows(2, true, false, cx);
        });
        assert_eq!(selection(&view, cx), [t.g, t.a]);
    }

    /// Review Focus 2, and the rename ruling of P4.T1's review: a blank name, or the name the
    /// layer has, changes nothing and makes no undo step.
    #[gpui_kit::test]
    fn blank_rename_keeps_name(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-blank-rename");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let view = open(cx, &path);
        let steps = undo_steps(&view, cx);
        for blank in ["  ", "", "\t"] {
            view.update(cx, |view, cx| view.rename_layer(t.a, blank, cx));
            assert_eq!(name_of(&view, t.a, cx), "A");
        }
        view.update(cx, |view, cx| view.rename_layer(t.a, "A", cx));
        assert_eq!(undo_steps(&view, cx), steps);
        // A name typed is kept, as one undo step.
        view.update(cx, |view, cx| view.rename_layer(t.a, "Avatar", cx));
        assert_eq!(name_of(&view, t.a, cx), "Avatar");
        assert_eq!(undo_steps(&view, cx), steps + 1);
        assert_eq!(toast(&view, cx), None);
    }

    #[gpui_kit::test]
    fn eye_and_lock_toggle(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-eye-lock");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let view = open(cx, &path);
        let steps = undo_steps(&view, cx);
        view.update(cx, |view, cx| view.toggle_visible(t.a, cx));
        assert_eq!(shown_and_locked(&view, t.a, cx), (false, false));
        view.update(cx, |view, cx| view.toggle_visible(t.a, cx));
        assert_eq!(shown_and_locked(&view, t.a, cx), (true, false));
        view.update(cx, |view, cx| view.toggle_locked(t.b, cx));
        assert_eq!(shown_and_locked(&view, t.b, cx), (true, true));
        view.update(cx, |view, cx| view.toggle_locked(t.b, cx));
        assert_eq!(shown_and_locked(&view, t.b, cx), (true, false));
        // Each one undo step, on the row's layer alone.
        assert_eq!(undo_steps(&view, cx), steps + 4);
        assert_eq!(shown_and_locked(&view, t.f, cx), (true, false));
    }

    /// Review Focus 4: deleting the open page opens another; the only page can't be deleted, and
    /// FigCraft's refusal is a toast.
    #[gpui_kit::test]
    fn deleting_current_page_selects_another(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-delete-page");
        let path = tmp.0.join("design.figcraft");
        design_file(&path);
        let view = open(cx, &path);
        let (_, first) = pages(&view, cx);
        view.update(cx, |view, cx| view.page_action(PageAction::Add, cx));
        let (all, _) = pages(&view, cx);
        let second = all[1];
        view.update(cx, |view, cx| {
            view.page_action(PageAction::Select(first), cx);
            view.page_action(PageAction::Select(second), cx);
        });
        assert_eq!(pages(&view, cx), (vec![first, second], second));
        view.update(cx, |view, cx| {
            view.page_action(PageAction::Delete(second), cx)
        });
        assert_eq!(pages(&view, cx), (vec![first], first));
        assert_eq!(toast(&view, cx), None);
        // The only page stays, open, and the toast says why.
        view.update(cx, |view, cx| {
            view.page_action(PageAction::Delete(first), cx)
        });
        assert_eq!(pages(&view, cx), (vec![first], first));
        assert!(toast(&view, cx).is_some());
    }

    #[gpui_kit::test]
    fn pages_are_renamed_and_duplicated(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-pages");
        let path = tmp.0.join("design.figcraft");
        design_file(&path);
        let view = open(cx, &path);
        let (_, first) = pages(&view, cx);
        let steps = undo_steps(&view, cx);
        // A blank name, or the page's own, changes nothing (as for a layer).
        view.update(cx, |view, cx| {
            view.page_action(PageAction::Rename(first, " ".into()), cx);
            view.page_action(PageAction::Rename(first, "Page 1".into()), cx);
        });
        assert_eq!(name_of(&view, first, cx), "Page 1");
        assert_eq!(undo_steps(&view, cx), steps);
        view.update(cx, |view, cx| {
            view.page_action(PageAction::Rename(first, "Cover".into()), cx);
            view.page_action(PageAction::Duplicate(first), cx);
        });
        assert_eq!(name_of(&view, first, cx), "Cover");
        // The copy comes right after the page, and opens.
        let (all, open) = pages(&view, cx);
        assert_eq!(all.len(), 2);
        assert_eq!((all[0], open), (first, all[1]));
        assert_eq!(name_of(&view, all[1], cx), "Copy of Cover");
        assert_eq!(toast(&view, cx), None);
    }

    #[gpui_kit::test]
    fn drop_reorders(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-drop");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let view = open(cx, &path);
        assert_eq!(children(&view, t.f, cx), [t.g, t.a, t.b]);
        let steps = undo_steps(&view, cx);
        // F open: F, B, A and G. A, dragged, lands above B, on top of F's layers.
        view.update(cx, |view, cx| {
            view.expanded.insert(t.f);
            view.drag = Some(LayerDrag {
                ids: vec![t.a],
                over: None,
            });
            view.drop_rows(1, DropPos::Above, cx);
        });
        assert_eq!(children(&view, t.f, cx), [t.g, t.b, t.a]);
        assert!(view.read_with(cx, |view, _| view.drag.is_none()));
        assert_eq!(undo_steps(&view, cx), steps + 1);
        view.update(cx, |view, cx| view.choose("edit.undo", json!({}), cx));
        assert_eq!(children(&view, t.f, cx), [t.g, t.a, t.b]);
        // A drop that would leave the layers where they are makes no undo step.
        view.update(cx, |view, cx| {
            view.drag = Some(LayerDrag {
                ids: vec![t.b],
                over: None,
            });
            view.drop_rows(2, DropPos::Above, cx);
        });
        assert_eq!(children(&view, t.f, cx), [t.g, t.a, t.b]);
        assert_eq!(undo_steps(&view, cx), steps);
    }

    /// P4.T1's review: a layer inside an instance is the engine's to refuse. Dropped on a free
    /// frame, it stays where it was, the toast says why, and no undo step is made.
    #[gpui_kit::test]
    fn a_layer_dragged_out_of_an_instance_stays_and_the_toast_says_why(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-instance");
        let path = tmp.0.join("design.figcraft");
        let mut session = figcraft_engine::Session::new();
        let mut make = |command: &str, params: Value| {
            let made = session.execute(command, &params).unwrap();
            figcraft_engine::params::id_of(&made["id"]).unwrap()
        };
        let card = make(
            "node.create",
            json!({"type": "frame", "name": "Card", "x": 0, "y": 0, "width": 100, "height": 100}),
        );
        make(
            "node.create",
            json!({"type": "rectangle", "name": "Label", "parent": card, "width": 20, "height": 20}),
        );
        let main = make("component.create", json!({"ids": [card]}));
        let instance = make(
            "instance.create",
            json!({"component": main, "x": 200, "y": 0}),
        );
        let free = make(
            "node.create",
            json!({"type": "frame", "name": "Free", "x": 400, "y": 0, "width": 100, "height": 100}),
        );
        session.execute("select.none", &json!({})).unwrap();
        session
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        let view = open(cx, &path);
        let inside = children(&view, instance, cx);
        let [label] = inside[..] else {
            panic!("the instance holds {inside:?}")
        };
        let steps = undo_steps(&view, cx);
        view.update(cx, |view, cx| {
            view.expanded.insert(instance);
            let rows = view.layer_rows();
            let over = rows.iter().position(|row| row.id == free).unwrap();
            view.drag = Some(LayerDrag {
                ids: vec![label],
                over: None,
            });
            view.drop_rows(over, DropPos::Into, cx);
        });
        assert_eq!(children(&view, instance, cx), [label]);
        assert!(children(&view, free, cx).is_empty());
        assert!(selection(&view, cx).is_empty());
        assert_eq!(undo_steps(&view, cx), steps);
        assert!(toast(&view, cx).is_some());
    }

    /// Spec D11: the panel shows at first beside a design file, not beside a board.
    #[gpui_kit::test]
    fn the_panel_shows_beside_a_design_file_and_not_a_board(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-defaults");
        let design = tmp.0.join("design.figcraft");
        design_file(&design);
        let view = open(cx, &design);
        assert!(view.read_with(cx, |view, _| view.show_layers));
        let board = open(cx, &tmp.0.join("canvas.figcraft"));
        assert!(!board.read_with(cx, |view, _| view.show_layers));
    }

    /// Rulings P4-C10 and P4-C16: `view.layers` (⌥1) and `view.leftSidebar` (⇧⌘\), from an
    /// agent's command op, show and hide the panel at once.
    #[gpui_kit::test]
    fn view_layers_and_the_left_sidebar_show_and_hide_the_panel(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-op");
        let view = open(cx, &tmp.0.join("canvas.figcraft"));
        let shown = |cx: &TestAppContext| view.read_with(cx, |view, _| view.show_layers);
        let reply = run_op(&view, command("view.layers"), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(shown(cx));
        run_op(&view, command("view.leftSidebar"), cx);
        assert!(!shown(cx));
        run_op(&view, command("view.leftSidebar"), cx);
        assert!(shown(cx));
        run_op(&view, command("view.layers"), cx);
        assert!(!shown(cx));
        assert!(view.read_with(cx, |view, _| {
            view.board().unwrap().session.ui_requests.is_empty()
        }));
    }

    /// Ruling P4-C9: Collapse layers (⌥L) closes every row but those leading to the selection.
    #[gpui_kit::test]
    fn collapse_layers_keeps_the_way_to_the_selection(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-collapse");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let view = open(cx, &path);
        let expanded = |cx: &TestAppContext| view.read_with(cx, |view, _| view.expanded.clone());
        view.update(cx, |view, _| {
            view.expanded.extend([t.f, t.g, t.b]);
            view.board_mut().unwrap().session.set_selection(vec![t.c]);
        });
        run_op(&view, command("layers.collapseAll"), cx);
        assert_eq!(expanded(cx), HashSet::from([t.f, t.g]));
        run_op(&view, command("select.none"), cx);
        run_op(&view, command("layers.collapseAll"), cx);
        assert!(expanded(cx).is_empty());
    }

    /// Ruling P4-C11: Rename (⌘R, F2) shows the panel, opens the layers holding the selection, and
    /// starts typing in the row of its top layer, which it selects alone.
    #[gpui_kit::test]
    fn rename_from_the_keys_starts_in_the_row_of_the_top_layer(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-rename-request");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let view = open(cx, &path);
        view.update(cx, |view, _| {
            view.show_layers = false;
            view.board_mut().unwrap().session.set_selection(vec![t.c]);
        });
        // `object.rename` without a name asks the panel for it.
        let reply = run_op(&view, command("object.rename"), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        view.read_with(cx, |view, _| {
            assert!(view.show_layers);
            assert!(view.expanded.contains(&t.f) && view.expanded.contains(&t.g));
            assert_eq!(view.renaming, Some(t.c));
        });
        // Of several layers, the top one listed: B, above A.
        view.update(cx, |view, cx| {
            view.renaming = None;
            view.board_mut()
                .unwrap()
                .session
                .set_selection(vec![t.a, t.b]);
            cx.notify();
        });
        run_op(&view, command("object.rename"), cx);
        assert_eq!(view.read_with(cx, |view, _| view.renaming), Some(t.b));
        assert_eq!(selection(&view, cx), [t.b]);
    }

    /// Ruling P4-C8: the panel goes with the UI, and comes back with it. Ruling P4-C14: the canvas,
    /// with its toolbar, is laid out beside it, not under it.
    #[gpui_kit::test]
    fn the_layers_panel_goes_with_the_ui_and_comes_back(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-ui");
        let path = tmp.0.join("design.figcraft");
        design_file(&path);
        let (view, mut cx) = open_in_window(cx, &path);
        let canvas = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| {
                let bounds = view.bounds;
                (f32::from(bounds.origin.x), f32::from(bounds.size.width))
            })
        };
        assert!(drawn(&mut cx, "board-layers-panel"));
        assert_eq!(canvas(&cx), (WIDTH, 1000. - WIDTH - design_panel::WIDTH));
        let toolbar = cx.debug_bounds("board-toolbar").unwrap();
        assert!(f32::from(toolbar.origin.x) > WIDTH, "{toolbar:?}");
        change(&view, &mut cx, |view, cx| {
            view.choose("view.toggleUi", json!({}), cx)
        });
        assert!(!drawn(&mut cx, "board-layers-panel"));
        assert!(view.read_with(&cx, |view, _| view.show_layers));
        assert_eq!(canvas(&cx), (0., 1000.));
        change(&view, &mut cx, |view, cx| {
            view.choose("view.toggleUi", json!({}), cx)
        });
        assert!(drawn(&mut cx, "board-layers-panel"));
        assert_eq!(canvas(&cx).0, WIDTH);
    }

    /// Ruling P4-C13: the top bar's toggle sits after the zoom menu, before the Design panel's,
    /// and shows and hides the panel.
    #[gpui_kit::test]
    fn the_top_bar_toggle_shows_and_hides_the_panel(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-top-bar");
        let path = tmp.0.join("design.figcraft");
        design_file(&path);
        let (view, mut cx) = open_in_window(cx, &path);
        let left = |cx: &mut VisualTestContext, selector| {
            f32::from(cx.debug_bounds(selector).unwrap().origin.x)
        };
        let (zoom, toggle, design) = (
            left(&mut cx, "board-zoom"),
            left(&mut cx, "board-layers"),
            left(&mut cx, "board-design"),
        );
        assert!(zoom < toggle && toggle < design, "{zoom} {toggle} {design}");
        click(&mut cx, "board-layers");
        cx.run_until_parked();
        assert!(!view.read_with(&cx, |view, _| view.show_layers));
        assert!(!drawn(&mut cx, "board-layers-panel"));
        click(&mut cx, "board-layers");
        cx.run_until_parked();
        assert!(drawn(&mut cx, "board-layers-panel"));
    }

    /// Ruling P4-C5 and the filter ruling of P4.T1's review: a filter shows what it finds open
    /// without adding to the layers the user opened, its arrows do nothing, a filter that finds
    /// nothing says so, and clearing it gives the tree back as it was.
    #[gpui_kit::test]
    fn the_filter_leaves_the_open_layers_alone(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-filter");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let (view, mut cx) = open_in_window(cx, &path);
        change(&view, &mut cx, |view, _| {
            view.expanded = HashSet::from([t.f])
        });
        let expanded = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.expanded.clone());
        assert!(drawn(&mut cx, row(t.a)) && !drawn(&mut cx, row(t.c)));

        change(&view, &mut cx, |view, _| view.layer_filter = "c".into());
        // C, under F and G, both open: A and B aren't listed.
        assert!(drawn(&mut cx, row(t.f)) && drawn(&mut cx, row(t.g)) && drawn(&mut cx, row(t.c)));
        assert!(!drawn(&mut cx, row(t.a)) && !drawn(&mut cx, row(t.b)));
        assert_eq!(expanded(&cx), HashSet::from([t.f]));
        // G's arrow does nothing under the filter.
        click(&mut cx, selector(format!("layers-arrow-{}", t.g.0)));
        cx.run_until_parked();
        assert_eq!(expanded(&cx), HashSet::from([t.f]));
        assert!(drawn(&mut cx, row(t.c)));

        // Nothing found: the list says so, and shows no row of the tree before it.
        change(&view, &mut cx, |view, _| view.layer_filter = "zzz".into());
        assert!(drawn(&mut cx, "layers-empty"));
        assert!(!drawn(&mut cx, row(t.f)));
        assert_eq!(NO_MATCHES, "No matching layers");

        change(&view, &mut cx, |view, _| view.layer_filter.clear());
        assert_eq!(expanded(&cx), HashSet::from([t.f]));
        assert!(drawn(&mut cx, row(t.a)) && drawn(&mut cx, row(t.g)));
        assert!(!drawn(&mut cx, row(t.c)));
    }

    /// Ruling P4-C7: a key typed in the filter goes to the filter, and the canvas's keys come back
    /// with the keyboard.
    #[gpui_kit::test]
    fn typing_in_the_filter_leaves_the_canvas_keys_alone(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-filter-keys");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let (view, mut cx) = open_in_window(cx, &path);
        // With the rectangle tool, "v" on the canvas would choose Move.
        change(&view, &mut cx, |view, cx| {
            view.choose("tool.rectangle", json!({}), cx)
        });
        click(&mut cx, "layers-filter");
        cx.simulate_keystrokes("v");
        cx.run_until_parked();
        assert_eq!(tool(&view, &cx), "rectangle");
        assert_eq!(
            view.read_with(&cx, |view, _| view.layer_filter.clone()),
            "v"
        );
        assert!(!drawn(&mut cx, row(t.f)));
        // Back on the canvas, "v" is its key again.
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("v");
        assert_eq!(tool(&view, &cx), "move");
    }

    /// Ruling P4-C12: hovering a row outlines its layer on the canvas, and leaving the rows ends
    /// it. The row's eye and padlock show while it is hovered, or while the layer is hidden.
    #[gpui_kit::test]
    fn hovering_a_row_outlines_its_layer(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-hover");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let (view, mut cx) = open_in_window(cx, &path);
        change(&view, &mut cx, |view, _| {
            view.expanded.insert(t.f);
        });
        let eye = |id: NodeId| selector(format!("layers-eye-{}", id.0));
        let none = Modifiers::default();
        assert!(!drawn(&mut cx, eye(t.a)));

        let over_a = middle(&mut cx, row(t.a));
        cx.simulate_mouse_move(over_a, None, none);
        cx.run_until_parked();
        assert_eq!(hovered(&view, &cx), Some(t.a));
        assert!(drawn(&mut cx, eye(t.a)));
        let over_b = middle(&mut cx, row(t.b));
        cx.simulate_mouse_move(over_b, None, none);
        cx.run_until_parked();
        assert_eq!(hovered(&view, &cx), Some(t.b));
        assert!(!drawn(&mut cx, eye(t.a)));
        // Onto the filter, under no row.
        let off = middle(&mut cx, "layers-filter");
        cx.simulate_mouse_move(off, None, none);
        cx.run_until_parked();
        assert_eq!(hovered(&view, &cx), None);

        // A hidden layer's eye shows without a hover, and a click on it shows the layer.
        change(&view, &mut cx, |view, cx| view.toggle_visible(t.a, cx));
        assert!(drawn(&mut cx, eye(t.a)));
        let selected = selection(&view, &cx);
        click(&mut cx, eye(t.a));
        cx.run_until_parked();
        assert_eq!(shown_and_locked(&view, t.a, &cx), (true, false));
        // The click was the eye's: the row's layer wasn't selected by it.
        assert_eq!(selection(&view, &cx), selected);
    }

    /// The reveal: a layer selected on the canvas opens the layers holding it in the tree, once,
    /// so that a row closed by hand afterwards stays closed.
    #[gpui_kit::test]
    fn a_selection_made_on_the_canvas_opens_its_way_in_the_tree(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-reveal");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let (view, mut cx) = open_in_window(cx, &path);
        assert!(!drawn(&mut cx, row(t.c)));
        change(&view, &mut cx, |view, _| {
            view.board_mut().unwrap().session.set_selection(vec![t.c]);
        });
        assert!(drawn(&mut cx, row(t.c)));
        assert_eq!(
            view.read_with(&cx, |view, _| view.expanded.clone()),
            HashSet::from([t.f, t.g])
        );
        // Closed by hand, G stays closed.
        click(&mut cx, selector(format!("layers-arrow-{}", t.g.0)));
        cx.run_until_parked();
        assert!(!drawn(&mut cx, row(t.c)));
        assert!(drawn(&mut cx, row(t.g)));
    }

    /// A click on a row selects its layer, and the canvas keeps the keyboard; a double-click types
    /// a new name in the row, from the name the layer has: Enter keeps it, Escape drops it, and a
    /// layer with no name keeps none when nothing was typed (P4.T1's rename ruling).
    #[gpui_kit::test]
    fn a_double_click_renames_in_the_row(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-rename-row");
        let path = tmp.0.join("design.figcraft");
        let mut doc = Document::new("Design");
        let page = doc.first_page().unwrap();
        let nameless = add(&mut doc, page, NodeKind::Rectangle, "");
        let card = add(&mut doc, page, NodeKind::Frame, "Card");
        write(&path, doc);
        let (view, mut cx) = open_in_window(cx, &path);
        let field = |cx: &VisualTestContext| {
            view.read_with(cx, |view, cx| {
                let field = view.layers.field.as_ref()?;
                Some(field.input.read(cx).value().to_string())
            })
        };
        let canvas_has_keyboard = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| view.read(cx).focus.is_focused(window))
        };
        let steps = undo_steps(&view, &cx);
        click(&mut cx, row(card));
        cx.run_until_parked();
        assert_eq!(selection(&view, &cx), [card]);
        assert!(canvas_has_keyboard(&mut cx));

        let at = middle(&mut cx, row(card));
        double_click(&mut cx, at);
        assert_eq!(view.read_with(&cx, |view, _| view.renaming), Some(card));
        assert!(drawn(&mut cx, "layers-rename"));
        assert_eq!(field(&cx).as_deref(), Some("Card"));
        // What is typed replaces the name, which starts selected.
        cx.simulate_keystrokes("H e r o enter");
        cx.run_until_parked();
        assert_eq!(name_of(&view, card, &cx), "Hero");
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        assert!(!drawn(&mut cx, "layers-rename"));
        assert!(canvas_has_keyboard(&mut cx));

        // Escape drops what was typed.
        let at = middle(&mut cx, row(card));
        double_click(&mut cx, at);
        cx.simulate_keystrokes("X escape");
        cx.run_until_parked();
        assert_eq!(name_of(&view, card, &cx), "Hero");
        assert_eq!(view.read_with(&cx, |view, _| view.renaming), None);
        assert!(!drawn(&mut cx, "layers-rename"));

        // A layer with no name: the row shows its kind, and the field starts empty.
        let at = middle(&mut cx, row(nameless));
        double_click(&mut cx, at);
        assert_eq!(field(&cx).as_deref(), Some(""));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(name_of(&view, nameless, &cx), "");
        assert_eq!(undo_steps(&view, &cx), steps + 1);
        assert_eq!(toast(&view, &cx), None);
    }

    /// Plan 2's final review, carried: with the Move tool, a double-click on a frame's name on the
    /// canvas starts typing its name in the panel, and a blank name keeps the old one.
    #[gpui_kit::test]
    fn a_double_click_on_a_frames_name_renames_it(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-rename-label");
        let path = tmp.0.join("design.figcraft");
        let mut session = figcraft_engine::Session::new();
        let frame = json!({
            "type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100,
        });
        let made = session.execute("node.create", &frame).unwrap();
        let frame = figcraft_engine::params::id_of(&made["id"]).unwrap();
        session.execute("select.none", &json!({})).unwrap();
        session
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        let (view, mut cx) = open_in_window(cx, &path);
        change(&view, &mut cx, |view, _| view.show_layers = false);
        let steps = undo_steps(&view, &cx);
        // The middle of the frame's name, where the canvas painted it.
        let label = view.read_with(&cx, |view, _| {
            let board = view.board().unwrap();
            let (id, quad) = board.session.tool.labels[0];
            assert_eq!(id, frame);
            let x = quad.iter().map(|p| p.x).sum::<f64>() / 4.0;
            let y = quad.iter().map(|p| p.y).sum::<f64>() / 4.0;
            let (x, y) = board.session.ds().unwrap().view.page_to_screen(x, y);
            view.bounds.origin + point(px(x as f32), px(y as f32))
        });
        double_click(&mut cx, label);
        assert!(view.read_with(&cx, |view, _| view.show_layers));
        assert_eq!(view.read_with(&cx, |view, _| view.renaming), Some(frame));
        assert!(drawn(&mut cx, "layers-rename"));
        cx.simulate_keystrokes("backspace enter");
        cx.run_until_parked();
        assert_eq!(name_of(&view, frame, &cx), "Frame");
        assert_eq!(undo_steps(&view, &cx), steps);
        assert_eq!(view.read_with(&cx, |view, _| view.renaming), None);
    }

    /// A drag of a row onto the top of another moves the layer there, with the mouse.
    #[gpui_kit::test]
    fn a_row_dragged_onto_the_top_of_another_goes_above_it(cx: &mut TestAppContext) {
        let tmp = Scratch::new("layers-drag");
        let path = tmp.0.join("design.figcraft");
        let t = design_file(&path);
        let (view, mut cx) = open_in_window(cx, &path);
        change(&view, &mut cx, |view, _| {
            view.expanded.insert(t.f);
        });
        let (left, none) = (MouseButton::Left, Modifiers::default());
        let from = middle(&mut cx, row(t.a));
        let b = cx.debug_bounds(row(t.b)).unwrap();
        let onto = point(b.center().x, b.origin.y + px(3.));
        cx.simulate_mouse_down(from, left, none);
        cx.simulate_mouse_move(from + point(px(0.), px(-6.)), left, none);
        cx.simulate_mouse_move(onto, left, none);
        cx.run_until_parked();
        assert_eq!(
            view.read_with(&cx, |view, _| view.drag.clone()),
            Some(LayerDrag {
                ids: vec![t.a],
                over: Some((1, DropPos::Above)),
            })
        );
        cx.simulate_mouse_up(onto, left, none);
        cx.run_until_parked();
        assert_eq!(children(&view, t.f, &cx), [t.g, t.b, t.a]);
        assert!(view.read_with(&cx, |view, _| view.drag.is_none()));
    }

    #[test]
    fn a_drop_goes_into_a_row_in_its_middle_half_when_the_row_takes_layers() {
        assert_eq!(drop_pos(0.1, true), DropPos::Above);
        assert_eq!(drop_pos(0.3, true), DropPos::Into);
        assert_eq!(drop_pos(0.7, true), DropPos::Into);
        assert_eq!(drop_pos(0.8, true), DropPos::Below);
        assert_eq!(drop_pos(0.3, false), DropPos::Above);
        assert_eq!(drop_pos(0.6, false), DropPos::Below);
    }

    /// Every icon the panel and its toggle show is one Sawit embeds (as ruling P5-C11 has it for
    /// the Design panel).
    #[test]
    fn every_icon_of_the_layers_panel_is_embedded() {
        let kinds = [
            LayerIcon::Frame,
            LayerIcon::Section,
            LayerIcon::Group,
            LayerIcon::Component,
            LayerIcon::Instance,
            LayerIcon::Text,
            LayerIcon::Rectangle,
            LayerIcon::Ellipse,
            LayerIcon::Vector,
            LayerIcon::Line,
            LayerIcon::Image,
            LayerIcon::Sticky,
            LayerIcon::Shape,
            LayerIcon::Connector,
            LayerIcon::Ink,
            LayerIcon::Stamp,
            LayerIcon::Table,
            LayerIcon::Other,
        ];
        let icons = ICONS.iter().copied().chain(kinds.map(layer_icon));
        for icon in icons.chain([top_bar::LAYERS_ICON]) {
            let path = icon.path();
            let svg = crate::assets::AppAssets.load(&path).unwrap();
            assert!(svg.is_some(), "{path} isn't embedded");
        }
        assert_eq!(NO_LAYERS, "No layers on this page");
    }
}
