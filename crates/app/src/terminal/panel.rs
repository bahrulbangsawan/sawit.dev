//! Tabs of terminals in the bottom dock, with SSH sessions that
//! reconnect with one click. Tabs are grouped by the
//! window's project they started in.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::ssh::Launch;
use sawit_core::terminal::{
    AgentKind, Command, GridSize, MIN_COLS, MIN_ROWS, PaneId, Rect, Session, SplitDir,
    default_shell, run_bytes, shell_quote,
};

use super::agents;
use super::tab::{TabEvent, TerminalTab, Unsplit};
use super::view::TerminalView;
use super::{RenameTab, SplitDown, SplitRight};
use crate::states::{Banner, Placeholder};
use crate::tab_groups::{group_color, group_header, group_stripe, project_name};
use crate::tabs::{Drawn, Slot, insert_index, project_of, strip};

pub struct TerminalPanel {
    tabs: Vec<Entity<TerminalTab>>,
    active: usize,
    /// The active project's folder: where new terminals start.
    cwd: Option<PathBuf>,
    /// Every project open in the window, in switcher order.
    projects: Vec<PathBuf>,
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
#[allow(dead_code)]
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
            projects: Vec::new(),
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

    /// The projects open in the window and the active one, whose folder
    /// new terminals start in and whose group they join. When the first
    /// project opens, shells idle at their prompt follow it: untouched ones
    /// are reopened there (their prompt may not be drawn yet), used ones
    /// `cd`. Busy ones are left alone, and so are all shells on later
    /// switches. Tabs of a closed project stay open, out of any group.
    pub fn set_projects(
        &mut self,
        projects: Vec<PathBuf>,
        active: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let first = self.cwd.is_none() && active.is_some();
        self.projects = projects;
        self.cwd = active;
        self.collapsed.retain(|r| self.projects.contains(r));
        for tab in &self.tabs {
            tab.update(cx, |t, _| {
                if t.group.as_ref().is_some_and(|g| !self.projects.contains(g)) {
                    t.group = None;
                }
            });
        }
        if let Some(dir) = self.cwd.clone().filter(|_| first) {
            let idle: Vec<(usize, PaneId, bool)> = self
                .all_panes(cx)
                .filter(|(_, _, v)| v.read(cx).idle_shell())
                .map(|(ix, pane, v)| (ix, pane, v.read(cx).touched))
                .collect();
            for (ix, pane, touched) in idle {
                let tab = self.tabs[ix].clone();
                if touched {
                    if let Some(view) = tab.read(cx).view(pane) {
                        view.read(cx).cd(&dir);
                    }
                    tab.update(cx, |t, _| t.group = Some(dir.clone()));
                } else if let Some(fresh) = self.spawn_view(
                    Command {
                        cwd: Some(dir.clone()),
                        ..Command::default()
                    },
                    &shell_name(),
                    None,
                    cx,
                ) {
                    // Replacing a view focuses its pane; give the focus back.
                    tab.update(cx, |t, cx| {
                        let focused = t.state.focused;
                        t.replace_view(pane, fresh, cx);
                        t.state.focus(focused);
                        t.group = Some(dir.clone());
                    });
                }
            }
        }
        self.regroup(cx);
        cx.notify();
    }

    /// Where a tab of `group` goes: at the end of that group's tabs.
    fn insert_at(&self, group: Option<&Path>, cx: &App) -> usize {
        insert_index(&slots(&self.groups(cx)), false, group)
    }

    /// The group of each tab, in strip order.
    fn groups(&self, cx: &App) -> Vec<Option<PathBuf>> {
        self.tabs.iter().map(|t| t.read(cx).group.clone()).collect()
    }

    /// The colour of the group of project `root`.
    fn group_color(&self, root: &Path, cx: &App) -> Hsla {
        let at = self.projects.iter().position(|p| p == root).unwrap_or(0);
        group_color(at, cx)
    }

    /// Adds a tab at its group's end; returns its index.
    fn insert(&mut self, tab: Entity<TerminalTab>, cx: &App) -> usize {
        let group = tab.read(cx).group.clone();
        let at = self.insert_at(group.as_deref(), cx);
        self.tabs.insert(at, tab);
        at
    }

    /// Adds a tab at its group's end without changing which tab is active;
    /// returns its index.
    fn add_tab(&mut self, tab: Entity<TerminalTab>, cx: &App) -> usize {
        let at = self.insert(tab, cx);
        if self.tabs.len() > 1 && at <= self.active {
            self.active += 1;
        }
        at
    }

    /// Puts every tab back in group order, keeping the active one.
    fn regroup(&mut self, cx: &App) {
        let active = self.tabs.get(self.active).cloned();
        for tab in std::mem::take(&mut self.tabs) {
            self.insert(tab, cx);
        }
        self.active = active
            .and_then(|a| self.tabs.iter().position(|t| *t == a))
            .unwrap_or(0);
    }

