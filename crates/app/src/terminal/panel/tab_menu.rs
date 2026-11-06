//! The menu of a terminal tab (right-click on it in the strip): renaming it, its project, closing
//! it and others, locking and pinning it.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{CloseScope, TerminalPanel};
use crate::tab_groups::project_name;
use crate::terminal::RenameTab;
use crate::terminal::tab::TerminalTab;

/// What the menu is opened for.
#[derive(Clone)]
pub(super) struct MenuFor {
    pub panel: WeakEntity<TerminalPanel>,
    pub tab: WeakEntity<TerminalTab>,
    /// Every project open in the window.
    pub projects: Rc<[PathBuf]>,
    /// The tab's project.
    pub group: Option<PathBuf>,
    /// A pane on screen, whose keys the menu shows.
    pub keys_in: Rc<Option<FocusHandle>>,
}

/// Which items apply to the tab, by where it is and what it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuState {
    pub first: bool,
    pub last: bool,
    pub locked: bool,
    pub pinned: bool,
    /// Something runs in it.
    pub busy: bool,
    /// Another tab can be closed by Close Others.
    pub others: bool,
}

impl MenuState {
    /// Close is off while the tab is locked.
    pub fn can_close(&self) -> bool {
        !self.locked
    }
}

impl TerminalPanel {
    /// The menu's state for the tab at `ix`.
    pub fn menu_state(&self, ix: usize, cx: &App) -> MenuState {
        let state = &self.tabs[ix].read(cx).state;
        MenuState {
            first: ix == 0,
            last: ix + 1 == self.tabs.len(),
            locked: state.locked,
            pinned: state.pinned,
            busy: !self.running_in_tab(ix, cx).is_empty(),
            others: self.tabs.len() > 1,
        }
    }
}

/// Runs `f` on the panel with the tab's index, while the tab is still open.
fn on_tab(
    m: &MenuFor,
    f: impl Fn(&mut TerminalPanel, usize, &mut Window, &mut Context<TerminalPanel>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let (panel, tab) = (m.panel.clone(), m.tab.clone());
    move |_, window, cx| {
        let _ = panel.update(cx, |p, cx| {
            if let Some(ix) = p.index_of(&tab) {
                f(p, ix, window, cx)
            }
        });
    }
}

/// Builds the tab's menu. `had_keyboard`: the tab had the keyboard before the menu took it.
pub(super) fn build(
    m: &MenuFor,
    state: MenuState,
    had_keyboard: bool,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let others: Vec<PathBuf> = m
        .projects
        .iter()
        .filter(|p| Some(*p) != m.group.as_ref())
        .cloned()
        .collect();
    let menu = match &*m.keys_in {
        Some(pane) => menu.action_context(pane.clone()),
        None => menu,
    };
    let rename = PopupMenuItem::new("Rename Tab")
        .action(Box::new(RenameTab))
        .on_click(on_tab(m, |p, ix, window, cx| p.rename_tab(ix, window, cx)));
    let move_m = m.clone();
    let menu = menu.item(rename).when(!others.is_empty(), |menu| {
        menu.submenu("Move to Project", window, cx, move |mut menu, _, _| {
            for root in &others {
                let to = root.clone();
                let item = PopupMenuItem::new(project_name(root)).on_click(on_tab(
                    &move_m,
                    move |p, ix, _, cx| {
                        let tab = p.tabs[ix].downgrade();
                        p.move_to_group(&tab, Some(to.clone()), cx)
                    },
                ));
                menu = menu.item(item);
            }
            menu
        })
    });
    let menu = menu.when(m.group.is_some(), |menu| {
        menu.item(
            PopupMenuItem::new("Remove from Group").on_click(on_tab(m, |p, ix, _, cx| {
                let tab = p.tabs[ix].downgrade();
                p.move_to_group(&tab, None, cx)
            })),
        )
    });
    let close_tab = m.tab.clone();
    menu.separator()
        .item(
            PopupMenuItem::new("Close")
                .disabled(!state.can_close())
                .on_click(on_tab(m, move |p, _, window, cx| {
                    p.close_for_user(&close_tab, had_keyboard, window, cx)
                })),
        )
        .item(
            PopupMenuItem::new("Close Others")
                .disabled(!state.others)
                .on_click(on_tab(m, |p, ix, window, cx| {
                    p.close_tabs_for_user(CloseScope::Others, ix, window, cx)
                })),
        )
        .item(
            PopupMenuItem::new("Close All").on_click(on_tab(m, |p, ix, window, cx| {
                p.close_tabs_for_user(CloseScope::All, ix, window, cx)
            })),
        )
        .separator()
        .item(
            PopupMenuItem::new(if state.locked {
                "Unlock Tab"
            } else {
                "Lock Tab"
            })
            .on_click(on_tab(m, |p, ix, _, cx| {
                p.set_locked(ix, None, cx);
            })),
        )
        .item(
            PopupMenuItem::new(if state.pinned { "Unpin Tab" } else { "Pin Tab" }).on_click(
                on_tab(m, |p, ix, _, cx| {
                    p.set_pinned(ix, None, cx);
                }),
            ),
        )
}
