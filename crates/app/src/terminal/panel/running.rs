//! What runs in the terminals: the coding agents in their panes (and sending them the browser's
//! notes), and what quitting or closing the window would end.

use gpui_kit::{App, Context, Entity, Window};
use sawit_core::terminal::{AgentKind, PaneId};

use super::{Origin, RunningAgent, TerminalPanel};
use crate::terminal::tab::TerminalTab;
use crate::terminal::view::TerminalView;

impl TerminalPanel {
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

    /// The agents running on this Mac that the browser's notes may be sent to: those of
    /// [`Self::running_agents`] outside SSH sessions. A session's agent is known only by the
    /// title it set, which can outlast it, and its notes' paths are this Mac's.
    pub fn local_agents(&self, cx: &App) -> Vec<RunningAgent> {
        self.running_agents(cx)
            .into_iter()
            .filter(|a| self.agent_view(a.pane, a.kind, cx).is_some())
            .collect()
    }

    /// Sends `text` to the agent `kind` in `pane` as one message, as if the user pasted it into
    /// the agent's chat and pressed Return: a bracketed paste, so its lines arrive as one message
    /// rather than one per line, then Return a moment later ([`ENTER_AFTER`]), so the agent has
    /// taken the whole paste before it submits it. Only while that agent is still the pane's
    /// foreground program and has bracketed paste on: an agent that has exited left a shell
    /// there, which would run the notes as commands. The pane then shows as an agent's command's
    /// does ([`Origin::Agent`]): in front, without the keyboard, which stays with the user.
    /// Returns the agent as it was sent to, or what to tell the user.
    pub fn send_to_agent(
        &mut self,
        pane: PaneId,
        kind: AgentKind,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<RunningAgent, String> {
        let gone = || {
            format!(
                "{} is no longer running there, so nothing was sent.",
                kind.name()
            )
        };
        let view = self.agent_view(pane, kind, cx).ok_or_else(gone)?;
        let agent = self
            .running_agents(cx)
            .into_iter()
            .find(|a| a.pane == pane && a.kind == kind)
            .ok_or_else(gone)?;
        let ix = self
            .tabs
            .iter()
            .position(|t| t.downgrade() == agent.tab)
            .ok_or_else(gone)?;
        if !view.read(cx).accepts_paste() {
            return Err(format!(
                "{} isn't ready for a paste yet, so nothing was sent.",
                kind.name()
            ));
        }
        view.update(cx, |v, _| v.paste(text));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ENTER_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                // The agent may have ended meanwhile: Return would then reach its shell.
                if let Some(view) = this.agent_view(pane, kind, cx) {
                    view.read(cx).write(b"\r".to_vec());
                }
            });
        })
        .detach();
        self.reveal(ix, pane, Origin::Agent, window, cx);
        Ok(agent)
    }

    /// The terminal of `pane` while the agent `kind` runs there, on this Mac. The agent is as
    /// the pane last looked ([`TerminalView::agent`]), and something must still run in front of
    /// its shell, read now: an agent that has just exited, before the pane looks again, has left
    /// only its shell.
    fn agent_view(&self, pane: PaneId, kind: AgentKind, cx: &App) -> Option<Entity<TerminalView>> {
        self.all_panes(cx)
            .find(|(_, id, view)| {
                let v = view.read(cx);
                *id == pane
                    && !v.exited
                    && v.remote.is_none()
                    && v.agent() == Some(kind)
                    && v.foreground_job().is_some()
            })
            .map(|(_, _, view)| view.clone())
    }
}

/// How long after pasting a message into an agent Return follows. Agents read a paste in
/// pieces; a Return in the same read as the paste's end could be taken as part of it.
pub(super) const ENTER_AFTER: std::time::Duration = std::time::Duration::from_millis(150);

/// What runs in `pane` of `tab`, whose terminal is `view`, that closing it would end, by name: a
/// command in the foreground (by the pane's name, the name of a tab it is alone in, its script's,
/// its coding agent's, else the program's) or a live SSH session. `None` for a shell at its prompt, a script's terminal
/// between runs or an ended session.
pub(super) fn running_in(tab: &TerminalTab, pane: PaneId, view: &TerminalView) -> Option<String> {
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
