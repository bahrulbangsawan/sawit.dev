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

mod actions;
mod drag;
mod rename;
mod rows;

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::guard;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::layers::{self, DropPos, LayerIcon, LayerRow, PageRow};
use serde_json::Value;

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

/// The ids in `data.ids` of a UI request.
pub(super) fn ids_in(data: &Value) -> Vec<NodeId> {
    let ids = data.get("ids").and_then(Value::as_array);
    ids.into_iter()
        .flatten()
        .filter_map(figcraft_engine::params::id_of)
        .collect()
}

impl BoardView {
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
mod tests;
