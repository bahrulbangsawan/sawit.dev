//! The menu of a terminal tab (right-click on it in the strip), as complete as the file tab menu
//! (backlog item 22): its items in their groups, which ones apply to the tab (first, last,
//! locked, busy…), their keys read from the keymap (shown beside each item from its action, in a
//! pane's context), and what each does. Items that don't apply are disabled, not
//! hidden, so the menu keeps its shape.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

use super::{CloseScope, TerminalPanel};
use crate::tab_groups::project_name;
use crate::terminal::tab::TerminalTab;
use crate::terminal::{
    CloseOtherTabs, CloseTab, CloseTabsLeft, CloseTabsRight, DuplicateTab, ExportOutput,
    ForceCloseAll, RenameTab, SafeCloseAll, ToggleLockTab, TogglePinTab,
};

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
    /// Another tab is open.
    pub others: bool,
    /// The tab is in a project's group.
    pub grouped: bool,
    /// Another project is open, to move it to.
    pub other_projects: bool,
}

/// What a tab menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAct {
    Rename,
    Duplicate,
    MoveToProject,
    RemoveFromGroup,
    Close,
    CloseMany(CloseScope),
    Lock,
    Pin,
    CopyLastOutput,
    CopyError,
    Clear,
    Export,
    FullScreen,
}

/// One entry of the tab menu.
pub struct TabEntry {
    pub label: &'static str,
    pub act: TabAct,
    /// The action whose keys show beside it.
    pub action: Option<Box<dyn Action>>,
    pub enabled: bool,
    /// A separator goes before it.
    pub after_separator: bool,
}

/// The tab menu, in its groups.
pub fn tab_menu(s: &MenuState) -> Vec<TabEntry> {
    let e = |label, act, action: Option<Box<dyn Action>>, enabled, after_separator| TabEntry {
        label,
        act,
        action,
        enabled,
        after_separator,
    };
    use TabAct::*;
    vec![
        e("Rename Tab", Rename, Some(Box::new(RenameTab)), true, false),
        e(
            "Duplicate Tab",
            Duplicate,
            Some(Box::new(DuplicateTab)),
            true,
            false,
        ),
        e(
            "Move to Project",
            MoveToProject,
            None,
            s.other_projects,
            false,
        ),
        e("Remove from Group", RemoveFromGroup, None, s.grouped, false),
        e("Close", Close, Some(Box::new(CloseTab)), !s.locked, true),
        e(
            "Close Others",
            CloseMany(CloseScope::Others),
            Some(Box::new(CloseOtherTabs)),
            s.others,
            false,
        ),
        e(
            "Close Left",
            CloseMany(CloseScope::Left),
            Some(Box::new(CloseTabsLeft)),
            !s.first,
            true,
        ),
        e(
            "Close Right",
            CloseMany(CloseScope::Right),
            Some(Box::new(CloseTabsRight)),
            !s.last,
            false,
        ),
        e(
            "Safe Close All",
            CloseMany(CloseScope::Idle),
            Some(Box::new(SafeCloseAll)),
            true,
            true,
        ),
        e(
            "Force Close All",
            CloseMany(CloseScope::All),
            Some(Box::new(ForceCloseAll)),
            true,
            false,
        ),
        e(
            if s.locked { "Unlock Tab" } else { "Lock Tab" },
            Lock,
            Some(Box::new(ToggleLockTab)),
            true,
            true,
        ),
        e(
            if s.pinned { "Unpin Tab" } else { "Pin Tab" },
            Pin,
            Some(Box::new(TogglePinTab)),
            true,
            false,
        ),
        e(
            "Copy Last Output",
            CopyLastOutput,
            Some(Box::new(crate::terminal::CopyLastOutput)),
            true,
            true,
        ),
        e(
            "Copy Error",
            CopyError,
            Some(Box::new(crate::terminal::CopyError)),
            true,
            false,
        ),
        e(
            "Clear",
            Clear,
            Some(Box::new(crate::terminal::Clear)),
            true,
            false,
        ),
        e(
            "Export Output…",
            Export,
            Some(Box::new(ExportOutput)),
            true,
            false,
        ),
        e(
            "Full Screen",
            FullScreen,
            Some(Box::new(crate::workspace::ToggleMaximizePane)),
            true,
            true,
        ),
    ]
}

impl TerminalPanel {
    /// Copies the last output (or its errors) of the tab at `ix`'s focused pane (item 1).
    fn copy_output(&mut self, ix: usize, errors: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(ix) {
            tab.update(cx, |t, cx| {
                let pane = t.state.focused;
                t.copy_output(pane, errors, cx)
            });
        }
    }

    /// The menu's state for the tab at `ix`.
    pub fn menu_state(&self, ix: usize, cx: &App) -> MenuState {
        let t = self.tabs[ix].read(cx);
        MenuState {
            first: ix == 0,
            last: ix + 1 == self.tabs.len(),
            locked: t.state.locked,
            pinned: t.state.pinned,
            busy: !self.running_in_tab(ix, cx).is_empty(),
            others: self.tabs.len() > 1,
            grouped: t.group.is_some(),
            other_projects: self.projects.iter().any(|p| Some(p) != t.group.as_ref()),
        }
    }