    /// Where `tab` is in the strip, while it is open. The strip's buttons and menus look
    /// their tab up when used, since tabs open and close under them (an agent's, a script's).
    fn index_of(&self, tab: &WeakEntity<TerminalTab>) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.entity_id() == tab.entity_id())
    }

    /// Moves `tab` into `group` (or out of any).
    fn move_to_group(
        &mut self,
        tab: &WeakEntity<TerminalTab>,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = tab.upgrade() {
            tab.update(cx, |t, _| t.group = group);
            self.regroup(cx);
            cx.notify();
        }
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

    pub fn open_shell(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.open(Command::default(), &shell_name(), None, window, cx);
    }

    /// A new shell tab started in `dir`.
    pub fn open_shell_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let cmd = Command {
            cwd: Some(dir),
            ..Command::default()
        };
        self.open(cmd, &shell_name(), None, Some(window), cx);
    }

    /// Opens a new session to `remote` in its own tab, shown as `origin`
    /// allows; returns where, or why it couldn't start.
    pub fn open_ssh(
        &mut self,
        remote: Remote,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        sawit_core::ssh::ensure_ssh_dir();
        let name = remote.name.clone();
        let (ix, pane) = self.open_tab(ssh_command(&remote.launch), &name, Some(remote), cx)?;
        self.reveal(ix, pane, origin, window, cx);
        Ok((ix, pane))
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

    /// Live panes in sessions to the server with `key`.
    fn ssh_panes(&self, key: &str, cx: &App) -> Vec<(usize, PaneId)> {
        self.all_panes(cx)
            .filter(|(_, _, v)| {
                let v = v.read(cx);
                !v.exited && v.remote.as_ref().is_some_and(|r| r.key == key)
            })
            .map(|(ix, pane, _)| (ix, pane))
            .collect()
    }

    /// Live sessions to the server with `key`, in any pane.
    pub fn ssh_sessions(&self, key: &str, cx: &App) -> usize {
        self.ssh_panes(key, cx).len()
    }

    /// Shows the newest live session to `key`, in whichever pane it is, as
    /// `origin` allows; `None` when there is none.
    pub fn focus_ssh(
        &mut self,
        key: &str,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(usize, PaneId)> {
        let (ix, pane) = self.ssh_panes(key, cx).last().copied()?;
        self.reveal(ix, pane, origin, window, cx);
        Some((ix, pane))
    }

    /// Where SSH sessions get the server's settings when they reconnect.
    pub fn set_remote_source(
        &mut self,
        source: impl Fn(&str, &mut App) -> Result<Remote, String> + 'static,
    ) {
        self.remotes = Some(Rc::new(source));
    }

    /// Connects the ended SSH session in `pane` of the tab at `ix` again,
    /// in place, and focuses it, with the server's settings as they are now
    /// (fixed since, or with a password saved since). The tab's other panes
    /// are left alone, and so is a session that has not ended (a second
    /// click on Reconnect).
    pub(super) fn reconnect(
        &mut self,
        ix: usize,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return;
        };
        let Some(old) = tab
            .read(cx)
            .view(pane)
            .map(|v| v.read(cx))
            .filter(|v| v.exited)
            .and_then(|v| v.remote.clone())
        else {
            return;
        };
        let remote = match self.remotes.clone() {
            Some(source) => match source(&old.key, cx) {
                Ok(remote) => remote,
                Err(e) => {
                    self.error = Some(format!("Unable to reconnect to {}. {e}", old.name));
                    cx.notify();
                    return;
                }
            },
            None => old,
        };
        let name = remote.name.clone();
        if let Some(view) = self.spawn_view(ssh_command(&remote.launch), &name, Some(remote), cx) {
            tab.update(cx, |t, cx| t.replace_view(pane, view, cx));
            self.active = ix;
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Types `commands` into a shell and presses Enter: the active tab's
    /// focused pane if its shell is at its prompt, else the newest pane in
    /// any tab whose shell is, else a new shell tab. A shell running a
    /// program (an agent, an editor, `cat`) is left alone: the program would
    /// get the command line and the Enter. So is a Run panel script's shell,
    /// even between runs, and, for an agent, the shell the user is typing
    /// in: the command would join the line they haven't sent yet, or run in
    /// front of them and take their next keys. The pane shows as `origin`
    /// allows. Returns the tab and pane it typed into, or why no shell could
    /// start.
    pub fn run(
        &mut self,
        commands: &str,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        let typing = match origin {
            Origin::Agent => self.typing_pane(window, cx),
            Origin::User => None,
        };
        let usable = |ix: usize, pane: PaneId, v: &Entity<TerminalView>| {
            v.read(cx).idle_shell() && typing != Some((ix, pane))
        };
        let in_active = self.tabs.get(self.active).and_then(|t| {
            let t = t.read(cx);
            let pane = t.state.focused;
            usable(self.active, pane, t.focused_view()).then_some((self.active, pane))
        });
        let target = in_active.or_else(|| {
            self.all_panes(cx)
                .filter(|&(ix, pane, v)| usable(ix, pane, v))
                .map(|(ix, pane, _)| (ix, pane))
                .last()
        });
        let (ix, pane) = match target {
            Some(found) => found,
            None => self.open_tab(Command::default(), &shell_name(), None, cx)?,
        };
        if let Some(view) = self.tabs[ix].read(cx).view(pane).cloned() {
            view.update(cx, |view, _| {
                view.touched = true;
                view.write(run_bytes(commands));
            });
        }
        self.reveal(ix, pane, origin, window, cx);
        Ok((ix, pane))
    }

    /// The pane the user is typing in: the one with the keyboard, while a
    /// terminal has it.
    fn typing_pane(&self, window: &Window, cx: &App) -> Option<(usize, PaneId)> {
        self.all_panes(cx)
            .find(|(_, _, v)| v.focus_handle(cx).contains_focused(window, cx))
            .map(|(ix, pane, _)| (ix, pane))
    }

    /// Every tab, for `sawit-mcp`: the tab's own fields describe its focused
    /// pane, and `panes` lists each pane in layout order. `agent` is the
    /// coding agent running in a pane (`claude-code`), and a tab's `agents`
    /// every distinct one of its panes, as its logos in the strip show them.
    pub fn describe(&self, cx: &App) -> Vec<serde_json::Value> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let t = tab.read(cx);
                let v = t.focused_view().read(cx);
                let panes: Vec<_> = t
                    .views()
                    .enumerate()
                    .map(|(n, (pane, view))| {
                        let v = view.read(cx);
                        serde_json::json!({
                            "index": n,
                            "title": t.pane_title(pane, v).to_string(),
                            "focused": pane == t.state.focused,
                            "exited": v.exited,
                            "at_prompt": v.at_prompt(),
                            "running_job": v.foreground_job().is_some(),
                            "agent": v.agent().map(|a| a.id()),
                            "script": v.job.as_ref().map(|j| j.name.clone()),
                            "ssh": v.remote.as_ref().map(|r| r.name.clone()),
                        })
                    })
                    .collect();
                let tab_agents: Vec<&str> =
                    agents::distinct(t.agents(cx).into_iter().map(|(_, kind)| kind))
                        .into_iter()
                        .map(|kind| kind.id())
                        .collect();
                serde_json::json!({
                    "index": ix,
                    "title": t.title(cx).to_string(),
                    "active": ix == self.active,
                    "exited": v.exited,
                    "shell": v.shell,
                    "at_prompt": v.at_prompt(),
                    "running_job": v.foreground_job().is_some(),
                    "agent": v.agent().map(|a| a.id()),
                    "agents": tab_agents,
                    "script": v.job.as_ref().map(|j| j.name.clone()),
                    "ssh": v.remote.as_ref().map(|r| r.name.clone()),
                    "project": t.group,
                    "panes": panes,
                })
            })
            .collect()
    }

    fn tab_ix(&self, tab: Option<usize>) -> Result<usize, String> {
        let ix = tab.unwrap_or(self.active);
        if ix < self.tabs.len() {
            Ok(ix)
        } else if self.tabs.is_empty() {
            Err("No terminals are open.".into())
        } else {
            Err(format!(
                "There is no terminal {ix}; tabs are 0 to {}.",
                self.tabs.len() - 1
            ))
        }
    }

    /// The pane at position `pane` of the tab at `ix` (the tab's focused
    /// pane by default).
    fn pane_of(
        &self,
        ix: usize,
        pane: Option<usize>,
        cx: &App,
    ) -> Result<(PaneId, Entity<TerminalView>), String> {
        let tab = self.tabs[ix].read(cx);
        let Some(n) = pane else {
            return Ok((tab.state.focused, tab.focused_view().clone()));
        };
        let panes: Vec<_> = tab.views().collect();
        match panes.get(n) {
            Some((id, view)) => Ok((*id, (*view).clone())),
            None => Err(format!(
                "Terminal {ix} has no pane {n}; panes are 0 to {}.",
                panes.len() - 1
            )),
        }
    }

    /// The pane an agent reads or types into: the one at position `pane`
    /// of the tab at `tab`. With no tab, the active tab's, by default its
    /// focused pane: the one the user has in front. A named tab that is
    /// split needs its pane named too, as its focused pane may be the one
    /// the user types in while the agent's command runs in another.
    fn target(
        &self,
        tab: Option<usize>,
        pane: Option<usize>,
        cx: &App,
    ) -> Result<(usize, PaneId, Entity<TerminalView>), String> {
        let ix = self.tab_ix(tab)?;
        let panes = self.tabs[ix].read(cx).views().count();
        if tab.is_some() && pane.is_none() && panes > 1 {
            return Err(format!(
                "Terminal {ix} is split into {panes} panes: name the pane, 0 to {} (app_state \
                 lists them).",
                panes - 1
            ));
        }
        let (id, view) = self.pane_of(ix, pane, cx)?;
        Ok((ix, id, view))
    }

    /// The pane's last `lines` lines of output (see [`Self::target`]).
    pub fn text(
        &self,
        tab: Option<usize>,
        pane: Option<usize>,
        lines: usize,
        cx: &App,
    ) -> Result<String, String> {
        let (_, _, view) = self.target(tab, pane, cx)?;
        Ok(view.read(cx).text_tail(lines))
    }

    /// Sends raw input to a pane (see [`Self::target`]). The pane counts
    /// as used from then on, as when the user types there.
    pub fn input(
        &mut self,
        tab: Option<usize>,
        pane: Option<usize>,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let (ix, _, view) = self.target(tab, pane, cx)?;
        if view.read(cx).exited {
            return Err(format!("Terminal {ix} has exited."));
        }
        view.update(cx, |view, _| {
            view.touched = true;
            view.write(bytes);
        });
        cx.notify();
        Ok(ix)
    }

    /// Why an agent may not close the tab at `ix`, or only its pane at position `pane`, without
    /// saying it is sure (`terminal_close`'s `confirm`): closing would end what runs there (a
    /// command in the foreground or a live SSH session, as [`Self::running`] names them), or the
    /// user is typing in it. `None` when it would end only shells at their prompt, nobody types
    /// in, as quitting ends them without asking.
    pub fn close_refusal(
        &self,
        ix: usize,
        pane: Option<usize>,
        window: &Window,
        cx: &App,
    ) -> Result<Option<String>, String> {
        let ix = self.tab_ix(Some(ix))?;
        let panes: Vec<PaneId> = match pane {
            None => self.tabs[ix].read(cx).views().map(|(id, _)| id).collect(),
            Some(_) => vec![self.pane_of(ix, pane, cx)?.0],
        };
        let tab = self.tabs[ix].read(cx);
        let ends: Vec<String> = panes
            .iter()
            .filter_map(|&id| running_in(tab, id, tab.view(id)?.read(cx)))
            .collect();
        let typing = self
            .typing_pane(window, cx)
            .is_some_and(|(at, id)| at == ix && panes.contains(&id));
        let what = match pane {
            Some(n) => format!("Terminal {ix}, pane {n}"),
            None => format!("Terminal {ix}"),
        };
        let ending = format!("closing it ends what runs there ({})", ends.join(", "));
        let why = match (ends.is_empty(), typing) {
            (true, false) => return Ok(None),
            (false, false) => ending,
            (true, true) => "the user is typing in it".to_string(),
            (false, true) => format!("{ending}, and the user is typing in it"),
        };
        Ok(Some(format!(
            "{what} wasn't closed: {why}. Pass confirm: true to close it anyway."
        )))
    }

    /// Closes the tab at `ix`, or only its pane at position `pane`. Closing
    /// a pane moves the keyboard only if it was in that tab.
    pub fn close_tab(
        &mut self,
        ix: usize,
        pane: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.tab_ix(Some(ix))?;
        match pane {
            None => self.close(ix, cx),
            Some(_) => {
                let (id, _) = self.pane_of(ix, pane, cx)?;
                self.tabs[ix].update(cx, |t, cx| {
                    let refocus = t.has_focus(window, cx);
                    t.remove_pane(id, refocus, window, cx)
                });
            }
        }
        Ok(())
    }

    /// Runs `commands` in a new shell tab started in `dir`, shown as
    /// `origin` allows; returns the tab and its pane.
    pub fn run_in_new(
        &mut self,
        dir: Option<PathBuf>,
        commands: &str,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        let cmd = Command {
            cwd: dir,
            ..Command::default()
        };
        let (ix, pane) = self.open_tab(cmd, &shell_name(), None, cx)?;
        if let Some(view) = self.tabs[ix].read(cx).view(pane).cloned() {
            view.update(cx, |view, _| {
                view.touched = true;
                view.write(run_bytes(commands));
            });
        }
        self.reveal(ix, pane, origin, window, cx);
        Ok((ix, pane))
    }

    /// The index of the active tab, the one the dock shows (for tests).
    #[cfg(test)]
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Live panes running the script with `key`, oldest first, as the
    /// tab's index and the pane.
    fn job_panes(&self, key: &str, cx: &App) -> Vec<(usize, PaneId)> {
        self.all_panes(cx)
            .filter(|(_, _, v)| {
                let v = v.read(cx);
                !v.exited && v.job.as_ref().is_some_and(|j| j.key == key)
            })
            .map(|(ix, pane, _)| (ix, pane))
            .collect()
    }

    /// The newest live terminal of the script with `key`, if one is open.
    pub fn job_status(&self, key: &str, cx: &App) -> Option<JobStatus> {
        let (ix, pane) = *self.job_panes(key, cx).last()?;
        let view = self.tabs[ix].read(cx).view(pane)?;
        Some(JobStatus {
            running: view.read(cx).foreground_job(),
        })
    }

    /// Where the script with `key` runs, its newest live terminal, as
    /// `app_state` lists it: the tab's index and the pane's place in it.
    pub fn job_terminal(&self, key: &str, cx: &App) -> Option<(usize, usize)> {
        let (ix, pane) = *self.job_panes(key, cx).last()?;
        Some((ix, self.pane_index(ix, pane, cx)?))
    }

    /// Types `command` into the script's terminal: its idle pane when there
    /// is one (a running one is only shown), else a new shell in `dir`. The
    /// old shell may have moved since, and the command is relative to the
    /// script's folder, so it goes back to `dir` first. With `show`, the
    /// terminal shows as that origin allows (one of several started together
    /// passes `None`). Returns the tab and pane, or why no shell could start
    /// (the dock shows it too).
    pub fn start_job(
        &mut self,
        tag: JobTag,
        dir: PathBuf,
        command: &str,
        show: Option<Origin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        let (ix, pane, command) = match self.job_panes(&tag.key, cx).last().copied() {
            Some((ix, pane)) => {
                let cwd = self.tabs[ix]
                    .read(cx)
                    .view(pane)
                    .and_then(|v| v.read(cx).cwd());
                (ix, pane, command_in(&dir, command, cwd))
            }
            None => {
                let cmd = Command {
                    cwd: Some(dir),
                    ..Command::default()
                };
                let (ix, pane) = match self.open_tab(cmd, &tag.name, None, cx) {
                    Ok(opened) => opened,
                    Err(e) => {
                        cx.notify();
                        return Err(e);
                    }
                };
                (ix, pane, command.to_string())
            }
        };
        if let Some(view) = self.tabs[ix].read(cx).view(pane).cloned() {
            view.update(cx, |view, _| {
                view.job = Some(tag);
                if view.foreground_job().is_none() {
                    view.touched = true;
                    view.write(run_bytes(&command));
                }
            });
        }
        if let Some(origin) = show {
            self.reveal(ix, pane, origin, window, cx);
        }
        cx.notify();
        Ok((ix, pane))
    }

    /// Interrupts (Ctrl-C) whatever the script's terminals are running.
    pub fn stop_job(&mut self, key: &str, cx: &mut Context<Self>) {
        for (ix, pane) in self.job_panes(key, cx) {
            if let Some(view) = self.tabs[ix].read(cx).view(pane).map(|v| v.read(cx))
                && view.foreground_job().is_some()
            {
                view.write(vec![0x03]);
            }
        }
        cx.notify();
    }

    /// Shows the newest terminal of the script, in whichever pane it is;
    /// false when there is none.
    pub fn show_job(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((ix, pane)) = self.job_panes(key, cx).last().copied() else {
            return false;
        };
        self.show_pane(ix, pane, window, cx);
        true
    }

    /// Starts `cmd` in a new tab and makes it the active one, giving it the
    /// keyboard when there is a `window`.
    fn open(
        &mut self,
        cmd: Command,
        title: &str,
        remote: Option<Remote>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Ok((ix, _)) = self.open_tab(cmd, title, remote, cx) {
            self.active = ix;
            if let Some(window) = window {
                self.focus_active(window, cx);
            }
        }
        cx.notify();
    }

    /// Starts `cmd` in a new tab, leaving the active tab as it is; returns
    /// where, or why it couldn't start (also shown in the panel).
    fn open_tab(
        &mut self,
        cmd: Command,
        title: &str,
        remote: Option<Remote>,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        let Some(tab) = self.spawn(cmd, title, remote, cx) else {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "Unable to open a terminal.".into()));
        };
        let pane = tab.read(cx).state.focused;
        Ok((self.add_tab(tab, cx), pane))
    }

    /// Starts `cmd` in the panel's directory (else `$HOME`) in a new tab,
    /// without adding it to the strip.
    fn spawn(
        &mut self,
        mut cmd: Command,
        title: &str,
        remote: Option<Remote>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalTab>> {
        cmd.cwd = cmd.cwd.take().or_else(|| self.cwd.clone()).or_else(home);
        let group = match &remote {
            Some(_) => None,
            None => cmd
                .cwd
                .as_deref()
                .and_then(|d| project_of(&self.projects, d)),
        };
        let view = self.spawn_view(cmd, title, remote, cx)?;
        let tab = cx.new(|cx| TerminalTab::new(view, group, cx));
        cx.subscribe(&tab, Self::on_tab_event).detach();
        Some(tab)
    }

    /// Starts `cmd` in a terminal of its own. On failure the error shows
    /// in the panel and there is no terminal.
    fn spawn_view(
        &mut self,
        cmd: Command,
        title: &str,
        remote: Option<Remote>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        let size = GridSize {
            cols: 80,
            rows: 24,
            cell_width: 8,
            cell_height: 17,
        };
        let shell = cmd.program.is_empty();
        #[cfg(test)]
        if self.fail_spawns {
            self.error = Some("Failed to spawn command: the shell can't be run.".into());
            return None;
        }
        match Session::spawn(cmd, size) {
            Ok((session, events)) => {
                let title = title.to_string();
                self.error = None;
                Some(cx.new(|cx| {
                    let mut view = TerminalView::new(session, events, title, cx);
                    view.shell = shell;
                    view.remote = remote;
                    view
                }))
            }
            Err(e) => {
                self.error = Some(format!("{e}"));
                None
            }
        }
    }

    /// Splits the active tab's focused pane with a new shell, which takes
    /// the keyboard. The shell starts where [`split_folder`] says.
    pub fn split_active(&mut self, dir: SplitDir, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active).cloned() else {
            return;
        };
        let t = tab.read(cx);
        let cmd = Command {
            cwd: split_folder(t, t.state.focused, cx),
            ..Command::default()
        };
        if let Some(view) = self.spawn_view(cmd, &shell_name(), None, cx) {
            tab.update(cx, |t, cx| t.add_pane(dir, view, window, cx));
        }
        cx.notify();
    }

    /// The area the dock gives its tabs, as last drawn: every tab fills the same area, so a tab
    /// that hasn't been drawn (one opened behind the tab in front) is as big as the one that was.
    /// The tab in front was drawn last. `None` before the dock was ever on screen.
    fn drawn_area(&self, cx: &App) -> Option<Rect> {
        let front = self.tabs.get(self.active).and_then(|t| t.read(cx).area());
        front.or_else(|| self.tabs.iter().find_map(|t| t.read(cx).area()))
    }

    /// Splits pane `pane` of the tab at `ix` (see [`Self::target`]: a split tab needs it named)
    /// with a new shell in `cwd`, else where [`split_folder`] says, and types `commands` and Enter
    /// into it when given: an agent's dev server or test watcher next to the user's shell. The
    /// new pane shows as `origin` allows ([`Self::reveal`]), and an agent's never takes the
    /// keyboard or the tab's focus while the user types. A tab not drawn yet is as big as the
    /// dock's tabs ([`Self::drawn_area`]). Refused, with nothing started, when a half would be
    /// under 20 columns by 4 rows, or when the dock was never on screen. Returns the tab and the
    /// new pane.
    #[allow(clippy::too_many_arguments)]
    pub fn split(
        &mut self,
        ix: usize,
        pane: Option<usize>,
        dir: SplitDir,
        cwd: Option<PathBuf>,
        commands: Option<&str>,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        let (ix, target, _) = self.target(Some(ix), pane, cx)?;
        let tab = self.tabs[ix].clone();
        let area = self.drawn_area(cx);
        let which = match pane {
            Some(n) => format!("Terminal {ix}'s pane {n}"),
            None => format!("Terminal {ix}"),
        };
        let way = match dir {
            SplitDir::Right => "in two side by side",
            SplitDir::Down => "in two, one above the other",
        };
        match tab.read(cx).can_split(target, dir, area, window, cx) {
            Ok(()) => {}
            Err(Unsplit::Unmeasured) => {
                return Err(format!(
                    "{which} hasn't been on screen yet, so how much room its panes have isn't \
                     known, and nothing was started. The terminal shows now: split it again."
                ));
            }
            Err(Unsplit::TooSmall) => {
                return Err(format!(
                    "{which} is too small to split {way}: each half needs at least {MIN_COLS} \
                     columns by {MIN_ROWS} rows, and nothing was started. Split another pane, \
                     the other way, or close a pane first."
                ));
            }
        }
        let cmd = Command {
            cwd: cwd.or_else(|| split_folder(tab.read(cx), target, cx)),
            ..Command::default()
        };
        let view = self
            .spawn_view(cmd, &shell_name(), None, cx)
            .ok_or_else(|| {
                self.error
                    .clone()
                    .unwrap_or_else(|| "Unable to open a terminal.".into())
            })?;
        let new = tab
            .update(cx, |t, cx| {
                t.split_pane(target, dir, view.clone(), area, window, cx)
            })
            .map_err(|_| format!("{which} couldn't be split {way}: nothing was started."))?;
        if let Some(commands) = commands.filter(|c| !c.trim().is_empty()) {
            view.update(cx, |view, _| {
                view.touched = true;
                view.write(run_bytes(commands));
            });
        }
        self.reveal(ix, new, origin, window, cx);
        Ok((ix, new))
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

    /// What still runs in the terminals, one name per pane: a command in
    /// the foreground (by the pane's name, its script's, else the program's)
    /// and a live SSH session. Shells at their prompt, a script's terminal
    /// between runs and ended sessions don't count. These end when the app
    /// quits, or with their window.
    pub fn running(&self, cx: &App) -> Vec<String> {
        let mut running = Vec::new();
        for tab in &self.tabs {
            let t = tab.read(cx);
            for (pane, view) in t.views() {
                running.extend(running_in(t, pane, view.read(cx)));
            }
        }
        running
    }

    /// Every coding agent running in the terminals, in the tab strip's order and then each
    /// tab's reading order, background tabs too. One pane is `focused`: the one with the
    /// keyboard, else the one that had it last (the active tab's focused pane, when no pane
    /// has had it yet).
    pub fn running_agents(&self, cx: &App) -> Vec<RunningAgent> {
        let mut last_typed: Option<(std::time::Instant, PaneId)> = None;
        for (_, pane, view) in self.all_panes(cx) {
            if let Some(at) = view.read(cx).keyboard_at()
                && last_typed.is_none_or(|(latest, _)| at > latest)
            {
                last_typed = Some((at, pane));
            }
        }
        let typing = last_typed
            .map(|(_, pane)| pane)
            .or_else(|| self.tabs.get(self.active).map(|t| t.read(cx).state.focused));
        let mut running = Vec::new();
        for tab in &self.tabs {
            let t = tab.read(cx);
            for (pane, kind) in t.agents(cx) {
                let label = match (t.state.is_split(), t.view(pane)) {
                    (true, Some(view)) => t.pane_label(pane, view.read(cx)),
                    _ => t.label(cx),
                };
                running.push(RunningAgent {
                    tab: tab.downgrade(),
                    pane,
                    kind,
                    label,
                    focused: typing == Some(pane),
                });
            }
        }
        running
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

    /// The tab a paste from the browser of `project` (`None`: the browser of no project) may go
    /// to, by the program in its focused pane, which must have enabled bracketed paste
    /// ([`pick_paste_target`]).
    pub fn paste_target(&self, project: Option<&Path>, cx: &App) -> Option<usize> {
        let panes: Vec<FocusedPane> = self
            .tabs
            .iter()
            .map(|t| {
                let view = focused(t, cx);
                FocusedPane {
                    accepts: view.accepts_paste(),
                    at_prompt: view.at_prompt(),
                    remote: view.remote.is_some(),
                    place: match (project, t.read(cx).group.as_deref()) {
                        (None, _) => Place::Project,
                        (Some(_), None) => Place::Loose,
                        (Some(project), Some(group)) if group == project => Place::Project,
                        (Some(_), Some(_)) => Place::Other,
                    },
                }
            })
            .collect();
        pick_paste_target(self.active, &panes)
    }

    /// Pastes `text` into the focused pane of tab `ix` (never pressing
    /// Enter) and focuses it.
    pub fn paste(
        &mut self,
        ix: usize,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let view = self
            .tabs
            .get(ix)
            .map(|t| t.read(cx).focused_view().clone())
            .ok_or_else(|| format!("no terminal {}", ix + 1))?;
        if !view.read(cx).accepts_paste() {
            return Err(format!("terminal {} is not ready for a paste", ix + 1));
        }
        self.active = ix;
        view.update(cx, |v, _| v.paste(text));
        self.focus_active(window, cx);
        cx.notify();
        Ok(())
    }
}

impl TerminalPanel {
    /// The header in front of the run of `root`'s tabs that starts at tab `first`, `count` long:
    /// a click folds the group.
    fn render_group_header(
        &self,
        first: usize,
        count: usize,
        root: PathBuf,
        cx: &Context<Self>,
    ) -> AnyElement {
        let color = self.group_color(&root, cx);
        let collapsed = self.collapsed.contains(&root);
        group_header(
            ("term-group", first),
            project_name(&root),
            color,
            collapsed,
            count,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if !this.collapsed.remove(&root) {
                this.collapsed.insert(root.clone());
            }
            cx.notify();
        }))
        .into_any_element()
    }

    /// Every tab closed, or the first one failed to start.
    fn render_empty(&self, cx: &mut Context<Self>) -> Placeholder {
        let shell = Button::new("empty-new-terminal")
            .small()
            .icon(IconName::Plus)
            .label("New Terminal")
            .on_click(cx.listener(|this, _, window, cx| this.open_shell(Some(window), cx)));
        match &self.error {
            Some(e) => Placeholder::error("Unable to open a terminal")
                .detail(e.clone())
                .action(shell.label("Try Again")),
            None => Placeholder::new(Lucide::SquareTerminal, "No terminals open")
                .detail("Start a shell in the project folder.")
                .action(shell.primary()),
        }
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window = Some(window.window_handle());
        let weak = cx.entity().downgrade();
        // A header in front of each group's run of tabs; a folded group shows its active tab only.
        let groups = self.groups(cx);
        let drawn = strip(&slots(&groups), self.active, |g| self.collapsed.contains(g));
        let mut tabs: Vec<AnyElement> = Vec::with_capacity(drawn.len());
        // The active tab's place among the strip's children, which a scroll goes by.
        let mut active_child = None;
        // A pane on screen, where the tab menus look up the terminal's keys to show them.
        let keys_in = self
            .tabs
            .get(self.active)
            .map(|t| t.read(cx).focused_view().focus_handle(cx));
        let running = self.running_agents(cx);
        for item in drawn {
            let ix = match item {
                Drawn::Tab(ix) => ix,
                Drawn::Header { first, count } => {
                    if let Some(root) = groups[first].clone() {
                        tabs.push(self.render_group_header(first, count, root, cx));
                    }
                    continue;
                }
            };
            let tab = &self.tabs[ix];
            let t = tab.read(cx);
            let v = t.focused_view().read(cx);
            let group = t.group.clone();
            let color = group.as_ref().map(|g| self.group_color(g, cx));
            let active = ix == self.active;
            if active {
                active_child = Some(tabs.len());
            }
            let others: Vec<PathBuf> = self
                .projects
                .iter()
                .filter(|p| Some(*p) != group.as_ref())
                .cloned()
                .collect();
            let weak = weak.clone();
            let keys_in = keys_in.clone();
            let this_tab = tab.downgrade();
            let grouped = group.is_some();
            // The tab is renamed in its label; so is a one-pane tab's pane
            // that was being renamed when the tab's other panes closed.
            let rename = t.tab_rename_field(tab.downgrade()).or_else(|| {
                (!t.state.is_split())
                    .then(|| t.rename_field(tab.downgrade(), t.state.focused))
                    .flatten()
            });
            let named = t.state.name.as_deref().or(t.pane_name());
            tabs.push({
                // SSH tabs keep the server's name (the remote shell's title
                // says little across servers), numbered when it is open twice.
                // A name the user gave the tab or its pane goes over either.
                let (icon, label) = match &v.remote {
                    Some(r) => {
                        let same = |w: &Entity<TerminalTab>| {
                            focused(w, cx)
                                .remote
                                .as_ref()
                                .is_some_and(|o| o.key == r.key)
                        };
                        let nth = self.tabs[..ix].iter().filter(|w| same(w)).count();
                        let name = if let Some(named) = named {
                            named.to_string()
                        } else if nth > 0 || self.tabs[ix + 1..].iter().any(same) {
                            format!("{} {}", r.name, nth + 1)
                        } else {
                            r.name.clone()
                        };
                        let state = if v.exited { " (disconnected)" } else { "" };
                        (Lucide::Server, format!("{name}{state}"))
                    }
                    None if v.job.is_some() => {
                        let name = t.label(cx);
                        let state = if v.exited {
                            " (exited)"
                        } else if v.foreground_job().is_none() {
                            " (stopped)"
                        } else {
                            ""
                        };
                        (Lucide::Play, format!("{name}{state}"))
                    }
                    None if v.exited => (Lucide::Terminal, format!("{} (exited)", t.title(cx))),
                    None => (Lucide::Terminal, t.title(cx).to_string()),
                };
                // A live SSH session or a running script: the glyph turns the success colour,
                // and over agents' logos (which keep their own colours) a dot in it says so.
                let live = (v.remote.is_some() && !v.exited)
                    || (v.job.is_some() && v.foreground_job().is_some());
                let on_tab: Vec<&RunningAgent> = running
                    .iter()
                    .filter(|r| r.tab.entity_id() == tab.entity_id())
                    .collect();
                let kinds: Vec<AgentKind> = on_tab.iter().map(|r| r.kind).collect();
                let running_phrase = (!kinds.is_empty()).then(|| agents::running_phrase(&kinds));
                let icon = match &running_phrase {
                    None => Icon::new(icon)
                        .xsmall()
                        .when(live, |i| i.text_color(cx.theme().success))
                        .into_any_element(),
                    Some(phrase) => {
                        // The tooltip names each pane's agent, by its place in reading order.
                        let mut lines = vec![phrase.clone()];
                        if t.state.is_split() {
                            let order = t.state.reading_order(t.area());
                            lines.extend(on_tab.iter().map(|r| {
                                let n = order.iter().position(|p| *p == r.pane).unwrap_or(0) + 1;
                                format!("Pane {n}: {}", r.kind.name())
                            }));
                        }
                        let badge = agents::badge(kinds.iter().copied());
                        let ring = if active {
                            cx.theme().tab_active
                        } else {
                            cx.theme().tab_bar
                        };
                        agents::logos(&badge, &format!("term-agent-{ix}"), window, cx)
                            .id(("term-agents", ix))
                            .debug_selector(move || format!("term-agents-{ix}"))
                            .relative()
                            .role(Role::Image)
                            .aria_label(phrase.clone())
                            .tooltip(move |window, cx| agents::tooltip(lines.clone(), window, cx))
                            .when(live, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .right(px(-2.))
                                        .bottom(px(-2.))
                                        .size(px(6.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(ring)
                                        .bg(cx.theme().success),
                                )
                            })
                            .into_any_element()
                    }
                };
                h_flex()
                    .id(("term-tab", ix))
                    .role(Role::Tab)
                    .aria_label(label.clone())
                    .when_some(running_phrase, |el, phrase| el.aria_description(phrase))
                    .aria_selected(active)
                    .debug_selector(move || format!("term-tab-{ix}"))
                    .relative()
                    .flex_none()
                    .h_full()
                    .px_2()
                    .gap_1()
                    .text_xs()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .when(active, |el| {
                        el.bg(cx.theme().tab_active)
                            .text_color(cx.theme().tab_active_foreground)
                    })
                    .when(!active, |el| el.text_color(cx.theme().tab_foreground))
                    .child(icon)
                    .map(|el| match rename {
                        Some(field) => el.child(field),
                        None => el.child(label),
                    })
                    .child(
                        Button::new(("close-term", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .accessibility_label("Close Terminal")
                            .debug_selector(move || format!("close-term-{ix}"))
                            .on_click(cx.listener({
                                let tab = this_tab.clone();
                                move |this, _, window, cx| {
                                    // The tab under the × would activate itself next.
                                    cx.stop_propagation();
                                    let had_keyboard = tab
                                        .upgrade()
                                        .is_some_and(|t| t.read(cx).has_focus(window, cx));
                                    this.close_for_user(&tab, had_keyboard, window, cx)
                                }
                            })),
                    )
                    .when_some(color, |el, color| el.child(group_stripe(color, active)))
                    .on_click(cx.listener({
                        let tab = this_tab.clone();
                        move |this, e: &ClickEvent, window, cx| {
                            if let Some(ix) = this.index_of(&tab) {
                                this.activate(ix, window, cx);
                                // A double-click renames the tab.
                                if e.click_count() >= 2 {
                                    this.rename_tab(ix, window, cx);
                                }
                            }
                        }
                    }))
                    .context_menu(move |menu, window, cx| {
                        // The menu has the keyboard from here on: whether the tab had it
                        // before decides where it goes when the tab closes.
                        let had_keyboard = this_tab
                            .upgrade()
                            .is_some_and(|t| t.read(cx).has_focus(window, cx));
                        let rename_weak = weak.clone();
                        let rename_tab = this_tab.clone();
                        let move_weak = weak.clone();
                        let move_tab = this_tab.clone();
                        let others = others.clone();
                        // Its keys are the terminal's: they show as a pane on screen has them.
                        let menu = match &keys_in {
                            Some(pane) => menu.action_context(pane.clone()),
                            None => menu,
                        }
                        .item(
                            PopupMenuItem::new("Rename Tab")
                                .action(Box::new(RenameTab))
                                .on_click(move |_, window, cx| {
                                    let _ = rename_weak.update(cx, |p, cx| {
                                        if let Some(ix) = p.index_of(&rename_tab) {
                                            p.rename_tab(ix, window, cx)
                                        }
                                    });
                                }),
                        );
                        let menu = menu.when(!others.is_empty(), |menu| {
                            menu.submenu("Move to Project", window, cx, move |mut menu, _, _| {
                                for root in &others {
                                    let weak = move_weak.clone();
                                    let tab = move_tab.clone();
                                    let to = root.clone();
                                    menu =
                                        menu.item(PopupMenuItem::new(project_name(root)).on_click(
                                            move |_, _, cx| {
                                                let _ = weak.update(cx, |t, cx| {
                                                    t.move_to_group(&tab, Some(to.clone()), cx)
                                                });
                                            },
                                        ));
                                }
                                menu
                            })
                        });
                        let ungroup = weak.clone();
                        let ungroup_tab = this_tab.clone();
                        let close = weak.clone();
                        let close_tab = this_tab.clone();
                        menu.when(grouped, |menu| {
                            menu.item(PopupMenuItem::new("Remove from Group").on_click(
                                move |_, _, cx| {
                                    let _ = ungroup.update(cx, |t, cx| {
                                        t.move_to_group(&ungroup_tab, None, cx)
                                    });
                                },
                            ))
                        })
                        .separator()
                        .item(PopupMenuItem::new("Close").on_click(move |_, window, cx| {
                            let _ = close.update(cx, |t, cx| {
                                t.close_for_user(&close_tab, had_keyboard, window, cx)
                            });
                        }))
                    })
                    .into_any_element()
            });
        }
        // Bring a newly opened or activated tab into view, by its place among the strip's children:
        // the group headers and the folded tabs make that differ from its index (SMOKE-2).
        let key = self
            .tabs
            .get(self.active)
            .map(|t| (t.entity_id(), self.active));
        if let (Some(key), Some(child)) = (key, active_child)
            && self.scrolled_to != Some(key)
        {
            self.tab_scroll.scroll_to_item(child);
            self.scrolled_to = Some(key);
        }
        let (max_icon, max_tip) = if self.maximized {
            (Lucide::Minimize, "Restore Layout")
        } else {
            (Lucide::Maximize, "Maximize Terminal")
        };

        v_flex()
            .size_full()
            .on_action(cx.listener(|this, _: &SplitRight, window, cx| {
                this.split_active(SplitDir::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SplitDown, window, cx| {
                this.split_active(SplitDir::Down, window, cx)
            }))
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h(px(28.))
                    .bg(cx.theme().tab_bar)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .id("term-tabs")
                            .role(Role::TabList)
                            .aria_label("Terminals")
                            .debug_selector(|| "term-tabs".into())
                            .h_full()
                            .flex_1()
                            .overflow_x_scroll()
                            .track_scroll(&self.tab_scroll)
                            .children(tabs),
                    )
                    .child(
                        h_flex()
                            .px_1()
                            .gap_1()
                            .child(
                                Button::new("new-terminal")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Plus)
                                    .accessibility_label("New Terminal")
                                    .tooltip("New Terminal")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_shell(Some(window), cx)
                                    })),
                            )
                            .child(
                                Button::new("maximize-terminal")
                                    .ghost()
                                    .xsmall()
                                    .icon(max_icon)
                                    .accessibility_label(max_tip)
                                    .tooltip(max_tip)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToggleMaximize))),
                            ),
                    ),
            )
            .when(!self.tabs.is_empty(), |el| {
                el.when_some(self.error.clone(), |el, e| {
                    el.child(Banner::error("term-error", e).on_dismiss(cx.listener(
                        |this, _, _, cx| {
                            this.error = None;
                            cx.notify();
                        },
                    )))
                })
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .map(|el| match self.tabs.get(self.active).cloned() {
                        Some(tab) => el.child(tab),
                        None => el.child(self.render_empty(cx)),
                    }),
            )
    }
}

