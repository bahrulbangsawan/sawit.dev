//! Tabs of terminals in the bottom dock, with SSH sessions that
//! reconnect with one click. Tabs are grouped by the
//! window's project they started in.

mod control;
mod groups;
mod jobs;
mod open;
mod render;
mod running;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, EntityId, EventEmitter, ScrollHandle, SharedString,
    WeakEntity, Window,
};
use sawit_core::ssh::Launch;
use sawit_core::terminal::{AgentKind, Command, PaneId};

use super::tab::{TabEvent, TerminalTab};
use super::view::TerminalView;
use open::shell_name;

pub struct TerminalPanel {
    tabs: Vec<Entity<TerminalTab>>,
    active: usize,
    /// The active project's folder: where new terminals start.
    cwd: Option<PathBuf>,
    /// Every project open in the window, in switcher order (shared with the tab menus, which
    /// move a tab to another).
    projects: Rc<[PathBuf]>,
    /// Groups whose tabs are folded away.
    collapsed: HashSet<PathBuf>,
    /// The tab strip's scroll, which brings the active tab into view.
    tab_scroll: ScrollHandle,
    /// The active tab last scrolled into view, and its index: another tab, or the same one moved,
    /// is scrolled to; a fold or a scroll by hand leaves the strip alone.
    scrolled_to: Option<(EntityId, usize)>,
    error: Option<String>,
    /// Whether the panel fills the window; kept in sync by the workspace.
    pub maximized: bool,
    /// The window the panel last drew in, for acting on a tab's requests.
    window: Option<AnyWindowHandle>,
    /// How to reach an SSH server as its settings are now, by its key (the
    /// workspace's saved servers). Without it a session reconnects as it
    /// first connected.
    remotes: Option<RemoteSource>,
    /// New terminals fail to start, as when the shell can't be run (for
    /// tests: under a multiplexer every spawn goes through `env`, which
    /// always starts).
    #[cfg(test)]
    fail_spawns: bool,
}

/// Asks the workspace to fill the window with the panel, or to restore.
pub struct ToggleMaximize;

/// How to connect to the SSH server with a key, as its settings are now.
type RemoteSource = Rc<dyn Fn(&str, &mut App) -> Result<Remote, String>>;

/// An SSH session's server.
#[derive(Clone)]
pub struct Remote {
    /// Identity of the server, shared by all its sessions.
    pub key: String,
    pub name: String,
    pub launch: Launch,
}

/// A terminal started from the Run panel: the script it runs, by key, and
/// the name its tab shows (the shell's own title says little).
#[derive(Clone, PartialEq, Eq)]
pub struct JobTag {
    pub key: String,
    pub name: String,
}

/// Who asked for a terminal to be used, which decides what it may do to
/// the dock and the keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// The user, with a click or a key: the terminal shows and takes the
    /// keyboard (Homebrew's install waits there for a password).
    User,
    /// An agent, over the control socket. The terminal shows only when the
    /// user isn't typing in another terminal, and the keyboard stays where
    /// it is: what the user types next mustn't land in a shell.
    Agent,
}

/// A coding agent running in a pane ([`TerminalPanel::running_agents`]). The tab strip reads
/// which agents run where; the browser's Send to agent also reads what the user sees each one
/// called and which one the user types to.
#[derive(Clone)]
pub struct RunningAgent {
    pub tab: WeakEntity<TerminalTab>,
    pub pane: PaneId,
    pub kind: AgentKind,
    /// What the user sees the agent's terminal called: the pane's header in a split tab, else
    /// the tab's label.
    pub label: SharedString,
    /// The pane has the keyboard, or had it last of all the panes: where the user types.
    pub focused: bool,
}

/// What a script's terminal is doing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JobStatus {
    /// Its command, by process group, while the shell runs one.
    pub running: Option<u32>,
}

impl EventEmitter<ToggleMaximize> for TerminalPanel {}

