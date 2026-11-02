//! The terminal dock: the Run panel's scripts, commands run for an agent, new shells and SSH
//! sessions.

use super::*;

impl Workspace {
    /// Starts or shows scripts from the Run panel, showing the terminal.
    pub(super) fn on_run_event(
        &mut self,
        event: &RunEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let RunEvent::Browse(url) = event {
            self.open_browser(Some(url.clone()), window, cx);
            return;
        }
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        match event {
            // A terminal that can't start says why in the dock.
            RunEvent::Start(launches, origin) => {
                let _ = self.start_scripts(launches, *origin, window, cx);
            }
            RunEvent::Show(key) => {
                self.terminal
                    .update(cx, |t, cx| t.show_job(key, window, cx));
            }
            RunEvent::Browse(_) => {}
        }
        cx.notify();
    }

    /// Starts the terminals of a Run panel row (a running one is only
    /// shown), showing the dock if hidden; the last one shows as `origin`
    /// allows. Returns why the first that couldn't start didn't, once the
    /// others have started.
    pub(super) fn start_scripts(
        &mut self,
        launches: &[Launch],
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let last = launches.len().saturating_sub(1);
        let started: Vec<_> = self.terminal.update(cx, |t, cx| {
            launches
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let show = (i == last).then_some(origin);
                    t.start_job(l.tag.clone(), l.dir.clone(), &l.command, show, window, cx)
                })
                .collect()
        });
        cx.notify();
        started.into_iter().try_for_each(|s| s.map(drop))
    }

    /// Types `commands` into this window's terminal, showing the dock if
    /// hidden; the terminal shows as `origin` allows. Returns the tab and
    /// pane it typed into, or why no shell could start.
    pub fn run_in_terminal(
        &mut self,
        commands: &str,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        self.leave_welcome(TERMINAL_ONLY, cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let ran = self
            .terminal
            .update(cx, |t, cx| t.run(commands, origin, window, cx));
        cx.notify();
        ran
    }

    /// A new shell in the active project's folder (else the home folder).
    pub(super) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.root(cx) {
            Some(root) => self.open_terminal_in(root, window, cx),
            None => {
                self.leave_welcome(TERMINAL_ONLY, cx);
                if !self.layout.terminal {
                    self.layout.toggle(Area::Terminal);
                }
                self.terminal
                    .update(cx, |t, cx| t.open_shell(Some(window), cx));
                cx.notify();
            }
        }
    }

    /// Shows a session to the server with `key` in the terminal dock: the
    /// open one, unless `new` or there is none. It shows as `origin` allows.
    /// Returns the session's tab and pane, or why there is none: when the
    /// server's settings are incomplete, the user gets them in a tab.
    pub(super) fn connect_ssh(
        &mut self,
        key: String,
        new: bool,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        self.leave_welcome(Layout::default(), cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        if !new
            && let Some(found) = self
                .terminal
                .update(cx, |t, cx| t.focus_ssh(&key, origin, window, cx))
        {
            cx.notify();
            return Ok(found);
        }
        let opened = match self.ssh_store.update(cx, |s, cx| s.remote(&key, cx)) {
            Ok(remote) => self
                .terminal
                .update(cx, |t, cx| t.open_ssh(remote, origin, window, cx)),
            Err(problem) => {
                if origin == Origin::User
                    && let Some(id) = key.strip_prefix("ssh\u{0}")
                {
                    self.open_ssh_host(id.to_string(), window, cx);
                }
                Err(format!(
                    "{problem} Fix the server's settings (update_ssh_host), then connect again."
                ))
            }
        };
        cx.notify();
        opened
    }
}