/// The terminal of `tab`'s focused pane.
fn focused<'a>(tab: &Entity<TerminalTab>, cx: &'a App) -> &'a TerminalView {
    tab.read(cx).focused_view().read(cx)
}

/// What runs in `pane` of `tab`, whose terminal is `view`, that closing it would end, by name: a
/// command in the foreground (by the pane's name, the name of a tab it is alone in, its script's,
/// its coding agent's, else the program's) or a live SSH session. `None` for a shell at its prompt, a script's terminal
/// between runs or an ended session.
fn running_in(tab: &TerminalTab, pane: PaneId, view: &TerminalView) -> Option<String> {
    if view.exited {
        return None;
    }
    let alone = (!tab.state.is_split())
        .then(|| tab.state.name.clone())
        .flatten();
    if view.remote.is_some() {
        let name = alone.unwrap_or_else(|| tab.pane_label(pane, view).to_string());
        return Some(format!("{name} (SSH)"));
    }
    view.foreground_job()?;
    let named = tab
        .state
        .panes
        .get(&pane)
        .and_then(|m| m.name.clone())
        .or(alone);
    Some(
        named
            .or_else(|| view.job.as_ref().map(|j| j.name.clone()))
            .or_else(|| view.agent().map(|a| a.name().to_string()))
            .or_else(|| view.foreground_name())
            .unwrap_or_else(|| view.title.to_string()),
    )
}

