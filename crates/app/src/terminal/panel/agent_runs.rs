//! Agents' `terminal_run`: which terminal a command goes to, so an agent running fifty commands
//! leaves one or two tabs rather than fifty. Each run is remembered by a number, with the agent
//! session that sent it and its terminal: the agent's next command goes to its own latest idle
//! terminal, and a one-off tab can close itself once its command is done, its output kept for
//! `terminal_read` by the run's number for a while.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::{App, Context, Entity, Focusable as _, WeakEntity, Window};
use sawit_core::terminal::{PaneId, run_bytes};

use super::{Origin, TerminalPanel};
use crate::terminal::view::TerminalView;

/// How many runs are remembered (with the output of the closed ones).
const KEPT: usize = 64;
/// The lines a closed one-off tab's output keeps.
const KEPT_LINES: usize = 5_000;
/// How often a one-off tab looks whether its command is done, and for how long at most.
const POLL: Duration = Duration::from_millis(500);
const WATCH_FOR: Duration = Duration::from_secs(30 * 60);
/// A one-off tab's command is done once its shell has been back at its prompt this long after
/// the command was seen running...
const DONE_AFTER: Duration = Duration::from_millis(600);
/// ...or, for a command too quick to be seen running (looks are half a second apart), once the
/// shell has been quiet at its prompt this long. A new shell can take a second or two to start
/// and read the command typed into it, and is at its prompt meanwhile.
const QUIET: Duration = Duration::from_secs(3);

/// What the looks at a one-off tab have seen.
#[derive(Default)]
struct Watch {
    /// Something ran in front of the shell: the command, or the shell's start.
    seen_busy: bool,
    /// Since when the shell has been at its prompt with nothing in front.
    idle_since: Option<Instant>,
}

/// One `terminal_run`.
pub struct AgentRun {
    pub id: u64,
    /// The agent session that sent it (its id), when the op said.
    pub agent: Option<String>,
    pub view: WeakEntity<TerminalView>,
    /// The tab closes itself once the command is done.
    pub closes: bool,
    /// What the pane showed when it closed.
    pub output: Option<String>,
    pub started: Instant,
}

/// The runs agents made in this window's terminals, newest last.
#[derive(Default)]
pub struct AgentRuns {
    next: u64,
    runs: VecDeque<AgentRun>,
}

impl AgentRuns {
    fn add(&mut self, mut run: AgentRun) -> u64 {
        self.next += 1;
        run.id = self.next;
        self.runs.push_back(run);
        while self.runs.len() > KEPT {
            self.runs.pop_front();
        }
        self.next
    }