    /// Does what `act` says to the tab at `ix`, for the user.
    pub(super) fn run_tab_act(
        &mut self,
        ix: usize,
        act: TabAct,
        had_keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return;
        };
        let pane = tab.read(cx).state.focused;
        match act {
            TabAct::Rename => self.rename_tab(ix, window, cx),
            TabAct::Duplicate => self.duplicate_tab(ix, window, cx),
            TabAct::MoveToProject => {}
            TabAct::RemoveFromGroup => self.move_to_group(&tab.downgrade(), None, cx),
            TabAct::Close => self.close_for_user(&tab.downgrade(), had_keyboard, window, cx),
            TabAct::CloseMany(scope) => self.close_tabs_for_user(scope, ix, window, cx),
            TabAct::Lock => {
                self.set_locked(ix, None, cx);
            }
            TabAct::Pin => {
                self.set_pinned(ix, None, cx);
            }
            TabAct::CopyLastOutput => self.copy_output(ix, false, cx),
            TabAct::CopyError => self.copy_output(ix, true, cx),
            TabAct::Clear => tab.update(cx, |t, cx| t.clear_pane(pane, cx)),
            TabAct::Export => tab.update(cx, |t, cx| t.on_focused(cx, |v, cx| v.export_output(cx))),
            TabAct::FullScreen => {
                self.activate(ix, window, cx);
                window.dispatch_action(Box::new(crate::workspace::ToggleMaximizePane), cx);
            }
        }
    }

    /// A new shell in the tab at `ix`'s folder (its shell's, else its project's), in its group.
    pub(super) fn duplicate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        let t = tab.read(cx);
        let dir = t.focused_view().read(cx).cwd().or_else(|| t.group.clone());
        match dir {
            Some(dir) => self.open_shell_in(dir, window, cx),
            None => self.open_shell(Some(window), cx),
        }
    }
}

/// Builds the tab's menu. `had_keyboard`: the tab had the keyboard before the menu took it.
pub(super) fn build(
    m: &MenuFor,
    state: MenuState,
    had_keyboard: bool,
    mut menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if let Some(pane) = &*m.keys_in {
        menu = menu.action_context(pane.clone());
    }
    for entry in tab_menu(&state) {
        if entry.after_separator {
            menu = menu.separator();
        }
        if entry.act == TabAct::MoveToProject {
            menu = move_to_project(m, menu, window, cx);
            continue;
        }
        let (panel, tab, act) = (m.panel.clone(), m.tab.clone(), entry.act);
        // The keys show as the keymap gives them in a pane on screen (the action context).
        let mut item = PopupMenuItem::new(entry.label);
        if let Some(action) = entry.action {
            item = item.action(action);
        }
        let item = item
            .disabled(!entry.enabled)
            .on_click(move |_, window, cx| {
                let _ = panel.update(cx, |p, cx| {
                    if let Some(ix) = p.index_of(&tab) {
                        p.run_tab_act(ix, act, had_keyboard, window, cx)
                    }
                });
            });
        menu = menu.item(item);
    }
    menu
}

/// The most of a project's name a menu shows; a longer one ends in an ellipsis.
const NAME_MAX: usize = 40;

/// `name`, cut to [`NAME_MAX`] characters with an ellipsis.
fn short(name: &str) -> String {
    if name.chars().count() <= NAME_MAX {
        return name.to_string();
    }
    let cut: String = name.chars().take(NAME_MAX - 1).collect();
    format!("{cut}…")
}

/// Move to Project ▸, its projects' names cut short with an ellipsis; with no other project, a
/// disabled item saying so.
fn move_to_project(
    m: &MenuFor,
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
    let m = m.clone();
    menu.submenu("Move to Project", window, cx, move |mut menu, _, _| {
        if others.is_empty() {
            return menu.item(PopupMenuItem::new("No Other Project Open").disabled(true));
        }
        for root in &others {
            let (panel, tab, to) = (m.panel.clone(), m.tab.clone(), root.clone());
            let item = PopupMenuItem::new(short(&project_name(root))).on_click(move |_, _, cx| {
                let _ = panel.update(cx, |p, cx| p.move_to_group(&tab, Some(to.clone()), cx));
            });
            menu = menu.item(item);
        }
        menu
    })
}

/// The tab menu's keys (and the menu bar's Terminal menu) act on the active tab.
pub(super) fn on_actions(el: Div, cx: &mut Context<TerminalPanel>) -> Div {
    fn act<A: Action>(el: Div, act: TabAct, cx: &mut Context<TerminalPanel>) -> Div {
        el.on_action(cx.listener(move |this, _: &A, window, cx| {
            let ix = this.active;
            let had_keyboard = this.has_focus(window, cx);
            this.run_tab_act(ix, act, had_keyboard, window, cx)
        }))
    }
    let el = act::<CloseTab>(el, TabAct::Close, cx);
    let el = act::<CloseOtherTabs>(el, TabAct::CloseMany(CloseScope::Others), cx);
    let el = act::<CloseTabsLeft>(el, TabAct::CloseMany(CloseScope::Left), cx);
    let el = act::<CloseTabsRight>(el, TabAct::CloseMany(CloseScope::Right), cx);
    let el = act::<SafeCloseAll>(el, TabAct::CloseMany(CloseScope::Idle), cx);
    let el = act::<ForceCloseAll>(el, TabAct::CloseMany(CloseScope::All), cx);
    let el = act::<DuplicateTab>(el, TabAct::Duplicate, cx);
    let el = act::<ToggleLockTab>(el, TabAct::Lock, cx);
    act::<TogglePinTab>(el, TabAct::Pin, cx)
}
