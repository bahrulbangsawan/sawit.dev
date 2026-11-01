//! The panel's rows as they are drawn: the page list under its header, and the rows of the layer
//! tree in view; and what a hover, a click or a page's context menu on them does.

use std::ops::Range;

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::layers::{DropPos, LayerIcon, LayerRow, PageRow};

use super::drag::{DragLabel, DraggedRow};
use super::rename::rename_field;
use super::{
    ADD_PAGE_ICON, ARROW, CLOSED_ICON, DROP_MARK, HEADER, HIDDEN_ICON, INDENT, LOCKED_ICON,
    NO_LAYERS, NO_MATCHES, OPEN_ICON, PAGES_SHOWN, PAGES_TITLE, PageAction, ROW, ROW_PAD, Renamed,
    SHOWN_ICON, UNLOCKED_ICON, layer_icon,
};
use crate::board::{BoardView, option_icons};

impl BoardView {
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
    pub(super) fn render_pages_header(&self, cx: &mut Context<Self>) -> AnyElement {
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
    pub(super) fn render_pages(&self, pages: &[PageRow], cx: &mut Context<Self>) -> AnyElement {
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
    pub(super) fn render_tree(&self, rows: &[LayerRow], cx: &mut Context<Self>) -> AnyElement {
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

/// The context menu of page `id`'s row: rename, duplicate and delete it, each with its picture.
fn page_menu(menu: PopupMenu, view: WeakEntity<BoardView>, id: NodeId) -> PopupMenu {
    let act = |action: fn(&mut BoardView, NodeId, &mut Context<BoardView>)| {
        let view = view.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |view, cx| action(view, id, cx));
        }
    };
    menu.item(
        PopupMenuItem::new("Rename page")
            .icon(option_icons::RENAME_PAGE)
            .on_click(act(|view, id, cx| {
                view.start_page_rename(id, cx);
            })),
    )
    .item(
        PopupMenuItem::new("Duplicate page")
            .icon(option_icons::DUPLICATE_PAGE)
            .on_click(act(|view, id, cx| {
                view.page_action(PageAction::Duplicate(id), cx);
            })),
    )
    .item(
        PopupMenuItem::new("Delete page")
            .icon(option_icons::DELETE_PAGE)
            .on_click(act(|view, id, cx| {
                view.page_action(PageAction::Delete(id), cx);
            })),
    )
}
