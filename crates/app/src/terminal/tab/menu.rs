//! The pane menu: its items and their keys, opening it, and what each item does.

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use sawit_core::terminal::PaneId;

use super::{Menu, TerminalTab};
use crate::terminal::{
    ClosePane, RenamePane, SplitDown, SplitRight, ToggleSendRightClicks, ZoomPane,
};

/// One item of the pane menu: its label, `Some(checked)` for a toggle, and
/// the action whose key the menu shows beside it.
pub type PaneMenuItem = (&'static str, Option<bool>, Box<dyn Action>);

/// The pane menu, in order. A click on an item acts on the pane the menu
/// was opened for; its key acts on the pane with the keyboard.
pub fn pane_menu_items(zoomed: bool, send_right_clicks: bool) -> Vec<PaneMenuItem> {
    vec![
        ("Rename pane", None, Box::new(RenamePane)),
        ("Split right", None, Box::new(SplitRight)),
        ("Split down", None, Box::new(SplitDown)),
        (
            if zoomed { "Unzoom" } else { "Zoom" },
            None,
            Box::new(ZoomPane),
        ),
        (
            "Send right-clicks to pane",
            Some(send_right_clicks),
            Box::new(ToggleSendRightClicks),
        ),
        ("Close pane", None, Box::new(ClosePane)),
    ]
}

impl TerminalTab {
    /// Opens the pane menu for `pane` once the window can be reached.
    pub(super) fn request_menu(&mut self, pane: PaneId, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.in_window(cx, move |t, window, cx| t.open_menu(pane, at, window, cx));
    }

    /// Shows the pane menu for `pane` at `at` (window coordinates) and
    /// gives it the keyboard; dismissing it hands the keyboard back.
    fn open_menu(
        &mut self,
        pane: PaneId,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.views.get(&pane) else {
            return;
        };
        let back = view.focus_handle(cx);
        let zoomed = self.state.zoomed == Some(pane);
        let split = self.state.is_split();
        let send = self
            .state
            .panes
            .get(&pane)
            .is_some_and(|meta| meta.send_right_clicks);
        let tab = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            menu = menu.action_context(back);
            for (label, checked, action) in pane_menu_items(zoomed, send) {
                let tab = tab.clone();
                let item = match checked {
                    Some(checked) => PopupMenuItem::new(label).checked(checked),
                    None => PopupMenuItem::new(label),
                }
                .action(action)
                // One pane has nothing to zoom over.
                .disabled(!split && label == "Zoom");
                menu = menu.item(item.on_click(move |_, window, cx| {
                    let _ = tab.update(cx, |t, cx| t.run_menu_item(pane, label, window, cx));
                }));
            }
            menu
        });
        menu.focus_handle(cx).focus(window, cx);
        let dismiss = cx.subscribe(&menu, |this, menu, _: &DismissEvent, cx| {
            if this.menu.as_ref().is_some_and(|m| m.view == menu) {
                this.menu = None;
                cx.notify();
            }
        });
        self.menu = Some(Menu {
            view: menu,
            at,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// Does what the pane menu item `label` says, to `pane`.
    pub(super) fn run_menu_item(
        &mut self,
        pane: PaneId,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match label {
            "Rename pane" => self.rename_pane(pane, window, cx),
            "Split right" => {
                self.focus_pane(pane, window, cx);
                window.dispatch_action(Box::new(SplitRight), cx);
            }
            "Split down" => {
                self.focus_pane(pane, window, cx);
                window.dispatch_action(Box::new(SplitDown), cx);
            }
            "Zoom" | "Unzoom" => self.toggle_zoom(pane, window, cx),
            "Send right-clicks to pane" => self.toggle_right_clicks(pane, cx),
            "Close pane" => self.close_pane(pane, window, cx),
            _ => {}
        }
    }

    /// Flips whether `pane`'s right-clicks go to its program.
    pub(super) fn toggle_right_clicks(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        self.state.toggle_right_clicks(pane);
        let send = self
            .state
            .panes
            .get(&pane)
            .is_some_and(|meta| meta.send_right_clicks);
        if let Some(view) = self.views.get(&pane) {
            view.update(cx, |v, _| v.send_right_clicks = send);
        }
    }
}