impl TerminalPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut panel = Self {
            tabs: vec![],
            active: 0,
            cwd: None,
            projects: Rc::new([]),
            collapsed: HashSet::new(),
            tab_scroll: ScrollHandle::new(),
            scrolled_to: None,
            error: None,
            maximized: false,
            window: None,
            remotes: None,
            #[cfg(test)]
            fail_spawns: false,
        };
        panel.open(Command::default(), &shell_name(), None, None, cx);
        panel
    }

    /// Where `tab` is in the strip, while it is open. The strip's buttons and menus look
    /// their tab up when used, since tabs open and close under them (an agent's, a script's).
    fn index_of(&self, tab: &WeakEntity<TerminalTab>) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.entity_id() == tab.entity_id())
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = ix;
        if let Some(g) = self.tabs.get(ix).and_then(|t| t.read(cx).group.clone()) {
            self.collapsed.remove(&g);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Shows the tab at `ix` and puts a field for its name in its label.
    fn rename_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(ix, window, cx);
        if let Some(tab) = self.tabs.get(ix) {
            tab.update(cx, |t, cx| t.rename_tab(window, cx));
        }
    }

    /// Every pane of every tab: the tab's index, the pane and its terminal,
    /// tabs in strip order and panes in layout order.
    fn all_panes<'a>(
        &'a self,
        cx: &'a App,
    ) -> impl Iterator<Item = (usize, PaneId, &'a Entity<TerminalView>)> + 'a {
        self.tabs.iter().enumerate().flat_map(move |(ix, tab)| {
            tab.read(cx)
                .views()
                .map(move |(pane, view)| (ix, pane, view))
        })
    }

    /// Makes the tab at `ix` the active one with `pane` focused.
    fn show_pane(&mut self, ix: usize, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        self.active = ix;
        self.tabs[ix].update(cx, |t, _| t.state.focus(pane));
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Shows `pane` of the tab at `ix`, where something was just started
    /// or typed, as `origin` allows: for the user, it shows and takes the
    /// keyboard. For an agent it shows, without the keyboard, unless the
    /// user is typing in a terminal; then nothing changes on screen.
    fn reveal(
        &mut self,
        ix: usize,
        pane: PaneId,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match origin {
            Origin::User => self.show_pane(ix, pane, window, cx),
            Origin::Agent if self.has_focus(window, cx) => cx.notify(),
            Origin::Agent => {
                self.active = ix;
                self.tabs[ix].update(cx, |t, _| t.state.focus(pane));
                cx.notify();
            }
        }
    }

    /// The position of `pane` in the tab at `ix`, as `app_state` lists it.
    pub fn pane_index(&self, ix: usize, pane: PaneId, cx: &App) -> Option<usize> {
        self.tabs
            .get(ix)?
            .read(cx)
            .views()
            .position(|(id, _)| id == pane)
    }

    /// The index of the active tab, the one the dock shows (for tests).
    #[cfg(test)]
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Closes the tab at `ix`. The active tab stays the active one; when it is the one that
    /// closes, the tab that takes its place is (or the new last one).
    fn close(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.tabs.remove(ix);
            if ix < self.active {
                self.active -= 1;
            }
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
            cx.notify();
        }
    }

    /// Closes `tab` for the user (the strip's ×, the tab's menu). When the keyboard was in it
    /// (`had_keyboard`), the tab shown next takes it; otherwise it stays where it is.
    fn close_for_user(
        &mut self,
        tab: &WeakEntity<TerminalTab>,
        had_keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.index_of(tab) else {
            return;
        };
        self.close(ix, cx);
        if had_keyboard {
            self.focus_active(window, cx);
        }
    }

    /// Drops a tab whose last pane closed (handing the keyboard to the next
    /// tab if it was there), redraws the strip on changes, and does what a
    /// pane's banner asks.
    fn on_tab_event(&mut self, tab: Entity<TerminalTab>, event: &TabEvent, cx: &mut Context<Self>) {
        match event {
            TabEvent::Closed { refocus } => {
                if let Some(ix) = self.tabs.iter().position(|t| *t == tab) {
                    self.close(ix, cx);
                    if *refocus {
                        self.in_window(cx, |this, window, cx| this.focus_active(window, cx));
                    }
                }
            }
            TabEvent::Changed => cx.notify(),
            TabEvent::Reconnect(pane) => {
                let pane = *pane;
                self.in_window(cx, move |this, window, cx| {
                    if let Some(ix) = this.tabs.iter().position(|t| *t == tab) {
                        this.reconnect(ix, pane, window, cx);
                    }
                });
            }
            TabEvent::NewTerminal => {
                self.in_window(cx, |this, window, cx| this.open_shell(Some(window), cx));
            }
        }
    }

    /// Runs `f` on the panel with the window it last drew in, once that can
    /// be reached (a tab's events come without it).
    fn in_window(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(handle) = self.window else {
            return;
        };
        let panel = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                panel.update(cx, |p, cx| f(p, window, cx))
            });
        });
    }

    /// How many tabs are open (for tests).
    #[cfg(test)]
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// The tab at `ix` (for tests). Panics when there is none.
    #[cfg(test)]
    pub fn tab(&self, ix: usize) -> &Entity<TerminalTab> {
        &self.tabs[ix]
    }

    /// A terminal tab has the keyboard.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.tabs
            .iter()
            .any(|tab| tab.read(cx).has_focus(window, cx))
    }

    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.update(cx, |t, cx| t.focus(window, cx));
        }
    }
}
