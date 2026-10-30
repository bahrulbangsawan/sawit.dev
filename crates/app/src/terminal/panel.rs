//! Tabs of terminals in the bottom dock, with SSH sessions that
//! reconnect with one click. Tabs are grouped by the
//! window's project they started in.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::ssh::Launch;
use sawit_core::terminal::{
    Command, GridSize, PaneId, Session, SplitDir, default_shell, run_bytes,
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
}

/// Asks the workspace to fill the window with the panel, or to restore.
pub struct ToggleMaximize;

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
                .filter(|(_, _, v)| {
                    // `ssh` itself is "at the prompt"; the folder is local anyway.
                    let v = v.read(cx);
                    v.remote.is_none() && v.job.is_none() && v.at_prompt()
                })
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

    /// Moves the tab at `ix` into `group` (or out of any).
    fn move_to_group(&mut self, ix: usize, group: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(ix) {
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

    /// Opens a new session to `remote` in its own tab.
    pub fn open_ssh(
        &mut self,
        remote: Remote,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        sawit_core::ssh::ensure_ssh_dir();
        let name = remote.name.clone();
        self.open(ssh_command(&remote.launch), &name, Some(remote), window, cx);
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

    /// Shows the newest live session to `key`, in whichever pane it is;
    /// false when there is none.
    pub fn focus_ssh(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((ix, pane)) = self.ssh_panes(key, cx).last().copied() else {
            return false;
        };
        self.show_pane(ix, pane, window, cx);
        true
    }

    /// Connects the ended SSH session in `pane` of the tab at `ix` again,
    /// in place, and focuses it. The tab's other panes are left alone.
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
        let Some(remote) = tab
            .read(cx)
            .view(pane)
            .and_then(|v| v.read(cx).remote.clone())
        else {
            return;
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
    /// focused pane if it is a live shell, else the newest live shell pane
    /// in any tab, else a new one.
    pub fn run(&mut self, commands: &str, window: &mut Window, cx: &mut Context<Self>) {
        let live_shell = |v: &Entity<TerminalView>| {
            let v = v.read(cx);
            v.shell && !v.exited
        };
        let in_active = self.tabs.get(self.active).and_then(|t| {
            let t = t.read(cx);
            live_shell(t.focused_view()).then_some((self.active, t.state.focused))
        });
        let target = in_active.or_else(|| {
            self.all_panes(cx)
                .filter(|(_, _, v)| live_shell(v))
                .map(|(ix, pane, _)| (ix, pane))
                .last()
        });
        let (ix, pane) = match target {
            Some(found) => found,
            None => {
                self.open_shell(None, cx);
                if self.error.is_some() {
                    return;
                }
                (self.active, self.tabs[self.active].read(cx).state.focused)
            }
        };
        if let Some(view) = self.tabs[ix].read(cx).view(pane).cloned() {
            view.update(cx, |view, _| {
                view.touched = true;
                view.write(run_bytes(commands));
            });
        }
        self.show_pane(ix, pane, window, cx);
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
    pub fn input(
        &mut self,
        tab: Option<usize>,
        pane: Option<usize>,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let ix = self.tab_ix(tab)?;
        let (_, view) = self.pane_of(ix, pane, cx)?;
        let view = view.read(cx);
        if view.exited {
            return Err(format!("Terminal {ix} has exited."));
        }
        view.write(bytes);
        cx.notify();
        Ok(ix)
    }

    /// Closes the tab at `ix`, or only its pane at position `pane`.
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
                self.tabs[ix].update(cx, |t, cx| t.close_pane(id, window, cx));
            }
        }
        Ok(())
    }

    /// Runs `commands` in a new shell tab started in `dir`; returns its index.
    pub fn run_in_new(
        &mut self,
        dir: Option<PathBuf>,
        commands: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let cmd = Command {
            cwd: dir,
            ..Command::default()
        };
        self.open(cmd, &shell_name(), None, Some(window), cx);
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let ix = self.active;
        let view = self.tabs[ix].read(cx).focused_view().clone();
        view.update(cx, |view, _| {
            view.touched = true;
            view.write(run_bytes(commands));
        });
        Ok(ix)
    }

    /// The index of the tab `run` typed into last.
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
    /// is one (a running one is only shown), else a new shell in `dir`.
    pub fn start_job(
        &mut self,
        tag: JobTag,
        dir: PathBuf,
        command: &str,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let (ix, pane) = match self.job_panes(&tag.key, cx).last().copied() {
            Some(found) => found,
            None => {
                let cmd = Command {
                    cwd: Some(dir),
                    ..Command::default()
                };
                let Some(tab) = self.spawn(cmd, &tag.name, None, cx) else {
                    cx.notify();
                    return;
                };
                let pane = tab.read(cx).state.focused;
                (self.insert(tab, cx), pane)
            }
        };
        self.active = ix;
        self.tabs[ix].update(cx, |t, _| t.state.focus(pane));
        if let Some(view) = self.tabs[ix].read(cx).view(pane).cloned() {
            view.update(cx, |view, _| {
                view.job = Some(tag);
                if view.foreground_job().is_none() {
                    view.touched = true;
                    view.write(run_bytes(command));
                }
            });
        }
        if let Some(window) = window {
            self.focus_active(window, cx);
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

    fn open(
        &mut self,
        cmd: Command,
        title: &str,
        remote: Option<Remote>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.spawn(cmd, title, remote, cx) {
            self.active = self.insert(tab.clone(), cx);
            if let Some(window) = window {
                tab.update(cx, |t, cx| t.focus(window, cx));
            }
        }
        cx.notify();
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

    fn close(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.tabs.remove(ix);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
            cx.notify();
        }
    }

    /// Drops a tab whose last pane closed, redraws the strip on changes,
    /// and does what a pane's banner asks.
    fn on_tab_event(&mut self, tab: Entity<TerminalTab>, event: &TabEvent, cx: &mut Context<Self>) {
        match event {
            TabEvent::Closed => {
                if let Some(ix) = self.tabs.iter().position(|t| *t == tab) {
                    self.close(ix, cx);
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
                        let name = named
                            .or(v.job.as_ref().map(|j| j.name.as_str()))
                            .unwrap_or_default();
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
                            .on_click(cx.listener(move |this, _, _, cx| this.close(ix, cx))),
                    )
                    .when_some(color, |el, color| el.child(group_stripe(color, active)))
                    .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
                    .context_menu(move |menu, window, cx| {
                        let move_weak = weak.clone();
                        let others = others.clone();
                        let menu = menu.when(!others.is_empty(), |menu| {
                            menu.submenu("Move to Project", window, cx, move |mut menu, _, _| {
                                for root in &others {
                                    let weak = move_weak.clone();
                                    let to = root.clone();
                                    menu =
                                        menu.item(PopupMenuItem::new(project_name(root)).on_click(
                                            move |_, _, cx| {
                                                let _ = weak.update(cx, |t, cx| {
                                                    t.move_to_group(ix, Some(to.clone()), cx)
                                                });
                                            },
                                        ));
                                }
                                menu
                            })
                        });
                        let ungroup = weak.clone();
                        let close = weak.clone();
                        menu.when(grouped, |menu| {
                            menu.item(PopupMenuItem::new("Remove from Group").on_click(
                                move |_, _, cx| {
                                    let _ =
                                        ungroup.update(cx, |t, cx| t.move_to_group(ix, None, cx));
                                },
                            ))
                        })
                        .separator()
                        .item(PopupMenuItem::new("Close").on_click(move |_, _, cx| {
                            let _ = close.update(cx, |t, cx| t.close(ix, cx));
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

/// Runs `launch` in the terminal.
fn ssh_command(launch: &Launch) -> Command {
    Command {
        program: launch.program.clone(),
        args: launch.args.clone(),
        cwd: None,
        env: launch.env.iter().cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::super::testing::{focus_terminal, init, open_panel};
    use super::JobTag;

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
                p.start_job(tag, std::env::temp_dir(), "true", Some(window), cx)
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
}
