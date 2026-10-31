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
    Command, GridSize, PaneId, Session, SplitDir, default_shell, run_bytes, shell_quote,
};

use super::tab::{TabEvent, TerminalTab};
use super::view::TerminalView;
use super::{SplitDown, SplitRight};
use crate::states::{Banner, Placeholder};
use crate::tab_groups::{group_color, group_header, group_stripe, project_name};
use crate::tabs::{Slot, insert_index, project_of};

pub struct TerminalPanel {
    tabs: Vec<Entity<TerminalTab>>,
    active: usize,
    /// The active project's folder: where new terminals start.
    cwd: Option<PathBuf>,
    /// Every project open in the window, in switcher order.
    projects: Vec<PathBuf>,
    /// Groups whose tabs are folded away.
    collapsed: HashSet<PathBuf>,
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
        let groups: Vec<Option<PathBuf>> =
            self.tabs.iter().map(|t| t.read(cx).group.clone()).collect();
        let slots: Vec<Slot> = groups
            .iter()
            .map(|g| Slot {
                pinned: false,
                group: g.as_deref(),
            })
            .collect();
        insert_index(&slots, false, group)
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
    /// even between runs. The pane shows as `origin` allows. Returns the tab
    /// and pane it typed into, or why no shell could start.
    pub fn run(
        &mut self,
        commands: &str,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        let idle = |v: &Entity<TerminalView>| v.read(cx).idle_shell();
        let in_active = self.tabs.get(self.active).and_then(|t| {
            let t = t.read(cx);
            idle(t.focused_view()).then_some((self.active, t.state.focused))
        });
        let target = in_active.or_else(|| {
            self.all_panes(cx)
                .filter(|(_, _, v)| idle(v))
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

    /// Every tab, for `sawit-mcp`: the tab's own fields describe its focused
    /// pane, and `panes` lists each pane in layout order.
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
                            "script": v.job.as_ref().map(|j| j.name.clone()),
                            "ssh": v.remote.as_ref().map(|r| r.name.clone()),
                        })
                    })
                    .collect();
                serde_json::json!({
                    "index": ix,
                    "title": t.title(cx).to_string(),
                    "active": ix == self.active,
                    "exited": v.exited,
                    "shell": v.shell,
                    "at_prompt": v.at_prompt(),
                    "running_job": v.foreground_job().is_some(),
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

    /// The pane's last `lines` lines of output (the active tab's focused
    /// pane by default).
    pub fn text(
        &self,
        tab: Option<usize>,
        pane: Option<usize>,
        lines: usize,
        cx: &App,
    ) -> Result<String, String> {
        let ix = self.tab_ix(tab)?;
        let (_, view) = self.pane_of(ix, pane, cx)?;
        Ok(view.read(cx).text_tail(lines))
    }

    /// Sends raw input to a pane (the active tab's focused one by default).
    /// The pane counts as used from then on, as when the user types there.
    pub fn input(
        &mut self,
        tab: Option<usize>,
        pane: Option<usize>,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let ix = self.tab_ix(tab)?;
        let (_, view) = self.pane_of(ix, pane, cx)?;
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

    /// Types `command` into the script's terminal: its idle pane when there
    /// is one (a running one is only shown), else a new shell in `dir`. The
    /// old shell may have moved since, and the command is relative to the
    /// script's folder, so it goes back to `dir` first. With `show`, the
    /// terminal shows as that origin allows (one of several started together
    /// passes `None`).
    pub fn start_job(
        &mut self,
        tag: JobTag,
        dir: PathBuf,
        command: &str,
        show: Option<Origin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                let Ok((ix, pane)) = self.open_tab(cmd, &tag.name, None, cx) else {
                    cx.notify();
                    return;
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

    /// Splits the active tab's focused pane with a new shell. The shell
    /// starts in the focused pane's folder if it still exists, else in the
    /// tab's project, else in `$HOME`.
    pub fn split_active(&mut self, dir: SplitDir, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active).cloned() else {
            return;
        };
        let (folder, group) = {
            let t = tab.read(cx);
            (t.focused_view().read(cx).cwd(), t.group.clone())
        };
        let cmd = Command {
            cwd: folder.filter(|d| d.is_dir()).or(group).or_else(home),
            ..Command::default()
        };
        if let Some(view) = self.spawn_view(cmd, &shell_name(), None, cx) {
            tab.update(cx, |t, cx| t.add_pane(dir, view, window, cx));
        }
        cx.notify();
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
                let v = view.read(cx);
                if v.exited {
                    continue;
                }
                let named = t.state.panes.get(&pane).and_then(|m| m.name.clone());
                if v.remote.is_some() {
                    running.push(format!("{} (SSH)", t.pane_label(pane, v)));
                } else if v.foreground_job().is_some() {
                    let name = named
                        .or_else(|| v.job.as_ref().map(|j| j.name.clone()))
                        .or_else(|| v.foreground_name())
                        .unwrap_or_else(|| v.title.to_string());
                    running.push(name);
                }
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

    /// The tab a paste may go to: the active one if the program in its
    /// focused pane enabled bracketed paste, else the highest-numbered tab
    /// whose focused pane did.
    pub fn paste_target(&self, cx: &App) -> Option<usize> {
        let accepts: Vec<bool> = self
            .tabs
            .iter()
            .map(|t| focused(t, cx).accepts_paste())
            .collect();
        pick_paste_target(self.active, &accepts)
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
        let mut tabs: Vec<AnyElement> = Vec::new();
        for (ix, tab) in self.tabs.iter().enumerate() {
            let t = tab.read(cx);
            let v = t.focused_view().read(cx);
            let group = t.group.clone();
            let color = group.as_ref().map(|g| {
                let at = self.projects.iter().position(|p| p == g).unwrap_or(0);
                group_color(at, cx)
            });
            // A header in front of each group's run of tabs.
            let starts_run = ix == 0 || self.tabs[ix - 1].read(cx).group != group;
            if let (Some(root), Some(color), true) = (&group, color, starts_run) {
                let collapsed = self.collapsed.contains(root);
                let count = self.tabs[ix..]
                    .iter()
                    .take_while(|w| w.read(cx).group == group)
                    .count();
                let root = root.clone();
                tabs.push(
                    group_header(
                        ("term-group", ix),
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
                    .into_any_element(),
                );
            }
            let active = ix == self.active;
            // A folded group still shows its active tab.
            if !active && group.as_ref().is_some_and(|g| self.collapsed.contains(g)) {
                continue;
            }
            let others: Vec<PathBuf> = self
                .projects
                .iter()
                .filter(|p| Some(*p) != group.as_ref())
                .cloned()
                .collect();
            let weak = weak.clone();
            let this_tab = tab.downgrade();
            let grouped = group.is_some();
            // A one-pane tab is renamed in its label.
            let rename = (!t.state.is_split())
                .then(|| t.rename_field(tab.downgrade(), t.state.focused))
                .flatten();
            let named = t.pane_name();
            tabs.push({
                // SSH tabs keep the server's name (the remote shell's title
                // says little across servers), numbered when it is open twice.
                // A name the user gave the pane goes over either.
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
                        let name = t.pane_label(t.state.focused, v);
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
                h_flex()
                    .id(("term-tab", ix))
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
                    .child(Icon::new(icon).xsmall().when(
                        (v.remote.is_some() && !v.exited)
                            || (v.job.is_some() && v.foreground_job().is_some()),
                        |i| i.text_color(cx.theme().success),
                    ))
                    .map(|el| match rename {
                        Some(field) => el.child(field),
                        None => el.child(label),
                    })
                    .child(
                        Button::new(("close-term", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
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
                        move |this, _, window, cx| {
                            if let Some(ix) = this.index_of(&tab) {
                                this.activate(ix, window, cx)
                            }
                        }
                    }))
                    .context_menu(move |menu, window, cx| {
                        // The menu has the keyboard from here on: whether the tab had it
                        // before decides where it goes when the tab closes.
                        let had_keyboard = this_tab
                            .upgrade()
                            .is_some_and(|t| t.read(cx).has_focus(window, cx));
                        let move_weak = weak.clone();
                        let move_tab = this_tab.clone();
                        let others = others.clone();
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
                            .h_full()
                            .flex_1()
                            .overflow_x_scroll()
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

/// The tab to paste into: `active` if it accepts, else the last that does.
fn pick_paste_target(active: usize, accepts: &[bool]) -> Option<usize> {
    if accepts.get(active) == Some(&true) {
        return Some(active);
    }
    accepts.iter().rposition(|&a| a)
}

#[cfg(test)]
mod helper_tests {
    use super::{command_in, pick_paste_target};

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
        assert_eq!(pick_paste_target(1, &[true, false, true]), Some(2));
        assert_eq!(pick_paste_target(0, &[true, false]), Some(0));
        assert_eq!(pick_paste_target(0, &[false, false]), None);
        assert_eq!(pick_paste_target(0, &[]), None);
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, TestAppContext,
        VisualTestContext,
    };
    use sawit_core::terminal::{run_bytes, shell_quote};

    use super::super::tab::TerminalTab;
    use super::super::testing::{
        focus_terminal, init, open_panel, ready, scratch_dir, start, start_cat, wait_until,
    };
    use super::{JobTag, Origin, TerminalPanel};

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
        .expect("the test window is open");
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
        let _ = std::fs::remove_dir_all(&project);
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
            .expect("the test window is open");
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
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&elsewhere);
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
        .expect("the test window is open");
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
        .expect("the test window is open");
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
        assert_eq!(cx.update(|cx| panel.read(cx).paste_target(cx)), Some(0));
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
        .expect("the test window is open");
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
