//! The main menu and the context menu: FigCraft's menu model under Sawit's policy, as popups whose
//! items run their command ([`BoardView::run_menu`]) and whose submenus open beside their item.

use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::menu::{self, MenuNode};

use super::{ContextPopup, SCROLLS_PAST, guarded};
use crate::board::BoardView;

impl BoardView {
    /// The context menu, where it opened, over everything. Its popup has the keyboard.
    pub(super) fn render_context_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let at = self.menus.context_menu_at?;
        let menu = match &self.menus.context_menu {
            Some(popup) => popup.menu.clone(),
            None => {
                let nodes = (self.board())
                    .map(|board| guarded("ui.contextMenu", || menu::context_menu(&board.session)))
                    .unwrap_or_default();
                if nodes.is_empty() {
                    self.menus.context_menu_at = None;
                    return None;
                }
                let menu = to_popup(&nodes, cx.entity().downgrade(), window, cx);
                let closed =
                    cx.subscribe_in(&menu, window, |view, menu, _: &DismissEvent, _, cx| {
                        view.context_menu_closed(menu, cx);
                    });
                let keyboard = menu.focus_handle(cx);
                keyboard.focus(window, cx);
                self.menus.context_menu = Some(ContextPopup {
                    menu: menu.clone(),
                    keyboard,
                    _closed: closed,
                });
                menu
            }
        };
        let placed = anchored()
            .position(at.position)
            .snap_to_window_with_margin(px(8.))
            .child(menu);
        Some(
            deferred(placed)
                .with_priority(POPUP_PRIORITY)
                .into_any_element(),
        )
    }

    /// The context menu's popup `menu` closed itself (an item ran, Escape, a press outside). The
    /// menu is closed, unless another opened since, and the keyboard goes back to the canvas if
    /// the popup still has it.
    fn context_menu_closed(&mut self, menu: &Entity<PopupMenu>, cx: &mut Context<Self>) {
        if self
            .menus
            .context_menu
            .as_ref()
            .is_some_and(|popup| popup.menu == *menu)
        {
            self.close_menus(cx);
        }
    }
}

/// Fills `menu`, the one the top bar's ≡ button drops, with the main menu of `view`'s canvas as
/// it is now ([`menu::main_menu`]), its checks among it. Closed, it gives the canvas the keyboard.
pub(in crate::board) fn fill_main_menu(
    menu: PopupMenu,
    view: &WeakEntity<BoardView>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(canvas) = view.upgrade() else {
        return menu;
    };
    let (nodes, keyboard) = {
        let canvas = canvas.read(cx);
        let nodes = (canvas.board())
            .map(|board| guarded("ui.mainMenu", || menu::main_menu(&board.session)))
            .unwrap_or_default();
        (nodes, canvas.focus.clone())
    };
    add_items(menu.action_context(keyboard), &nodes, view, window, cx)
}

/// A popup of `nodes` for `view`'s canvas: an item runs its command ([`BoardView::run_menu`]),
/// and a submenu opens beside its item.
fn to_popup(
    nodes: &[MenuNode],
    view: WeakEntity<BoardView>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    PopupMenu::build(window, cx, |menu, window, cx| {
        add_items(menu, nodes, &view, window, cx)
    })
}

/// `menu` with `nodes` added: an item for each command, a popup for each submenu
/// ([`to_popup`]), and the lines between them. A long menu scrolls.
fn add_items(
    mut menu: PopupMenu,
    nodes: &[MenuNode],
    view: &WeakEntity<BoardView>,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    for node in nodes {
        menu = match node {
            MenuNode::Item {
                id,
                label,
                checked,
                enabled,
                ..
            } => {
                // The keys as the keymap binds them now, not FigCraft's in the model.
                let keys = crate::board::command_keys(id, cx);
                menu.item(
                    menu_item(id, label, keys.as_deref(), view.clone())
                        .checked(*checked == Some(true))
                        .disabled(!*enabled),
                )
            }
            MenuNode::Submenu { label, children } => {
                let submenu = to_popup(children, view.clone(), window, cx);
                menu.item(PopupMenuItem::submenu(label.clone(), submenu))
            }
            MenuNode::Separator => menu.separator(),
        };
    }
    menu.scrollable(nodes.len() > SCROLLS_PAST)
}

/// The item of command `id` called `label`, which runs it in `view`'s canvas. One with keys shows
/// them on the right (ruling P6-C15).
fn menu_item(
    id: &'static str,
    label: &str,
    keys: Option<&str>,
    view: WeakEntity<BoardView>,
) -> PopupMenuItem {
    let item = match keys {
        Some(keys) => {
            let label = SharedString::from(label.to_owned());
            let keys = SharedString::from(keys.to_owned());
            PopupMenuItem::element(move |_, cx| {
                h_flex()
                    .debug_selector(move || format!("board-menu-{id}"))
                    .w_full()
                    .gap_6()
                    .justify_between()
                    // As text a screen reader reads: a menu item drawn from elements has no name.
                    .child(crate::catalog::label(
                        SharedString::from(format!("board-menu-{id}")),
                        label.clone(),
                    ))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(keys.clone()),
                    )
            })
        }
        None => PopupMenuItem::new(label.to_owned()),
    };
    item.on_click(move |_, window, cx| {
        let _ = view.update(cx, |view, cx| view.run_menu(id, window, cx));
    })
}
