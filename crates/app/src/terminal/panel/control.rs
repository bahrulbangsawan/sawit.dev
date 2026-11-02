//! What agents ask of the terminals over the control socket, and what the app's Run in Terminal
//! does: type commands into a shell, read a pane, send it keys, describe the tabs and close them,
//! each by tab and pane.

use std::path::PathBuf;

use gpui_kit::{App, Context, Entity, Focusable as _, Window};
use sawit_core::terminal::{Command, PaneId, run_bytes};

use super::open::shell_name;
use super::running::running_in;
use super::{Origin, TerminalPanel};
use crate::terminal::agents;
use crate::terminal::view::TerminalView;

impl TerminalPanel {
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
    pub(super) fn target(
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

    /// Names the tab at `ix`, or its pane at position `pane` (see
    /// [`TerminalTab::set_name`](crate::terminal::tab::TerminalTab::set_name)):
    /// `terminal_rename`'s answer, with the name the strip or the pane's header shows after.
    pub fn rename(
        &mut self,
        ix: usize,
        pane: Option<usize>,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        let ix = self.tab_ix(Some(ix))?;
        let id = match pane {
            Some(_) => Some(self.pane_of(ix, pane, cx)?.0),
            None => None,
        };
        let shown = self.tabs[ix].update(cx, |t, cx| t.set_name(id, name, cx));
        cx.notify();
        let named = if id.is_some() && self.tabs[ix].read(cx).state.is_split() {
            "pane"
        } else {
            "tab"
        };
        Ok(serde_json::json!({
            "terminal": ix,
            "pane": pane,
            "named": named,
            "name": shown.to_string(),
        }))
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
}
