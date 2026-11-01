//! Starting terminals: shell tabs, SSH sessions (and connecting an ended one again), and the
//! shells split off a pane, with ⌘D or for an agent.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{App, AppContext as _, Context, Entity, Window};
use sawit_core::ssh::Launch;
use sawit_core::terminal::{
    Command, GridSize, MIN_COLS, MIN_ROWS, PaneId, Rect, Session, SplitDir, default_shell,
    run_bytes,
};

use super::{Origin, Remote, TerminalPanel};
use crate::tabs::project_of;
use crate::terminal::tab::{TerminalTab, Unsplit};
use crate::terminal::view::TerminalView;

impl TerminalPanel {
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
    pub(in crate::terminal) fn reconnect(
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

    /// Starts `cmd` in a new tab and makes it the active one, giving it the
    /// keyboard when there is a `window`.
    pub(super) fn open(
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
    pub(super) fn open_tab(
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
    pub(super) fn spawn_view(
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

/// The user's home folder.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Tab title for the default shell ("zsh", "bash", "powershell").
pub(super) fn shell_name() -> String {
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
