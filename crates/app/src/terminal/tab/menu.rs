//! The pane menu (right-click inside a terminal): its items in their groups, which ones apply,
//! their keys (read from the keymap, as the file tab menu shows them), opening it, and what each
//! does. Items that don't apply are disabled, not hidden, so the menu keeps its shape.

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use sawit_core::terminal::{PaneId, Target};

use super::{Menu, TerminalTab};
use crate::terminal::view::TerminalView;
use crate::terminal::{
    Clear, ClosePane, Copy, CopyError, CopyLastOutput, Paste, RenamePane, ResetTerminal, SelectAll,
    SplitDown, SplitRight, ToggleSendRightClicks, ZoomPane,
};

/// What a pane menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneAct {
    Copy,
    Paste,
    SelectAll,
    /// ⌘F's find bar over the pane.
    Find,
    CopyLastOutput,
    CopyError,
    OpenLink,
    CopyLink,
    OpenFile,
    Clear,
    Reset,
    Rename,
    SplitRight,
    SplitDown,
    Zoom,
    FullScreen,
    SendRightClicks,
    Close,
}

/// One entry of the pane menu.
pub enum PaneEntry {
    Separator,
    Item {
        label: &'static str,
        act: PaneAct,
        /// `Some(checked)` for a toggle.
        checked: Option<bool>,
        /// The action whose keys show beside it, if it has one.
        action: Option<Box<dyn Action>>,
        enabled: bool,
    },
}

/// What decides the pane menu's items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneMenuState {
    pub zoomed: bool,
    pub split: bool,
    pub send_right_clicks: bool,
    pub has_selection: bool,
    /// The link or the existing file the right-click landed on.
    pub target: Option<Target>,
    /// The tab is locked: Close Pane is off.
    pub locked: bool,
}