/// The folder a shell split off `pane` of `tab` starts in, as ⌘D starts it: the pane's folder if
/// it is still there, else the tab's project, else `$HOME`.
fn split_folder(tab: &TerminalTab, pane: PaneId, cx: &App) -> Option<PathBuf> {
    let folder = tab.view(pane).and_then(|v| v.read(cx).cwd());
    folder
        .filter(|d| d.is_dir())
        .or_else(|| tab.group.clone())
        .or_else(home)
}

/// Tabs in `groups` as grouping sees them: terminal tabs are never pinned.
fn slots(groups: &[Option<PathBuf>]) -> Vec<Slot<'_>> {
    groups
        .iter()
        .map(|g| Slot {
            pinned: false,
            group: g.as_deref(),
        })
        .collect()
}

/// The user's home folder.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Tab title for the default shell ("zsh", "bash", "powershell").
fn shell_name() -> String {
    let (program, _) = default_shell();
    std::path::Path::new(&program)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "shell".into())
}

/// `command` as typed into a shell whose folder is `cwd` (when known) to run
/// in `dir`: as is when the shell is there, else after a `cd` there.
fn command_in(dir: &Path, command: &str, cwd: Option<PathBuf>) -> String {
    let real = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if cwd.as_deref() == Some(real.as_path()) {
        command.to_string()
    } else {
        format!("cd -- {} && {command}", shell_quote(dir))
    }
}

/// Runs `launch` in the terminal.
fn ssh_command(launch: &Launch) -> Command {
    Command {
        program: launch.program.clone(),
        args: launch.args.clone(),
        cwd: None,
        env: launch.env.iter().cloned().collect(),
    }
}

/// A tab's focused pane, as a paste sees it.
#[derive(Clone, Copy, Debug)]
struct FocusedPane {
    /// Its program turned bracketed paste on.
    accepts: bool,
    /// A shell waiting at its prompt, not running a program.
    at_prompt: bool,
    /// An SSH session: what runs there runs on the server.
    remote: bool,
    /// Where its tab works, from the browser's project.
    place: Place,
}

/// Where a terminal tab works, seen from the project of the browser that pastes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// In the browser's project; every tab is, for the browser of no project.
    Project,
    /// In no project: a terminal opened before any project was.
    Loose,
    /// In another project.
    Other,
}

impl FocusedPane {
    /// How well a paste of the browser's notes fits here, best first; `None` where it never
    /// goes: a pane that takes no paste, or an SSH session (its prompt or program is the
    /// server's, not an agent working on this Mac). The browser's project comes first, its agent
    /// (a program that takes a paste) before its shell at its prompt; then a terminal in no
    /// project the same way, as its agent may work on any project; then another project's.
    fn rank(&self) -> Option<u8> {
        if !self.accepts || self.remote {
            return None;
        }
        let agent = !self.at_prompt;
        Some(match (self.place, agent) {
            (Place::Project, true) => 0,
            (Place::Loose, true) => 1,
            (Place::Project, false) => 2,
            (Place::Loose, false) => 3,
            (Place::Other, true) => 4,
            (Place::Other, false) => 5,
        })
    }
}

/// The tab to paste into, by its focused pane's [`FocusedPane::rank`]: among the best, `active`
/// first, else the last. A pane without the keyboard is never chosen.
fn pick_paste_target(active: usize, panes: &[FocusedPane]) -> Option<usize> {
    let best = panes.iter().filter_map(FocusedPane::rank).min()?;
    if panes.get(active).and_then(FocusedPane::rank) == Some(best) {
        return Some(active);
    }
    panes.iter().rposition(|p| p.rank() == Some(best))
}

#[cfg(test)]
mod helper_tests {
    use super::{FocusedPane, Place, command_in, pick_paste_target};

    /// A shell at its prompt, a program that takes a paste (Claude Code), and one that doesn't,
    /// in the browser's project.
    const SHELL: FocusedPane = FocusedPane {
        accepts: true,
        at_prompt: true,
        remote: false,
        place: Place::Project,
    };
    const AGENT: FocusedPane = FocusedPane {
        accepts: true,
        at_prompt: false,
        remote: false,
        place: Place::Project,
    };
    const BUSY: FocusedPane = FocusedPane {
        accepts: false,
        at_prompt: false,
        remote: false,
        place: Place::Project,
    };

    /// `pane` in a tab that works at `place`.
    const fn at(place: Place, pane: FocusedPane) -> FocusedPane {
        FocusedPane { place, ..pane }
    }