    fn get(&self, id: u64) -> Option<&AgentRun> {
        self.runs.iter().find(|r| r.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut AgentRun> {
        self.runs.iter_mut().find(|r| r.id == id)
    }
}

/// Where an agent's command went.
pub struct Ran {
    pub tab: usize,
    pub pane: PaneId,
    pub run: u64,
    /// An idle terminal was used again rather than a new one.
    pub reused: bool,
    /// The tab closes itself when the command is done.
    pub closes: bool,
}

/// The shell line that runs `command` in `dir`: `cd` there first.
pub fn in_folder(dir: &Path, command: &str) -> String {
    format!(
        "cd {} && {command}",
        sawit_core::package_manager::quote(&dir.to_string_lossy())
    )
}

impl TerminalPanel {
    /// An agent's `terminal_run`. Unless `new_tab`, the agent's own most recent terminal that is
    /// idle goes first (with a `cd` when it is in another folder than `cwd`); else, without a
    /// `cwd`, an idle shell as [`Self::run`] picks it; else a new tab in `cwd`, or in `new_dir`
    /// (the agent's project). Never a terminal the user is typing in, a locked or pinned one, or
    /// one running something (a dev server, a script's terminal, an SSH session). With `close`,
    /// a tab opened for the command closes itself once the command is done.
    #[allow(clippy::too_many_arguments)]
    pub fn run_for_agent(
        &mut self,
        command: &str,
        cwd: Option<PathBuf>,
        new_dir: Option<PathBuf>,
        new_tab: bool,
        close: bool,
        agent: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Ran, String> {
        // An op without an identity (an older `sawit-mcp`) counts as one anonymous agent.
        let own = (!new_tab)
            .then(|| self.own_idle(agent, window, cx))
            .flatten();
        let (ix, pane, view, reused, opened) = if let Some((ix, pane, view)) = own {
            let line = match &cwd {
                Some(dir) if view.read(cx).cwd().as_deref() != Some(dir.as_path()) => {
                    in_folder(dir, command)
                }
                _ => command.to_string(),
            };
            view.update(cx, |v, _| {
                v.touched = true;
                v.write(run_bytes(&line));
            });
            self.reveal(ix, pane, Origin::Agent, window, cx);
            (ix, pane, view, true, false)
        } else if cwd.is_none() && !new_tab {
            let before = self.tabs.len();
            let (ix, pane) = self.run(command, Origin::Agent, window, cx)?;
            let opened = self.tabs.len() > before;
            let view = self.view_at(ix, pane, cx)?;
            (ix, pane, view, !opened, opened)
        } else {
            let (ix, pane) =
                self.run_in_new(cwd.or(new_dir), command, Origin::Agent, window, cx)?;
            let view = self.view_at(ix, pane, cx)?;
            (ix, pane, view, false, true)
        };
        let closes = close && opened;
        let run = self.agent_runs.add(AgentRun {
            id: 0,
            agent: agent.map(String::from),
            view: view.downgrade(),
            closes,
            output: None,
            started: Instant::now(),
        });
        if closes {
            self.close_when_done(run, cx);
        }
        Ok(Ran {
            tab: ix,
            pane,
            run,
            reused,
            closes,
        })
    }

    fn view_at(&self, ix: usize, pane: PaneId, cx: &App) -> Result<Entity<TerminalView>, String> {
        self.tabs
            .get(ix)
            .and_then(|t| t.read(cx).view(pane).cloned())
            .ok_or_else(|| "The terminal closed at once.".to_string())
    }

    /// The agent's own most recent terminal that is idle now, where the tab and pane are now.
    fn own_idle(
        &self,
        agent: Option<&str>,
        window: &Window,
        cx: &App,
    ) -> Option<(usize, PaneId, Entity<TerminalView>)> {
        self.agent_runs
            .runs
            .iter()
            .rev()
            .filter(|r| r.agent.as_deref() == agent && !r.closes)
            .filter_map(|r| r.view.upgrade())
            .find_map(|view| {
                let (ix, pane, _) = self
                    .all_panes(cx)
                    .find(|(_, _, v)| v.entity_id() == view.entity_id())?;
                let tab = self.tabs[ix].read(cx);
                let v = view.read(cx);
                let free = v.idle_shell()
                    && !v.exited
                    && !tab.state.locked
                    && !tab.state.pinned
                    && !view.focus_handle(cx).contains_focused(window, cx);
                free.then(|| (ix, pane, view.clone()))
            })
    }

    /// Watches run `id`'s tab and closes it once its command is done, keeping its output.
    /// A tab the user locked, pinned, split or typed in meanwhile stays.
    fn close_when_done(&mut self, id: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut watch = Watch::default();
            loop {
                cx.background_executor().timer(POLL).await;
                let done = this.update(cx, |this, cx| this.try_close(id, &mut watch, cx));
                if !matches!(done, Ok(false)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look at run `id`: `true` once there is nothing more to watch (closed, or kept).
    fn try_close(&mut self, id: u64, watch: &mut Watch, cx: &mut Context<Self>) -> bool {
        let Some(run) = self.agent_runs.get(id) else {
            return true;
        };
        if run.started.elapsed() > WATCH_FOR {
            return true;
        }
        let Some(view) = run.view.upgrade() else {
            return true;
        };
        let Some((ix, _, _)) = self
            .all_panes(cx)
            .find(|(_, _, v)| v.entity_id() == view.entity_id())
        else {
            return true;
        };
        let tab = self.tabs[ix].read(cx);
        if tab.state.locked || tab.state.pinned || tab.views().count() > 1 {
            return true;
        }
        let v = view.read(cx);
        let idle = v.exited || (v.at_prompt() && v.foreground_job().is_none());
        if !idle {
            watch.seen_busy = true;
            watch.idle_since = None;
            return false;
        }
        let since = *watch.idle_since.get_or_insert_with(Instant::now);
        let wait = if watch.seen_busy { DONE_AFTER } else { QUIET };
        if since.elapsed() < wait {
            return false;
        }
        let output = v.text_tail(KEPT_LINES);
        if let Some(run) = self.agent_runs.get_mut(id) {
            run.output = Some(output);
        }
        self.close(ix, cx);
        true
    }

    /// The text of run `id`'s terminal, `lines` at most: its pane while it is open, else what it
    /// showed when it closed.
    pub fn run_text(&self, id: u64, lines: usize, cx: &App) -> Result<String, String> {
        let run = self.agent_runs.get(id).ok_or_else(|| {
            format!(
                "There is no run {id} in this window: runs are kept for the last {KEPT} commands."
            )
        })?;
        if let Some(view) = run.view.upgrade()
            && self
                .all_panes(cx)
                .any(|(_, _, v)| v.entity_id() == view.entity_id())
        {
            return Ok(view.read(cx).text_tail(lines));
        }
        match &run.output {
            Some(text) => {
                let tail: Vec<&str> = text.lines().collect();
                let start = tail.len().saturating_sub(lines);
                Ok(format!(
                    "(run {id}'s terminal closed when its command was done)\n{}",
                    tail[start..].join("\n")
                ))
            }
            None => Err(format!("Run {id}'s terminal was closed.")),
        }
    }
}