/// The pane menu, in its groups. A click on an item acts on the pane the menu was opened for;
/// its key acts on the pane with the keyboard.
pub fn pane_menu(s: &PaneMenuState) -> Vec<PaneEntry> {
    let item = |label, act, action: Option<Box<dyn Action>>, enabled| PaneEntry::Item {
        label,
        act,
        checked: None,
        action,
        enabled,
    };
    let link = matches!(s.target, Some(Target::Link(_)));
    let file = matches!(s.target, Some(Target::Path(_)));
    vec![
        item("Copy", PaneAct::Copy, Some(Box::new(Copy)), s.has_selection),
        item("Paste", PaneAct::Paste, Some(Box::new(Paste)), true),
        item(
            "Select All",
            PaneAct::SelectAll,
            Some(Box::new(SelectAll)),
            true,
        ),
        item(
            "Find…",
            PaneAct::Find,
            Some(Box::new(crate::find::Find)),
            true,
        ),
        PaneEntry::Separator,
        item(
            "Copy Last Output",
            PaneAct::CopyLastOutput,
            Some(Box::new(CopyLastOutput)),
            true,
        ),
        item(
            "Copy Error",
            PaneAct::CopyError,
            Some(Box::new(CopyError)),
            true,
        ),
        item("Open Link", PaneAct::OpenLink, None, link),
        item("Copy Link", PaneAct::CopyLink, None, link),
        item("Open File", PaneAct::OpenFile, None, file),
        PaneEntry::Separator,
        item("Clear", PaneAct::Clear, Some(Box::new(Clear)), true),
        item(
            "Reset Terminal",
            PaneAct::Reset,
            Some(Box::new(ResetTerminal)),
            true,
        ),
        PaneEntry::Separator,
        item(
            "Rename Pane",
            PaneAct::Rename,
            Some(Box::new(RenamePane)),
            true,
        ),
        item(
            "Split Right",
            PaneAct::SplitRight,
            Some(Box::new(SplitRight)),
            true,
        ),
        item(
            "Split Down",
            PaneAct::SplitDown,
            Some(Box::new(SplitDown)),
            true,
        ),
        item(
            if s.zoomed { "Unzoom Pane" } else { "Zoom Pane" },
            PaneAct::Zoom,
            Some(Box::new(ZoomPane)),
            // One pane has nothing to zoom over.
            s.split,
        ),
        item(
            "Full Screen",
            PaneAct::FullScreen,
            Some(Box::new(crate::workspace::MaximizeTerminal)),
            true,
        ),
        PaneEntry::Separator,
        PaneEntry::Item {
            label: "Send Right-Clicks to Pane",
            act: PaneAct::SendRightClicks,
            checked: Some(s.send_right_clicks),
            action: Some(Box::new(ToggleSendRightClicks)),
            enabled: true,
        },
        PaneEntry::Separator,
        item(
            "Close Pane",
            PaneAct::Close,
            Some(Box::new(ClosePane)),
            !s.locked,
        ),
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
        let state = PaneMenuState {
            zoomed: self.state.zoomed == Some(pane),
            split: self.state.is_split(),
            send_right_clicks: self
                .state
                .panes
                .get(&pane)
                .is_some_and(|meta| meta.send_right_clicks),
            has_selection: view.read(cx).has_selection(),
            target: self.menu_target(pane, cx),
            locked: self.state.locked,
        };
        let tab = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            menu = menu.action_context(back);
            for entry in pane_menu(&state) {
                let PaneEntry::Item {
                    label,
                    act,
                    checked,
                    action,
                    enabled,
                } = entry
                else {
                    menu = menu.separator();
                    continue;
                };
                let tab = tab.clone();
                // An item with an action shows the keys the keymap gives it in the pane (its
                // action context), as the file tab menu shows its keys.
                let mut item = PopupMenuItem::new(label);
                if let Some(checked) = checked {
                    item = item.checked(checked);
                }
                if let Some(action) = action {
                    item = item.action(action);
                }
                menu = menu.item(item.disabled(!enabled).on_click(move |_, window, cx| {
                    let _ = tab.update(cx, |t, cx| t.run_pane_act(pane, act, window, cx));
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

    /// The link, or the existing file, the right-click that opened `pane`'s menu landed on.
    fn menu_target(&self, pane: PaneId, cx: &App) -> Option<Target> {
        let view = self.views.get(&pane)?.read(cx);
        match view.menu_target.clone()? {
            Target::Link(url) => Some(Target::Link(url)),
            Target::Path(path) => view
                .resolve_path(&path)
                .map(|p| Target::Path(p.to_string_lossy().into_owned())),
        }
    }

    /// Does what the pane menu item `act` says, to `pane`.
    pub(super) fn run_pane_act(
        &mut self,
        pane: PaneId,
        act: PaneAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.views.get(&pane).cloned() else {
            return;
        };
        let target = self.menu_target(pane, cx);
        match act {
            PaneAct::Copy => view.update(cx, |v, cx| v.copy_selection(cx)),
            PaneAct::Paste => view.update(cx, |v, cx| v.paste_clipboard(cx)),
            PaneAct::SelectAll => view.update(cx, |v, cx| v.select_all(cx)),
            PaneAct::CopyLastOutput => self.copy_output(pane, false, cx),
            PaneAct::CopyError => self.copy_output(pane, true, cx),
            PaneAct::OpenLink => {
                if let Some(Target::Link(url)) = target {
                    cx.open_url(&url);
                }
            }
            PaneAct::CopyLink => {
                if let Some(Target::Link(url)) = target {
                    view.update(cx, |v, cx| v.copy_text(url, cx));
                }
            }
            PaneAct::OpenFile => {
                if let Some(Target::Path(path)) = target {
                    cx.emit(super::TabEvent::OpenFile(path.into()));
                }
            }
            PaneAct::Clear => view.update(cx, |v, cx| v.clear(cx)),
            PaneAct::Reset => view.update(cx, |v, cx| v.reset(cx)),
            PaneAct::Rename => self.rename_pane(pane, window, cx),
            PaneAct::SplitRight | PaneAct::SplitDown | PaneAct::FullScreen | PaneAct::Find => {
                // From the pane: Find opens its find bar, Full Screen fills the window with the
                // terminal (item 17's Maximize Terminal).
                self.focus_pane(pane, window, cx);
                let action: Box<dyn Action> = match act {
                    PaneAct::SplitRight => Box::new(SplitRight),
                    PaneAct::SplitDown => Box::new(SplitDown),
                    PaneAct::Find => Box::new(crate::find::Find),
                    _ => Box::new(crate::workspace::MaximizeTerminal),
                };
                window.dispatch_action(action, cx);
            }
            PaneAct::Zoom => self.toggle_zoom(pane, window, cx),
            PaneAct::SendRightClicks => self.toggle_right_clicks(pane, cx),
            PaneAct::Close => self.close_pane(pane, window, cx),
        }
    }

    /// Runs `f` on the focused pane's terminal (the pane keys' actions).
    pub fn on_focused(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut TerminalView, &mut Context<TerminalView>),
    ) {
        if let Some(view) = self.views.get(&self.state.focused) {
            view.update(cx, f);
        }
    }

    /// Clears `pane`'s scrollback and screen (Clear, an agent's `terminal_clear`).
    pub fn clear_pane(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if let Some(view) = self.views.get(&pane) {
            view.update(cx, |v, cx| v.clear(cx));
        }
    }

    /// Copies the output of `pane`'s last command, or only its errors (item 1).
    pub fn copy_output(&mut self, pane: PaneId, errors: bool, cx: &mut Context<Self>) {
        if let Some(view) = self.views.get(&pane) {
            view.update(cx, |v, cx| v.copy_last_output(errors, cx));
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