    #[test]
    fn a_script_goes_back_to_its_folder_unless_its_shell_is_there() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(
            command_in(&dir, "make build", Some(dir.clone())),
            "make build"
        );
        let moved = command_in(&dir, "make build", Some(dir.join("sub")));
        assert_eq!(
            moved,
            format!("cd -- {} && make build", super::shell_quote(&dir))
        );
        assert_eq!(command_in(&dir, "make build", None), moved);
        let spaced = std::path::Path::new("/tmp/my project");
        assert_eq!(
            command_in(spaced, "pnpm dev", None),
            "cd -- '/tmp/my project' && pnpm dev"
        );
    }

    #[test]
    fn picks_active_else_highest_accepting() {
        assert_eq!(pick_paste_target(1, &[SHELL, BUSY, SHELL]), Some(2));
        assert_eq!(pick_paste_target(0, &[SHELL, BUSY]), Some(0));
        assert_eq!(pick_paste_target(0, &[BUSY, BUSY]), None);
        assert_eq!(pick_paste_target(0, &[]), None);
    }

    /// Annotations go to the terminal running an agent: a tab whose focused pane runs a
    /// program that takes a paste comes before a shell at its prompt, even the active one.
    #[test]
    fn an_agent_comes_before_an_idle_shell() {
        assert_eq!(pick_paste_target(0, &[SHELL, AGENT]), Some(1));
        assert_eq!(pick_paste_target(1, &[AGENT, SHELL, BUSY]), Some(0));
        assert_eq!(
            pick_paste_target(2, &[AGENT, SHELL, AGENT]),
            Some(2),
            "the active one first"
        );
        assert_eq!(
            pick_paste_target(1, &[AGENT, SHELL, AGENT]),
            Some(2),
            "else the last"
        );
    }

    /// An SSH session is never pasted into: the remote shell turns bracketed paste on at its
    /// prompt, and a session isn't a local shell, so it passed for the agent. The notes, local
    /// paths and all, went to the server's prompt.
    #[test]
    fn a_paste_never_goes_to_an_ssh_session() {
        const SSH: FocusedPane = FocusedPane {
            remote: true,
            ..AGENT
        };
        assert_eq!(pick_paste_target(0, &[SHELL, SSH]), Some(0));
        assert_eq!(pick_paste_target(1, &[AGENT, SSH]), Some(0), "even active");
        assert_eq!(pick_paste_target(0, &[SSH]), None);
    }

    /// The browser's notes are about its project, so they go to that project's terminals first:
    /// its agent, else its shell. A terminal in no project comes next, the agent first, as one
    /// may be working on any project; another project's terminal is the last choice. The
    /// highest-numbered agent anywhere won, so one project's notes went to another's agent.
    #[test]
    fn a_paste_goes_to_the_pages_project_first() {
        use Place::{Loose, Other};
        let (theirs, loose) = (at(Other, AGENT), at(Loose, AGENT));
        assert_eq!(pick_paste_target(0, &[SHELL, theirs]), Some(0));
        assert_eq!(
            pick_paste_target(1, &[AGENT, theirs]),
            Some(0),
            "the project's agent, though the other is active"
        );
        assert_eq!(pick_paste_target(1, &[SHELL, loose]), Some(1), "an agent");
        assert_eq!(pick_paste_target(0, &[at(Loose, SHELL), theirs]), Some(0));
        assert_eq!(
            pick_paste_target(0, &[theirs, at(Other, SHELL)]),
            Some(0),
            "another project's terminal, when nothing else takes a paste"
        );
        assert_eq!(pick_paste_target(2, &[AGENT, theirs, AGENT]), Some(2));
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, MouseDownEvent,
        MouseUpEvent, TestAppContext, VisualTestContext,
    };
    use sawit_core::terminal::{run_bytes, shell_quote};

    use super::super::tab::TerminalTab;
    use super::super::testing::{
        focus_terminal, init, open_panel, open_panel_sized, ready, scratch_dir, start, start_cat,
        wait_until,
    };
    use super::{JobTag, Origin, TerminalPanel};
    use gpui_kit::Focusable as _;
    use sawit_core::terminal::{AgentKind, SplitDir};

    /// An agent splits the tab the user types in, `pane` of tab `ix` (from the test window `w`),
    /// with `command` in the new half.
    fn agent_split(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        panel: &Entity<TerminalPanel>,
        (ix, pane): (usize, Option<usize>),
        dir: SplitDir,
        command: Option<&str>,
    ) -> Result<(usize, sawit_core::terminal::PaneId), String> {
        let split = cx
            .update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| {
                    p.split(ix, pane, dir, None, command, Origin::Agent, window, cx)
                })
            })
            .expect("the test window is open");
        cx.run_until_parked();
        split
    }

    /// An agent's split (terminal_split) runs its command in the new half, beside the shell the
    /// user types in: the keyboard and the tab's focus stay with the user's pane, so their next
    /// keys still go there, and the answer names the new pane by its place in the tab, as
    /// app_state lists panes. Without a folder, the new shell starts in the split pane's, as ⌘D's
    /// does.
    #[gpui_kit::test]
    fn an_agents_split_runs_beside_the_user_and_leaves_the_keyboard(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let dir = scratch_dir("agent-split");
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.open_shell_in(dir.clone(), window, cx))
        })
        .expect("the test window is open");
        cx.run_until_parked();
        let user = active(cx, &panel);
        let (ix, users_pane, typing) = cx.update(|cx| {
            let p = panel.read(cx);
            let t = user.read(cx);
            (p.active_index(), t.state.focused, t.focused_view().clone())
        });
        ready(cx, &typing);
        wait_until(cx, "the user's shell runs in the folder", |cx| {
            typing.read(cx).cwd().as_ref() == Some(&dir)
        });
        assert!(has_keyboard(cx, w, &user), "the user isn't typing there");

        let (at, new) = agent_split(
            cx,
            w,
            &panel,
            (ix, None),
            SplitDir::Right,
            Some("printf 'sawit-%s\\n' split"),
        )
        .expect("the pane splits");
        let fresh = cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!((at, p.pane_index(at, new, cx)), (ix, Some(1)));
            assert!(p.tab(ix) == &user, "the split went to another tab");
            let t = user.read(cx);
            assert_eq!(t.views().count(), 2);
            assert_eq!(
                t.state.focused, users_pane,
                "the split took the tab's focus"
            );
            t.view(new).expect("the new pane").clone()
        });
        let typed = cx
            .update_window(w, |_, window, cx| {
                typing.focus_handle(cx).is_focused(window)
            })
            .expect("the test window is open");
        assert!(typed, "the agent's split took the keyboard");
        wait_until(cx, "the command ran in the new pane", |cx| {
            panel
                .read(cx)
                .text(Some(ix), Some(1), 50, cx)
                .is_ok_and(|text| text.contains("sawit-split"))
        });
        wait_until(cx, "the new shell runs in the split pane's folder", |cx| {
            fresh.read(cx).cwd().as_ref() == Some(&dir)
        });
        cx.simulate_keystrokes(w, "x y x y");
        wait_until(cx, "the user's keys reach their shell", |cx| {
            typing.read(cx).text_tail(5).contains("xyxy")
        });
        cx.update(|cx| assert!(!fresh.read(cx).text_tail(50).contains("xyxy")));
    }

    /// A tab an agent opened behind the one the user types in has never been drawn, so it has no
    /// size of its own: it is as big as the dock's other tabs, and splits. With the dock never on
    /// screen at all there is no size to go by, and the split says so instead of starting a
    /// shell.
    #[gpui_kit::test]
    fn an_agents_split_of_a_tab_never_drawn_takes_the_docks_size(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        let user = active(cx, &panel);
        let (behind, _) = cx
            .update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| {
                    p.run_in_new(None, "true", Origin::Agent, window, cx)
                })
            })
            .expect("the test window is open")
            .expect("a new shell opens");
        cx.run_until_parked();
        cx.update(|cx| {
            let p = panel.read(cx);
            assert!(p.tab(behind) != &user, "the agent's tab came in front");
            assert_eq!(p.tab(behind).read(cx).area(), None, "the tab was drawn");
        });
        let (at, new) = agent_split(cx, w, &panel, (behind, None), SplitDir::Down, None)
            .expect("the tab behind splits");
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!((at, p.pane_index(at, new, cx)), (behind, Some(1)));
            assert_eq!(p.tab(behind).read(cx).views().count(), 2);
        });
        assert!(active(cx, &panel) == user, "the split switched tabs");
        assert!(has_keyboard(cx, w, &user), "the split took the keyboard");

        // A panel whose dock was never drawn: the window shows another one.
        let hidden = cx.update(|cx| cx.new(TerminalPanel::new));
        let refused = agent_split(cx, w, &hidden, (0, None), SplitDir::Right, Some("true"))
            .expect_err("a split of a dock never drawn");
        assert!(refused.contains("hasn't been on screen"), "{refused}");
        cx.update(|cx| assert_eq!(hidden.read(cx).tab(0).read(cx).views().count(), 1));
    }

    /// A split that would leave a half under 20 columns by 4 rows is refused, as ⌘D's is, and
    /// says why; nothing is started. A split tab needs the pane to split named, and a pane it
    /// doesn't have is an error.
    #[gpui_kit::test]
    fn a_split_with_no_room_says_why_and_starts_nothing(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel_sized(cx, 280., 600.);
        let refused = agent_split(cx, w, &panel, (0, None), SplitDir::Right, Some("true"))
            .expect_err("a split with no room");
        assert!(refused.contains("20 columns by 4 rows"), "{refused}");
        assert!(refused.contains("nothing was started"), "{refused}");
        cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).views().count(), 1));

        agent_split(cx, w, &panel, (0, None), SplitDir::Down, None).expect("room to stack");
        let unnamed = agent_split(cx, w, &panel, (0, None), SplitDir::Down, None)
            .expect_err("a split tab with no pane named");
        assert!(unnamed.contains("name the pane"), "{unnamed}");
        let missing = agent_split(cx, w, &panel, (0, Some(5)), SplitDir::Down, None)
            .expect_err("a pane the tab doesn't have");
        assert!(missing.contains("no pane 5"), "{missing}");
        let tab = agent_split(cx, w, &panel, (3, None), SplitDir::Down, None)
            .expect_err("a tab there isn't");
        assert!(tab.contains("no terminal 3"), "{tab}");
        cx.update(|cx| assert_eq!(panel.read(cx).tab(0).read(cx).views().count(), 2));
    }

    /// An agent's terminal_close ends what runs there, so without its confirm it is refused
    /// while a program runs in the tab or pane, or the user is typing in it, naming why; shells
    /// waiting at their prompt that nobody types in close as before.
    #[gpui_kit::test]
    fn closing_asks_while_a_program_runs_or_the_user_types(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 1);
        click_tab(cx, w, &panel, 0);
        let other = cx.update(|cx| panel.read(cx).tab(1).read(cx).focused_view().clone());
        ready(cx, &other);
        let refusal = |cx: &mut TestAppContext, ix: usize, pane: Option<usize>| {
            cx.update_window(w, |_, window, cx| {
                panel.read(cx).close_refusal(ix, pane, window, cx)
            })
            .expect("the test window is open")
        };

        assert_eq!(
            refusal(cx, 1, None),
            Ok(None),
            "an idle shell nobody types in"
        );
        let typing = refusal(cx, 0, None).unwrap().expect("the user types there");
        assert!(typing.contains("the user is typing in it"), "{typing}");
        assert!(typing.contains("confirm: true"), "{typing}");

        start(cx, &other, "sleep 30", "sleep");
        let busy = refusal(cx, 1, Some(0)).unwrap().expect("sleep runs there");
        assert!(
            busy.starts_with("Terminal 1, pane 0 wasn't closed"),
            "{busy}"
        );
        assert!(busy.contains("(sleep)"), "{busy}");
        assert!(!busy.contains("typing"), "{busy}");
        assert!(refusal(cx, 4, None).is_err());
    }

    /// The active tab.
    fn active(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>) -> Entity<TerminalTab> {
        cx.update(|cx| {
            let p = panel.read(cx);
            p.tab(p.active_index()).clone()
        })
    }

    /// Opens `n` more shell tabs; the last one opened is the active one.
    fn open_tabs(cx: &mut TestAppContext, panel: &Entity<TerminalPanel>, n: usize) {
        for _ in 0..n {
            cx.update(|cx| panel.update(cx, |p, cx| p.open_shell(None, cx)));
        }
        cx.run_until_parked();
    }

    /// Makes tab `ix` the active one with the keyboard, as a click on it does.
    fn click_tab(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        panel: &Entity<TerminalPanel>,
        ix: usize,
    ) {
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.activate(ix, window, cx))
        })
        .expect("the test window is open");
        cx.run_until_parked();
    }

    /// Whether a pane of `tab` has the keyboard.
    fn has_keyboard(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        tab: &Entity<TerminalTab>,
    ) -> bool {
        cx.update_window(w, |_, window, cx| tab.read(cx).has_focus(window, cx))
            .expect("the test window is open")
    }

    /// A debug selector made at run time: tests only, so leaking it is fine.
    fn selector(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    /// Clicks the × of tab `ix` in the strip.
    fn click_close(cx: &mut TestAppContext, w: AnyWindowHandle, ix: usize) {
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        let at = vcx
            .debug_bounds(selector(format!("close-term-{ix}")))
            .unwrap_or_else(|| panic!("tab {ix}'s × isn't drawn"))
            .center();
        vcx.simulate_click(at, Modifiers::default());
        vcx.run_until_parked();
    }

    /// The × of the active, rightmost tab closes it and the tab left of it shows, with the
    /// keyboard the closed tab had. The tab's own click must not run too: it would make the
    /// closed tab's index (now past the end) the active one, and the dock would say "No
    /// terminals open" while a tab is left.
    #[gpui_kit::test]
    fn the_close_button_of_the_last_tab_shows_the_one_left(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let first = active(cx, &panel);
        open_tabs(cx, &panel, 1);
        click_tab(cx, w, &panel, 1);
        click_close(cx, w, 1);
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.tab_count(), 1);
            assert_eq!(p.active_index(), 0, "the active tab is past the end");
            assert!(
                p.text(None, None, 5, cx).is_ok(),
                "reading the active tab fails"
            );
        });
        assert!(
            has_keyboard(cx, w, &first),
            "the keyboard didn't go to the tab left"
        );
    }

    /// The × of a tab in the background closes only that tab, on either side of the active
    /// one: the active tab stays, and so does the keyboard.
    #[gpui_kit::test]
    fn the_close_button_of_a_background_tab_keeps_the_active_one(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 3);
        // [A, B*, C, D], B with the keyboard.
        click_tab(cx, w, &panel, 1);
        let b = active(cx, &panel);
        for (ix, closing) in [
            (2, "C, right of B"),
            (0, "A, left of B"),
            (1, "D, the last"),
        ] {
            click_close(cx, w, ix);
            assert!(active(cx, &panel) == b, "closing {closing} switched tabs");
            assert!(
                has_keyboard(cx, w, &b),
                "closing {closing} took the keyboard"
            );
        }
        cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 1));
    }

    /// The × of the active tab while it has the keyboard hands the keyboard to the tab that
    /// takes its place.
    #[gpui_kit::test]
    fn the_close_button_of_the_typing_tab_hands_on_the_keyboard(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 1);
        // [A*, B], A with the keyboard.
        click_tab(cx, w, &panel, 0);
        let b = cx.update(|cx| panel.read(cx).tab(1).clone());
        click_close(cx, w, 0);
        assert!(active(cx, &panel) == b);
        assert!(has_keyboard(cx, w, &b), "the keyboard went nowhere");
    }

    /// A tab's menu acts on the tab it was opened for, even when the strip changes while it is
    /// open: here an agent closes a tab left of it, and the menu's Close still closes its tab,
    /// not the one that moved into its place.
    #[gpui_kit::test]
    fn a_tabs_menu_closes_the_tab_it_was_opened_for(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 3);
        let [a, b, c, d] = cx.update(|cx| {
            let p = panel.read(cx);
            [0, 1, 2, 3].map(|ix| p.tab(ix).clone())
        });
        // Right-click B in the strip.
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        let at = vcx
            .debug_bounds("term-tab-1")
            .expect("tab 1 is drawn")
            .center();
        vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        vcx.run_until_parked();
        // An agent closes A while the menu is open.
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.close_tab(0, None, window, cx))
        })
        .expect("the test window is open")
        .expect("tab 0 closes");
        cx.run_until_parked();
        // The menu's last item is Close.
        cx.simulate_keystrokes(w, "up enter");
        cx.run_until_parked();
        let left: Vec<Entity<TerminalTab>> = cx.update(|cx| {
            let p = panel.read(cx);
            (0..p.tab_count()).map(|ix| p.tab(ix).clone()).collect()
        });
        assert!(!left.contains(&a));
        assert!(!left.contains(&b), "the menu's Close didn't close its tab");
        assert_eq!(left, [c.clone(), d], "the menu's Close closed another tab");

        // [C, D*] with the keyboard in D: closing D from its menu hands the keyboard to C.
        click_tab(cx, w, &panel, 1);
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        let at = vcx
            .debug_bounds("term-tab-1")
            .expect("tab 1 is drawn")
            .center();
        vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        vcx.run_until_parked();
        cx.simulate_keystrokes(w, "up enter");
        cx.run_until_parked();
        assert!(active(cx, &panel) == c);
        assert!(has_keyboard(cx, w, &c), "the keyboard went nowhere");
    }

    /// Right-clicks tab `ix` in the strip, opening its menu.
    fn open_tab_menu(cx: &mut TestAppContext, w: AnyWindowHandle, ix: usize) {
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        let at = vcx
            .debug_bounds(selector(format!("term-tab-{ix}")))
            .expect("the tab is drawn")
            .center();
        vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        vcx.run_until_parked();
    }

    /// Double-clicks tab `ix` in the strip.
    fn double_click_tab(cx: &mut TestAppContext, w: AnyWindowHandle, ix: usize) {
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        let position = vcx
            .debug_bounds(selector(format!("term-tab-{ix}")))
            .expect("the tab is drawn")
            .center();
        for click_count in [1, 2] {
            vcx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count,
                first_mouse: false,
            });
            vcx.simulate_event(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count,
            });
        }
        vcx.run_until_parked();
    }

    /// Types `name` in the open rename field and presses Enter.
    fn enter_name(cx: &mut TestAppContext, w: AnyWindowHandle, name: &str) {
        if !name.is_empty() {
            cx.simulate_input(w, name);
        } else {
            cx.simulate_keystrokes(w, "backspace");
        }
        cx.simulate_keystrokes(w, "enter");
        cx.run_until_parked();
    }

    /// A tab's menu starts with Rename Tab, which names the tab it was opened for and shows it;
    /// a double-click on a tab does the same. An empty name gives the tab its pane's title back.
    #[gpui_kit::test]
    fn a_tabs_menu_and_a_double_click_rename_the_tab(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 1);
        let [a, b] = cx.update(|cx| {
            let p = panel.read(cx);
            [0, 1].map(|ix| p.tab(ix).clone())
        });
        assert!(active(cx, &panel) == b);

        open_tab_menu(cx, w, 0);
        cx.simulate_keystrokes(w, "down enter");
        cx.run_until_parked();
        enter_name(cx, w, "server");
        assert!(active(cx, &panel) == a, "the renamed tab isn't shown");
        cx.update(|cx| {
            assert_eq!(a.read(cx).state.name.as_deref(), Some("server"));
            assert_eq!(a.read(cx).title(cx), "server");
            assert_eq!(b.read(cx).state.name, None, "another tab was renamed");
        });
        assert!(
            has_keyboard(cx, w, &a),
            "the keyboard didn't go back to the tab"
        );
        double_click_tab(cx, w, 1);
        enter_name(cx, w, "db");
        assert!(active(cx, &panel) == b);
        cx.update(|cx| assert_eq!(b.read(cx).state.name.as_deref(), Some("db")));

        double_click_tab(cx, w, 0);
        enter_name(cx, w, "");
        cx.update(|cx| {
            let t = a.read(cx);
            assert_eq!(t.state.name, None, "an empty name didn't clear it");
            assert_eq!(
                t.title(cx),
                t.pane_title(t.state.focused, t.focused_view().read(cx))
            );
        });
    }

    /// Closing a tab left of the active one keeps the active tab (and the keyboard in it), as
    /// when an agent's `terminal_close` tidies up while the user types in a later tab.
    #[gpui_kit::test]
    fn closing_a_tab_left_of_the_active_one_keeps_it(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 3);
        // [A, B, C*, D], C with the keyboard.
        click_tab(cx, w, &panel, 2);
        let c = active(cx, &panel);
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.close_tab(0, None, window, cx))
        })
        .expect("the test window is open")
        .expect("tab 0 closes");
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 3));
        assert!(active(cx, &panel) == c, "the active tab moved off C");
        assert!(has_keyboard(cx, w, &c));
    }

    /// `run` never types into a Run panel script's terminal, even once the script has ended
    /// and its shell waits at its prompt: the script would then read as running again, and
    /// its Stop would interrupt the other command.
    #[gpui_kit::test]
    fn run_leaves_a_finished_scripts_shell_alone(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &shell);
        let tag = JobTag {
            key: "build".into(),
            name: "build".into(),
        };
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.start_job(
                    tag,
                    std::env::temp_dir(),
                    "printf 'sawit-%s\\n' built",
                    Some(Origin::User),
                    window,
                    cx,
                )
            })
        })
        .expect("the test window is open")
        .expect("the script starts");
        let job = cx.update(|cx| {
            let p = panel.read(cx);
            p.tab(p.active_index()).read(cx).focused_view().clone()
        });
        wait_until(cx, "the script ran and its shell is at its prompt", |cx| {
            let v = job.read(cx);
            v.text_tail(50).contains("sawit-built") && v.at_prompt()
        });

        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run("printf 'sawit-%s\\n' other", Origin::User, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("a shell runs it");
        wait_until(cx, "the command ran", |cx| {
            let ran =
                |v: &Entity<super::TerminalView>| v.read(cx).text_tail(50).contains("sawit-other");
            ran(&shell) || ran(&job)
        });
        cx.update(|cx| {
            assert!(
                !job.read(cx).text_tail(50).contains("sawit-other"),
                "the command went into the script's terminal"
            );
            assert!(shell.read(cx).text_tail(50).contains("sawit-other"));
            let status = panel
                .read(cx)
                .job_status("build", cx)
                .expect("the script's tab");
            assert_eq!(status.running, None, "the script reads as running");
        });
    }

    /// Commands an agent types (`terminal_input`) mark the shell as used, so opening the
    /// window's first project moves it there with a `cd` instead of replacing it.
    #[gpui_kit::test]
    fn an_agents_input_keeps_its_shell_when_the_first_project_opens(cx: &mut TestAppContext) {
        cx.update(init);
        let (_w, panel) = open_panel(cx);
        let view = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        wait_until(cx, "the shell is at its prompt", |cx| {
            view.read(cx).at_prompt()
        });
        cx.update(|cx| {
            panel.update(cx, |p, cx| {
                p.input(Some(0), None, run_bytes("printf 'sawit-%s\\n' typed"), cx)
            })
        })
        .expect("the shell takes input");
        wait_until(cx, "the shell ran what the agent typed", |cx| {
            let v = view.read(cx);
            v.text_tail(50).contains("sawit-typed") && v.at_prompt()
        });
        let project = scratch_dir("agent-input-project");
        cx.update(|cx| {
            panel.update(cx, |p, cx| {
                p.set_projects(vec![project.clone()], Some(project.clone()), cx)
            })
        });
        cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            assert!(
                *tab.focused_view() == view,
                "the shell the agent used was replaced"
            );
        });
        wait_until(cx, "the shell moved to the project", |cx| {
            view.read(cx).cwd().as_ref() == Some(&project)
        });
    }

    /// Asserts that the active tab is drawn whole inside the strip, as the last frame drew them,
    /// its scroll included; `what` says which tab that is.
    fn assert_active_in_view(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        panel: &Entity<TerminalPanel>,
        what: &str,
    ) {
        let ix = cx.update(|cx| panel.read(cx).active_index());
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        let strip = vcx.debug_bounds("term-tabs").expect("the strip is drawn");
        let tab = vcx
            .debug_bounds(selector(format!("term-tab-{ix}")))
            .unwrap_or_else(|| panic!("{what}: tab {ix} isn't drawn"));
        let slack = gpui_kit::px(0.5);
        assert!(
            tab.left() >= strip.left() - slack && tab.right() <= strip.right() + slack,
            "{what}: tab {ix} is at x {:?}..{:?}, out of the strip's {:?}..{:?}",
            tab.left(),
            tab.right(),
            strip.left(),
            strip.right(),
        );
    }

    /// SMOKE-2: the strip scrolls the active tab into view, however it became the active one: a
    /// new tab opened past the strip's end, a tab activated left of what shows, a new tab that
    /// takes the active tab's index, and a tab after a folded group. The strip draws a header in
    /// front of each project's tabs and a folded group's other tabs not at all, so the scroll goes
    /// by the tab's place among what is drawn, not by its index.
    #[gpui_kit::test]
    fn the_strip_scrolls_the_active_tab_into_view(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel_sized(cx, 420., 300.);
        let (alpha, beta) = (scratch_dir("strip-alpha"), scratch_dir("strip-beta"));
        let projects = vec![alpha.clone(), beta.clone()];
        // `n` new tabs in `project`, the way ⌘T opens them there.
        let open_in = |cx: &mut TestAppContext, project: &std::path::PathBuf, n: usize| {
            let (projects, project) = (projects.clone(), project.clone());
            cx.update(|cx| panel.update(cx, |p, cx| p.set_projects(projects, Some(project), cx)));
            open_tabs(cx, &panel, n);
        };

        // Alpha's tabs, then more of beta's than the strip holds.
        open_in(cx, &alpha, 2);
        open_in(cx, &beta, 8);
        assert_active_in_view(cx, w, &panel, "the newest tab");

        // Beta's first tab, by now left of what the strip shows.
        let first_beta = cx.update(|cx| {
            let p = panel.read(cx);
            (0..p.tab_count())
                .find(|&ix| p.tab(ix).read(cx).group.as_ref() == Some(&beta))
                .expect("beta has tabs")
        });
        click_tab(cx, w, &panel, first_beta);
        assert_active_in_view(cx, w, &panel, "a tab left of what shows");

        // A new tab of alpha goes at the end of alpha's tabs: at the active tab's index.
        open_in(cx, &alpha, 1);
        cx.update(|cx| assert_eq!(panel.read(cx).active_index(), first_beta));
        assert_active_in_view(cx, w, &panel, "a new tab at the active tab's index");

        // Beta's newest tab, with alpha folded: alpha draws its header alone.
        cx.update(|cx| {
            panel.update(cx, |p, cx| {
                p.collapsed.insert(alpha.clone());
                cx.notify();
            })
        });
        open_in(cx, &beta, 1);
        assert_active_in_view(cx, w, &panel, "a tab after a folded group");
    }

    /// Running a script again types it into its old shell after a `cd` back to the script's
    /// folder: its command is relative (`pnpm run dev`, `make build`) and the shell may have
    /// moved since.
    #[gpui_kit::test]
    fn running_a_script_again_goes_back_to_its_folder(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let home = scratch_dir("script-home");
        let elsewhere = scratch_dir("script-elsewhere");
        let start = |cx: &mut TestAppContext, command: &str| {
            let tag = JobTag {
                key: "build".into(),
                name: "build".into(),
            };
            cx.update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| {
                    p.start_job(tag, home.clone(), command, Some(Origin::User), window, cx)
                })
            })
            .expect("the test window is open")
            .expect("the script starts");
        };
        start(cx, "printf 'sawit-%s\\n' first");
        let job = cx.update(|cx| {
            let p = panel.read(cx);
            p.tab(p.active_index()).read(cx).focused_view().clone()
        });
        wait_until(cx, "the script ran in its folder", |cx| {
            let v = job.read(cx);
            v.text_tail(50).contains("sawit-first")
                && v.at_prompt()
                && v.cwd().as_ref() == Some(&home)
        });
        let cd = format!("cd {}", shell_quote(&elsewhere));
        cx.update(|cx| job.read(cx).write(run_bytes(&cd)));
        wait_until(cx, "the script's shell moved", |cx| {
            let v = job.read(cx);
            v.at_prompt() && v.cwd().as_ref() == Some(&elsewhere)
        });

        start(cx, "printf 'sawit-%s\\n' second");
        wait_until(cx, "the script ran again", |cx| {
            let v = job.read(cx);
            v.text_tail(50).contains("sawit-second") && v.at_prompt()
        });
        cx.update(|cx| {
            assert_eq!(
                job.read(cx).cwd(),
                Some(home.clone()),
                "the script ran where its shell had gone"
            )
        });
    }

    /// While the user types in a terminal (here into `cat`, standing in for Claude Code), an
    /// agent's command runs in another shell without switching tabs or taking the keyboard:
    /// the rest of what the user types still goes where they were typing.
    #[gpui_kit::test]
    fn an_agents_command_leaves_the_typing_user_alone(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 1);
        click_tab(cx, w, &panel, 0);
        let (user, other) = cx.update(|cx| {
            let p = panel.read(cx);
            (p.tab(0).clone(), p.tab(1).read(cx).focused_view().clone())
        });
        let typing = cx.update(|cx| user.read(cx).focused_view().clone());
        ready(cx, &typing);
        ready(cx, &other);
        start_cat(cx, &typing);

        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run("printf 'sawit-%s\\n' agent", Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("a shell runs it");
        cx.run_until_parked();
        wait_until(cx, "the agent's command ran in the other shell", |cx| {
            other.read(cx).text_tail(50).contains("sawit-agent")
        });
        assert!(active(cx, &panel) == user, "the agent switched tabs");
        assert!(has_keyboard(cx, w, &user), "the agent took the keyboard");
        cx.simulate_keystrokes(w, "x y x y");
        wait_until(cx, "the user's keys reach cat", |cx| {
            typing.read(cx).text_tail(50).contains("xyxy")
        });
        cx.update(|cx| assert!(!other.read(cx).text_tail(50).contains("xyxy")));
    }

    /// While the user types at a shell's prompt, a line they haven't sent yet, an agent's command
    /// runs in another idle shell, else in a new tab behind theirs, and never in the user's: there
    /// it would join their half-typed line and run with it on the agent's Enter, or, at an empty
    /// prompt, run in front of them and take their next keys. Their line stays, and runs as typed.
    #[gpui_kit::test]
    fn an_agents_command_never_types_into_the_shell_the_user_types_in(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        let user = active(cx, &panel);
        let typing = cx.update(|cx| user.read(cx).focused_view().clone());
        ready(cx, &typing);
        cx.simulate_keystrokes(w, "e c h o space x y x y");
        wait_until(cx, "the user's line shows what they typed", |cx| {
            typing.read(cx).text_tail(5).contains("echo xyxy")
        });
        let agent = |cx: &mut TestAppContext, word: &str| {
            let command = format!("printf 'sawit-%s\\n' {word}");
            cx.update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| p.run(&command, Origin::Agent, window, cx))
            })
            .expect("the test window is open")
            .expect("a shell runs it")
        };

        // The user's shell is the only one, so the command gets a tab of its own.
        let (ix, pane) = agent(cx, "agent-first");
        assert_ne!(
            ix, 0,
            "the agent's command went into the shell the user types in"
        );
        let shell = cx
            .update(|cx| panel.read(cx).tab(ix).read(cx).view(pane).cloned())
            .expect("the agent's pane");
        wait_until(cx, "the agent's command ran, and its shell is idle", |cx| {
            let v = shell.read(cx);
            v.text_tail(50).contains("sawit-agent-first") && v.idle_shell()
        });
        // That shell is idle now, and takes the next command: no third tab.
        assert_eq!(
            agent(cx, "agent-second"),
            (ix, pane),
            "the next command didn't go to the idle shell"
        );
        wait_until(cx, "the next command ran", |cx| {
            shell.read(cx).text_tail(50).contains("sawit-agent-second")
        });
        cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 2));
        assert!(active(cx, &panel) == user, "the agent switched tabs");
        assert!(has_keyboard(cx, w, &user), "the agent took the keyboard");
        cx.update(|cx| {
            assert!(
                !typing.read(cx).text_tail(50).contains("agent"),
                "the agent's text reached the user's shell"
            )
        });

        cx.simulate_keystrokes(w, "enter");
        wait_until(cx, "the user's line ran as typed", |cx| {
            typing
                .read(cx)
                .text_tail(50)
                .lines()
                .any(|l| l.trim_end() == "xyxy")
        });
    }

    /// With the keyboard outside the terminal (an editor, say), an agent's command shows the
    /// tab it runs in but leaves the keyboard where it is.
    #[gpui_kit::test]
    fn an_agents_command_shows_its_tab_but_leaves_the_keyboard(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 1);
        let target = cx.update(|cx| panel.read(cx).tab(0).clone());
        let shell = cx.update(|cx| target.read(cx).focused_view().clone());
        // Tab 1 is active and busy, so the command goes to tab 0.
        let busy = cx.update(|cx| panel.read(cx).tab(1).read(cx).focused_view().clone());
        ready(cx, &shell);
        ready(cx, &busy);
        start_cat(cx, &busy);
        // The keyboard is elsewhere in the window.
        let editor = cx.update(|cx| cx.focus_handle());
        cx.update_window(w, |_, window, cx| window.focus(&editor, cx))
            .expect("the test window is open");

        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run("printf 'sawit-%s\\n' agent", Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("a shell runs it");
        cx.run_until_parked();
        wait_until(cx, "the agent's command ran", |cx| {
            shell.read(cx).text_tail(50).contains("sawit-agent")
        });
        assert!(
            active(cx, &panel) == target,
            "the tab it runs in isn't shown"
        );
        let kept = cx
            .update_window(w, |_, window, _| editor.is_focused(window))
            .expect("the test window is open");
        assert!(kept, "the agent took the keyboard");
    }

    /// An agent's new terminal (terminal_run with new_tab, a Run panel script, an SSH
    /// session) opens beside the tab the user types in, without switching to it or taking the
    /// keyboard.
    #[gpui_kit::test]
    fn an_agents_new_terminals_open_behind_the_typing_user(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        let user = active(cx, &panel);
        let check = |cx: &mut TestAppContext, what: &str| {
            cx.run_until_parked();
            assert!(active(cx, &panel) == user, "{what} switched tabs");
            assert!(has_keyboard(cx, w, &user), "{what} took the keyboard");
        };
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.run_in_new(None, "true", Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("a new shell opens");
        check(cx, "a new tab");
        let tag = JobTag {
            key: "dev".into(),
            name: "dev".into(),
        };
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.start_job(
                    tag,
                    std::env::temp_dir(),
                    "true",
                    Some(Origin::Agent),
                    window,
                    cx,
                )
            })
        })
        .expect("the test window is open")
        .expect("the script starts");
        check(cx, "a script");
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.open_ssh(test_remote(), Origin::Agent, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("the session starts");
        check(cx, "an SSH session");
        cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 4));
    }

    /// When no shell can start for a command, `run` says why instead of naming a terminal: an
    /// agent would otherwise read the terminal that was active (cat's) as its command's output.
    #[gpui_kit::test]
    fn run_says_when_no_shell_can_start(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let busy = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &busy);
        start_cat(cx, &busy);
        cx.update(|cx| panel.update(cx, |p, _| p.fail_spawns = true));

        let ran = cx
            .update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| {
                    p.run("printf 'sawit-%s\\n' lost", Origin::Agent, window, cx)
                })
            })
            .expect("the test window is open");
        assert!(ran.is_err(), "run named terminal {ran:?}");
        cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 1));
        cx.update(|cx| assert!(!busy.read(cx).text_tail(50).contains("lost")));
    }

    /// The pane `run` typed into, by its place in the tab (as app_state lists panes), is what
    /// `terminal_run` reports and reads back, rather than the tab's focused pane.
    #[gpui_kit::test]
    fn run_names_the_pane_it_typed_into(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let (first, second) = cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            let panes = tab.state.layout.panes();
            (
                tab.view(panes[0]).unwrap().clone(),
                tab.view(panes[1]).unwrap().clone(),
            )
        });
        ready(cx, &first);
        ready(cx, &second);
        // The user types in the second pane, busy with cat; the agent's command goes to the
        // first, which doesn't take the tab's focus while the user types.
        start_cat(cx, &second);
        let (ix, pane) = cx
            .update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| {
                    p.run("printf 'sawit-%s\\n' here", Origin::Agent, window, cx)
                })
            })
            .expect("the test window is open")
            .expect("a shell runs it");
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!((ix, p.pane_index(ix, pane, cx)), (0, Some(0)));
            assert_ne!(
                p.tab(0).read(cx).state.focused,
                pane,
                "the agent took the focus"
            );
        });
        wait_until(cx, "the command ran in the first pane", |cx| {
            panel
                .read(cx)
                .text(Some(0), Some(0), 50, cx)
                .is_ok_and(|t| t.contains("sawit-here"))
        });
    }

    /// While the user types in one pane of a split tab (into `cat`, standing in for Claude Code)
    /// and an agent's command runs in the other, the tab's focused pane is the user's. So keys and
    /// reads for that tab must name their pane: a ctrl-c meant for the agent's command would
    /// otherwise interrupt the user's program. Named, it stops the agent's command, and `cat`
    /// keeps running.
    #[gpui_kit::test]
    fn a_split_tab_takes_keys_only_for_a_named_pane(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let (agents, users) = cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            let panes = tab.state.layout.panes();
            (
                tab.view(panes[0]).unwrap().clone(),
                tab.view(panes[1]).unwrap().clone(),
            )
        });
        ready(cx, &agents);
        ready(cx, &users);
        start_cat(cx, &users);
        let (ix, pane) = cx
            .update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| p.run("sleep 30", Origin::Agent, window, cx))
            })
            .expect("the test window is open")
            .expect("a shell runs it");
        let pane = cx.update(|cx| panel.read(cx).pane_index(ix, pane, cx));
        assert_eq!((ix, pane), (0, Some(0)), "the command went elsewhere");
        wait_until(cx, "the agent's sleep runs", |cx| {
            agents.read(cx).foreground_name().as_deref() == Some("sleep")
        });

        let interrupt = |cx: &mut TestAppContext, pane: Option<usize>| {
            cx.update(|cx| panel.update(cx, |p, cx| p.input(Some(0), pane, vec![0x03], cx)))
        };
        let sent = interrupt(cx, None);
        assert!(
            sent.as_ref().is_err_and(|e| e.contains("pane")),
            "keys went to the split tab's focused pane: {sent:?}"
        );
        let read = cx.update(|cx| panel.read(cx).text(Some(0), None, 10, cx));
        assert!(
            read.as_ref().is_err_and(|e| e.contains("pane")),
            "the split tab's focused pane was read: {read:?}"
        );

        interrupt(cx, Some(0)).expect("the agent's pane takes the keys");
        wait_until(cx, "the agent's sleep stops", |cx| {
            agents.read(cx).at_prompt()
        });
        cx.update(|cx| {
            assert_eq!(
                users.read(cx).foreground_name().as_deref(),
                Some("cat"),
                "the user's program was interrupted"
            )
        });
    }

    /// A script an agent starts while the user types in a terminal opens behind it, and says where
    /// it runs: app_run_script names that terminal, which the agent's terminal_read and
    /// terminal_input need, since the terminal in front is still the user's. A script whose shell
    /// can't start says why instead.
    #[gpui_kit::test]
    fn an_agents_script_says_where_it_runs(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        let user = active(cx, &panel);
        let typing = cx.update(|cx| user.read(cx).focused_view().clone());
        ready(cx, &typing);
        start_cat(cx, &typing);
        let start = |cx: &mut TestAppContext, key: &str| {
            let tag = JobTag {
                key: key.into(),
                name: key.into(),
            };
            cx.update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| {
                    p.start_job(
                        tag,
                        std::env::temp_dir(),
                        "sleep 30",
                        Some(Origin::Agent),
                        window,
                        cx,
                    )
                })
            })
            .expect("the test window is open")
        };

        let (ix, pane) = start(cx, "dev").expect("the script starts");
        assert!(active(cx, &panel) == user, "the script came in front");
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.pane_index(ix, pane, cx), Some(0));
            assert_eq!(p.job_terminal("dev", cx), Some((ix, 0)));
        });
        let script = cx.update(|cx| panel.read(cx).tab(ix).read(cx).focused_view().clone());
        wait_until(cx, "the script runs", |cx| {
            script.read(cx).foreground_name().as_deref() == Some("sleep")
        });
        cx.update(|cx| panel.update(cx, |p, cx| p.input(Some(ix), Some(0), vec![0x03], cx)))
            .expect("the script's terminal takes the keys");
        wait_until(cx, "the script stops", |cx| script.read(cx).at_prompt());
        cx.update(|cx| {
            assert_eq!(typing.read(cx).foreground_name().as_deref(), Some("cat"));
        });

        cx.update(|cx| panel.update(cx, |p, _| p.fail_spawns = true));
        let failed = start(cx, "build");
        assert!(
            failed.is_err(),
            "a script with no shell started: {failed:?}"
        );
        cx.update(|cx| assert_eq!(panel.read(cx).job_terminal("build", cx), None));
    }

    /// What quitting or closing the window would end: commands in the foreground and live SSH
    /// sessions. Shells at their prompt and a script's terminal between runs don't count.
    #[gpui_kit::test]
    fn running_lists_what_would_end_with_the_app(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let shell = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        ready(cx, &shell);
        let tag = JobTag {
            key: "build".into(),
            name: "build".into(),
        };
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.start_job(
                    tag,
                    std::env::temp_dir(),
                    "printf 'sawit-%s\\n' built",
                    Some(Origin::User),
                    window,
                    cx,
                )
            })
        })
        .expect("the test window is open")
        .expect("the script starts");
        let job = active(cx, &panel);
        let job = cx.update(|cx| job.read(cx).focused_view().clone());
        wait_until(cx, "the script ran", |cx| {
            let v = job.read(cx);
            v.text_tail(50).contains("sawit-built") && v.at_prompt()
        });
        let running = |cx: &mut TestAppContext| cx.update(|cx| panel.read(cx).running(cx));
        assert_eq!(running(cx), Vec::<String>::new());

        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.open_ssh(test_remote(), Origin::User, window, cx)
            })
        })
        .expect("the test window is open")
        .expect("the session starts");
        assert_eq!(running(cx), ["test (SSH)"]);

        start(cx, &shell, "sleep 30", "sleep");
        assert_eq!(running(cx), ["sleep", "test (SSH)"]);
    }

    /// A stand-in SSH server: a session that ends once it reads a line.
    fn test_remote() -> super::Remote {
        super::Remote {
            key: "test".into(),
            name: "test".into(),
            launch: sawit_core::ssh::Launch {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "read line".into()],
                env: Vec::new(),
            },
        }
    }

    #[gpui_kit::test]
    fn a_paste_goes_to_the_focused_pane(cx: &mut TestAppContext) {
        const MARK: &str = "sawit-paste-marker";
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let (other, focused) = cx.update(|cx| {
            let tab = panel.read(cx).tab(0).read(cx);
            let panes = tab.state.layout.panes();
            assert_eq!(panes.len(), 2);
            assert_eq!(tab.state.focused, panes[1]);
            let other = tab.view(panes[0]).expect("the first pane").clone();
            (other, tab.focused_view().clone())
        });
        // A program that turns bracketed paste on, then echoes what it gets.
        cx.update(|cx| {
            focused
                .read(cx)
                .write(run_bytes("printf '\\033[?2004h'; cat"))
        });
        wait_until(cx, "cat waits with bracketed paste on", |cx| {
            let v = focused.read(cx);
            v.foreground_job().is_some() && v.accepts_paste()
        });
        assert_eq!(
            cx.update(|cx| panel.read(cx).paste_target(None, cx)),
            Some(0)
        );
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| p.paste(0, MARK, window, cx))
        })
        .expect("the test window is open")
        .expect("the focused pane takes the paste");
        wait_until(cx, "the paste shows in the focused pane", |cx| {
            focused.read(cx).text_tail(100).contains(MARK)
        });
        cx.update(|cx| assert!(!other.read(cx).text_tail(100).contains(MARK)));
    }

    /// Send to terminal goes to the agent in the browser's project, and never into an SSH
    /// session, whose remote shell or program turns bracketed paste on as an agent does: the
    /// session in front, an agent of another project, won before.
    #[gpui_kit::test]
    fn a_paste_goes_to_the_projects_agent_and_never_over_ssh(cx: &mut TestAppContext) {
        /// Turns bracketed paste on, then waits: a stand-in for Claude Code, or for the remote
        /// shell at its prompt.
        const AGENT: &str = "printf '\\033[?2004h'; cat";
        cx.update(init);
        let (w, panel) = open_panel(cx);
        open_tabs(cx, &panel, 1);
        let (a, b) = (
            std::path::PathBuf::from("/work/a"),
            std::path::PathBuf::from("/work/b"),
        );
        let [tab_a, tab_b] = cx.update(|cx| [0, 1].map(|ix| panel.read(cx).tab(ix).clone()));
        for (tab, project) in [(&tab_a, &a), (&tab_b, &b)] {
            cx.update(|cx| {
                tab.update(cx, |t, _| t.group = Some(project.clone()));
                tab.read(cx).focused_view().read(cx).write(run_bytes(AGENT));
            });
        }
        let ssh = super::Remote {
            launch: sawit_core::ssh::Launch {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), AGENT.into()],
                env: Vec::new(),
            },
            ..test_remote()
        };
        let (session, _) = cx
            .update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| p.open_ssh(ssh, Origin::User, window, cx))
            })
            .expect("the test window is open")
            .expect("the session starts");
        wait_until(cx, "every tab takes a paste", |cx| {
            let p = panel.read(cx);
            (0..p.tab_count()).all(|ix| p.tab(ix).read(cx).focused_view().read(cx).accepts_paste())
        });
        cx.update(|cx| {
            let p = panel.read(cx);
            let ix = |tab: &Entity<TerminalTab>| (0..p.tab_count()).find(|&ix| p.tab(ix) == tab);
            let (in_a, in_b) = (ix(&tab_a), ix(&tab_b));
            assert_eq!(p.tab_count(), 3);
            assert_eq!(p.active_index(), session, "the session is in front");
            assert_eq!(p.paste_target(Some(&a), cx), in_a, "a's agent");
            assert_eq!(p.paste_target(Some(&b), cx), in_b, "b's agent");
            assert_eq!(
                p.paste_target(None, cx),
                in_a.max(in_b),
                "the browser of no project: the last agent, not the session"
            );
        });
    }

    #[gpui_kit::test]
    fn a_script_in_a_split_is_still_found(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let tag = JobTag {
            key: "k".into(),
            name: "k".into(),
        };
        cx.update_window(w, |_, window, cx| {
            panel.update(cx, |p, cx| {
                p.start_job(
                    tag,
                    std::env::temp_dir(),
                    "true",
                    Some(Origin::User),
                    window,
                    cx,
                )
            })
        })
        .expect("the test window is open")
        .expect("the script starts");
        cx.run_until_parked();
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let (job_tab, job_pane, new_pane) = cx.update(|cx| {
            let p = panel.read(cx);
            let tab = p.tab(p.active_index()).read(cx);
            let panes = tab.state.layout.panes();
            assert_eq!(panes.len(), 2);
            assert_eq!(tab.state.focused, panes[1]);
            (p.active_index(), panes[0], panes[1])
        });
        assert_ne!(job_pane, new_pane);
        cx.update(|cx| assert!(panel.read(cx).job_status("k", cx).is_some()));
        let found = cx
            .update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| p.show_job("k", window, cx))
            })
            .expect("the test window is open");
        assert!(found);
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.active_index(), job_tab);
            assert_eq!(p.tab(job_tab).read(cx).state.focused, job_pane);
        });
    }

    #[gpui_kit::test]
    fn describe_lists_panes(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        cx.update(|cx| {
            let tabs = panel.read(cx).describe(cx);
            let panes = tabs[0]["panes"].as_array().expect("a panes list");
            assert_eq!(panes.len(), 2);
            let focused = panes.iter().filter(|p| p["focused"] == true).count();
            assert_eq!(focused, 1);
        });
    }

    /// Splits tab 0 into a row of three panes with a fourth under the last, and returns them
    /// in reading order.
    fn four_panes(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        panel: &Entity<TerminalPanel>,
    ) -> Vec<sawit_core::terminal::PaneId> {
        focus_terminal(cx, w, panel);
        cx.simulate_keystrokes(w, "cmd-d cmd-d cmd-shift-d");
        cx.run_until_parked();
        cx.update(|cx| {
            let t = panel.read(cx).tab(0).read(cx);
            let order = t.state.reading_order(t.area());
            assert_eq!(order.len(), 4);
            order
        })
    }

    /// Makes the panes of tab 0 look as if `agents` ran in them, in reading order.
    fn fake_agents(
        cx: &mut TestAppContext,
        panel: &Entity<TerminalPanel>,
        panes: &[sawit_core::terminal::PaneId],
        agents: &[Option<AgentKind>],
    ) {
        cx.update(|cx| {
            let views: Vec<_> = panes
                .iter()
                .map(|pane| panel.read(cx).tab(0).read(cx).view(*pane).unwrap().clone())
                .collect();
            for (view, agent) in views.iter().zip(agents) {
                view.update(cx, |v, cx| v.fake_agent(*agent, cx));
            }
        });
        cx.run_until_parked();
    }

    /// Where the element tests know as `name` is drawn in window `w`, if it is.
    fn bounds_of(
        cx: &mut TestAppContext,
        w: AnyWindowHandle,
        name: String,
    ) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
        let mut vcx = VisualTestContext::from_window(w, cx);
        vcx.run_until_parked();
        vcx.debug_bounds(selector(name))
    }

    /// A split tab shows every distinct agent running in its panes, focused or not, in the panes'
    /// reading order, and past three, the first two and "+N". It keeps showing them while another
    /// tab is in front.
    #[gpui_kit::test]
    fn the_tab_shows_each_agent_of_its_panes_in_reading_order(cx: &mut TestAppContext) {
        use AgentKind::*;
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let panes = four_panes(cx, w, &panel);
        let logo = |id: &str| format!("term-agent-0-{id}");

        assert!(bounds_of(cx, w, "term-agents-0".into()).is_none());
        let plain = bounds_of(cx, w, "term-tab-0".into()).unwrap();
        fake_agents(
            cx,
            &panel,
            &panes,
            &[Some(Codex), Some(ClaudeCode), Some(Codex), None],
        );
        let codex = bounds_of(cx, w, logo("codex")).expect("Codex's logo");
        let claude = bounds_of(cx, w, logo("claude-code")).expect("Claude Code's logo");
        assert!(codex.origin.x < claude.origin.x, "in reading order");
        assert!(bounds_of(cx, w, logo("more")).is_none());
        let with_logos = bounds_of(cx, w, "term-tab-0".into()).unwrap();
        assert_eq!(
            with_logos.size.height, plain.size.height,
            "the tab keeps its height"
        );
        cx.update(|cx| {
            let agents: Vec<_> = panel
                .read(cx)
                .running_agents(cx)
                .iter()
                .map(|r| r.kind)
                .collect();
            assert_eq!(agents, [Codex, ClaudeCode, Codex]);
        });

        fake_agents(
            cx,
            &panel,
            &panes,
            &[
                Some(Codex),
                Some(ClaudeCode),
                Some(GeminiCli),
                Some(Opencode),
            ],
        );
        assert!(bounds_of(cx, w, logo("codex")).is_some());
        assert!(bounds_of(cx, w, logo("claude-code")).is_some());
        assert!(bounds_of(cx, w, logo("gemini")).is_none(), "past three");
        assert!(bounds_of(cx, w, logo("opencode")).is_none());
        assert!(bounds_of(cx, w, logo("more")).is_some(), "+2");

        // In the background, behind a new tab, the agents still show.
        open_tabs(cx, &panel, 1);
        assert_ne!(cx.update(|cx| panel.read(cx).active_index()), 0);
        assert!(bounds_of(cx, w, logo("codex")).is_some());

        // Once they all exit, the terminal glyph is back.
        fake_agents(cx, &panel, &panes, &[None, None, None, None]);
        assert!(bounds_of(cx, w, "term-agents-0".into()).is_none());
    }

    /// app_state reports each pane's agent by its id, the focused pane's as the tab's own, and
    /// every distinct agent of the tab as its strip shows them.
    #[gpui_kit::test]
    fn describe_reports_the_agents(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let panes = cx.update(|cx| {
            let t = panel.read(cx).tab(0).read(cx);
            t.state.reading_order(t.area())
        });
        fake_agents(
            cx,
            &panel,
            &panes,
            &[Some(AgentKind::ClaudeCode), Some(AgentKind::Codex)],
        );
        cx.update(|cx| {
            let tabs = panel.read(cx).describe(cx);
            let tab = &tabs[0];
            // ⌘D focused the new pane, on the right.
            assert_eq!(tab["agent"], "codex");
            assert_eq!(tab["agents"], serde_json::json!(["claude-code", "codex"]));
            let agents: Vec<_> = tab["panes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["agent"].clone())
                .collect();
            assert_eq!(agents, [serde_json::json!("claude-code"), "codex".into()]);
        });
        fake_agents(cx, &panel, &panes, &[None, None]);
        cx.update(|cx| {
            let tabs = panel.read(cx).describe(cx);
            assert_eq!(tabs[0]["agent"], serde_json::Value::Null);
            assert_eq!(tabs[0]["agents"], serde_json::json!([]));
        });
    }

    /// The pane the user types in is the running agent that is `focused`, even in a tab the user
    /// has left; with no pane ever typed in, the active tab's focused pane is.
    #[gpui_kit::test]
    fn running_agents_say_which_one_the_user_types_to(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        focus_terminal(cx, w, &panel);
        cx.simulate_keystrokes(w, "cmd-d");
        cx.run_until_parked();
        let panes = cx.update(|cx| {
            let t = panel.read(cx).tab(0).read(cx);
            t.state.reading_order(t.area())
        });
        fake_agents(
            cx,
            &panel,
            &panes,
            &[Some(AgentKind::ClaudeCode), Some(AgentKind::Codex)],
        );
        let focused = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                panel
                    .read(cx)
                    .running_agents(cx)
                    .iter()
                    .filter(|r| r.focused)
                    .map(|r| (r.kind, r.label.to_string()))
                    .collect::<Vec<_>>()
            })
        };
        let typed = focused(cx);
        assert_eq!(typed.len(), 1);
        assert_eq!(
            typed[0].0,
            AgentKind::Codex,
            "⌘D gave the new pane the keyboard"
        );
        cx.simulate_keystrokes(w, "cmd-[");
        cx.run_until_parked();
        assert_eq!(focused(cx)[0].0, AgentKind::ClaudeCode);
    }

    #[gpui_kit::test]
    fn a_bad_pane_index_is_an_error(cx: &mut TestAppContext) {
        cx.update(init);
        let (_w, panel) = open_panel(cx);
        cx.update(|cx| {
            assert_eq!(
                panel.read(cx).text(Some(0), Some(5), 10, cx),
                Err("Terminal 0 has no pane 5; panes are 0 to 0.".to_string())
            );
        });
    }

    /// `run` (terminal_run without new_tab, the Tools page's Run in Terminal, a preview's Run)
    /// types into the focused pane while its shell is at its prompt, and never into a program
    /// running there: with `cat` in the foreground, the command runs in a new shell tab and `cat`
    /// gets nothing. Each command prints a word that its own line doesn't contain.
    #[gpui_kit::test]
    fn run_types_only_into_a_shell_at_its_prompt(cx: &mut TestAppContext) {
        cx.update(init);
        let (w, panel) = open_panel(cx);
        let run = |cx: &mut TestAppContext, commands: &str| {
            cx.update_window(w, |_, window, cx| {
                panel.update(cx, |p, cx| p.run(commands, Origin::User, window, cx))
            })
            .expect("the test window is open")
            .expect("a shell runs it");
        };
        let first = cx.update(|cx| panel.read(cx).tab(0).read(cx).focused_view().clone());
        wait_until(cx, "the shell is at its prompt", |cx| {
            first.read(cx).at_prompt()
        });

        run(cx, "printf 'sawit-%s\\n' idle-ran");
        cx.update(|cx| assert_eq!(panel.read(cx).tab_count(), 1));
        wait_until(cx, "the shell at its prompt runs the command", |cx| {
            first.read(cx).text_tail(50).contains("sawit-idle-ran")
        });

        run(cx, "cat");
        wait_until(cx, "cat runs in the foreground", |cx| {
            let v = first.read(cx);
            v.foreground_job().is_some() && !v.at_prompt()
        });
        run(cx, "printf 'sawit-%s\\n' busy-ran");
        let fresh = cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.tab_count(), 2, "the command opens a new shell tab");
            assert_eq!(p.active_index(), 1);
            p.tab(1).read(cx).focused_view().clone()
        });
        wait_until(cx, "the new shell runs the command", |cx| {
            fresh.read(cx).text_tail(50).contains("sawit-busy-ran")
        });
        cx.update(|cx| {
            let busy = first.read(cx);
            assert!(
                !busy.text_tail(50).contains("busy-ran"),
                "cat got the command"
            );
            assert!(busy.foreground_job().is_some(), "cat still runs");
        });
    }
}
