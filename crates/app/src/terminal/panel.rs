//! Tabs of terminals in the bottom dock, with quick launch for herdr and
//! SSH sessions that reconnect with one click. Tabs are grouped by the
//! window's project they started in.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::probe::find_in_path;
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
            for ix in 0..self.tabs.len() {
                let view = focused(&self.tabs[ix], cx);
                // `ssh` itself is "at the prompt"; the folder is local anyway.
                if view.remote.is_some() || view.job.is_some() || !view.at_prompt() {
                    continue;
                }
                if view.touched {
                    view.cd(&dir);
                    self.tabs[ix].update(cx, |t, _| t.group = Some(dir.clone()));
                } else if let Some(fresh) = self.spawn(Command::default(), &shell_name(), None, cx)
                {
                    self.tabs[ix] = fresh;
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

    /// Whether herdr is on PATH. Render asks every frame, so the PATH scan
    /// is cached briefly; installing herdr still shows up within seconds.
    pub fn herdr_available() -> bool {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static CHECKED: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
        let mut checked = CHECKED.lock().unwrap_or_else(|e| e.into_inner());
        match *checked {
            Some((at, found)) if at.elapsed() < Duration::from_secs(10) => found,
            _ => {
                let found = find_in_path("herdr").is_some();
                *checked = Some((Instant::now(), found));
                found
            }
        }
    }

    pub fn open_herdr(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let cmd = Command {
            program: "herdr".into(),
            ..Command::default()
        };
        self.open(cmd, "herdr", None, window, cx);
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

    /// Live sessions to the server with `key`.
    pub fn ssh_sessions(&self, key: &str, cx: &App) -> usize {
        self.tabs
            .iter()
            .filter(|t| {
                let v = focused(t, cx);
                !v.exited && v.remote.as_ref().is_some_and(|r| r.key == key)
            })
            .count()
    }

    /// Shows the newest live session to `key`; false when there is none.
    pub fn focus_ssh(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let found = self.tabs.iter().rposition(|t| {
            let v = focused(t, cx);
            !v.exited && v.remote.as_ref().is_some_and(|r| r.key == key)
        });
        if let Some(ix) = found {
            self.active = ix;
            self.focus_active(window, cx);
            cx.notify();
        }
        found.is_some()
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

    /// Types `commands` into a shell and presses Enter: the active tab if it
    /// is a live shell, else the newest live shell, else a new one.
    pub fn run(&mut self, commands: &str, window: &mut Window, cx: &mut Context<Self>) {
        let live_shell = |tab: &Entity<TerminalTab>| {
            let v = focused(tab, cx);
            v.shell && !v.exited
        };
        let target = self
            .tabs
            .get(self.active)
            .filter(|v| live_shell(v))
            .map(|_| self.active)
            .or_else(|| self.tabs.iter().rposition(live_shell));
        match target {
            Some(ix) => self.active = ix,
            None => {
                self.open_shell(None, cx);
                if self.error.is_some() {
                    return;
                }
            }
        }
        let view = self.tabs[self.active].read(cx).focused_view().clone();
        view.update(cx, |view, _| {
            view.touched = true;
            view.write(run_bytes(commands));
        });
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Every tab, for `sawit-mcp`: title, state, and what it runs.
    pub fn describe(&self, cx: &App) -> Vec<serde_json::Value> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let t = tab.read(cx);
                let v = t.focused_view().read(cx);
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

    /// The tab's last `lines` lines of output (the active tab by default).
    pub fn text(&self, tab: Option<usize>, lines: usize, cx: &App) -> Result<String, String> {
        let ix = self.tab_ix(tab)?;
        Ok(focused(&self.tabs[ix], cx).text_tail(lines))
    }

    /// Sends raw input to a tab (the active one by default).
    pub fn input(
        &mut self,
        tab: Option<usize>,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) -> Result<usize, String> {
        let ix = self.tab_ix(tab)?;
        let view = focused(&self.tabs[ix], cx);
        if view.exited {
            return Err(format!("Terminal {ix} has exited."));
        }
        view.write(bytes);
        cx.notify();
        Ok(ix)
    }

    pub fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> Result<(), String> {
        self.tab_ix(Some(ix))?;
        self.close(ix, cx);
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

    /// Live tabs running the script with `key`, oldest first.
    fn job_tabs(&self, key: &str, cx: &App) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&ix| {
                let v = focused(&self.tabs[ix], cx);
                !v.exited && v.job.as_ref().is_some_and(|j| j.key == key)
            })
            .collect()
    }

    /// The newest live terminal of the script with `key`, if one is open.
    pub fn job_status(&self, key: &str, cx: &App) -> Option<JobStatus> {
        let ix = *self.job_tabs(key, cx).last()?;
        Some(JobStatus {
            running: focused(&self.tabs[ix], cx).foreground_job(),
        })
    }

    /// Types `command` into the script's terminal: its idle tab when there
    /// is one (a running one is only shown), else a new shell in `dir`.
    pub fn start_job(
        &mut self,
        tag: JobTag,
        dir: PathBuf,
        command: &str,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let existing = self.job_tabs(&tag.key, cx).last().copied();
        let ix = match existing {
            Some(ix) => ix,
            None => {
                let cmd = Command {
                    cwd: Some(dir),
                    ..Command::default()
                };
                let Some(tab) = self.spawn(cmd, &tag.name, None, cx) else {
                    cx.notify();
                    return;
                };
                self.insert(tab, cx)
            }
        };
        self.active = ix;
        let view = self.tabs[ix].read(cx).focused_view().clone();
        view.update(cx, |view, _| {
            view.job = Some(tag);
            if view.foreground_job().is_none() {
                view.touched = true;
                view.write(run_bytes(command));
            }
        });
        if let Some(window) = window {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Interrupts (Ctrl-C) whatever the script's terminals are running.
    pub fn stop_job(&mut self, key: &str, cx: &mut Context<Self>) {
        for ix in self.job_tabs(key, cx) {
            let view = focused(&self.tabs[ix], cx);
            if view.foreground_job().is_some() {
                view.write(vec![0x03]);
            }
        }
        cx.notify();
    }

    /// Shows the newest terminal of the script; false when there is none.
    pub fn show_job(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.job_tabs(key, cx).last().copied() else {
            return false;
        };
        self.active = ix;
        self.focus_active(window, cx);
        cx.notify();
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
    fn render_empty(&self, herdr: bool, cx: &mut Context<Self>) -> Placeholder {
        let shell = Button::new("empty-new-terminal")
            .small()
            .icon(IconName::Plus)
            .label("New Terminal")
            .on_click(cx.listener(|this, _, window, cx| this.open_shell(Some(window), cx)));
        let state = match &self.error {
            Some(e) => Placeholder::error("Unable to open a terminal")
                .detail(e.clone())
                .action(shell.label("Try Again")),
            None => Placeholder::new(Lucide::SquareTerminal, "No terminals open")
                .detail(if herdr {
                    "Start a shell in the project folder, or herdr for split panes."
                } else {
                    "Start a shell in the project folder."
                })
                .action(shell.primary()),
        };
        state.when(self.error.is_none(), |state| {
            state.action(if herdr {
                Button::new("empty-open-herdr")
                    .small()
                    .child(herdr_logo())
                    .child("Open herdr")
                    .on_click(cx.listener(|this, _, window, cx| this.open_herdr(Some(window), cx)))
            } else {
                install_herdr_button("empty-install-herdr").small()
            })
        })
    }
}

/// The herdr logo, sized for a button.
fn herdr_logo() -> impl IntoElement {
    img(crate::assets::HERDR_LOGO).size(px(14.)).rounded(px(3.))
}

/// Opens herdr.dev, which has the install steps (Sawit does not bundle herdr).
fn install_herdr_button(id: &'static str) -> Button {
    Button::new(id)
        .child(herdr_logo())
        .child("Install herdr")
        .tooltip("herdr adds split panes and sessions. Opens herdr.dev.")
        .on_click(|_, _, cx| cx.open_url("https://herdr.dev"))
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
        let herdr = Self::herdr_available();
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
                            .child(if herdr {
                                Button::new("open-herdr")
                                    .ghost()
                                    .xsmall()
                                    .child(herdr_logo())
                                    .child("herdr")
                                    .tooltip("Open herdr in a New Tab")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_herdr(Some(window), cx)
                                    }))
                            } else {
                                install_herdr_button("install-herdr").ghost().xsmall()
                            })
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
                        None => el.child(self.render_empty(herdr, cx)),
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
