//! Which coding agent runs in the terminal's foreground, looked at soon after output.

use gpui_kit::*;
use sawit_core::terminal::{AgentKind, detect, process_args, process_name};

use super::{AGENT_CHECK, TerminalEvent, TerminalView};

impl TerminalView {
    /// The coding agent running here (Claude Code, Codex, …), as last looked at: soon after
    /// any output or new title, never on a timer of its own, so an idle pane costs nothing.
    pub fn agent(&self) -> Option<AgentKind> {
        self.agent
    }

    /// Looks for the agent [`AGENT_CHECK`] from now, unless a look is already due: output
    /// that keeps coming gets one look per interval.
    pub(super) fn check_agent_soon(&mut self, cx: &mut Context<Self>) {
        if self.agent_check.is_some() {
            return;
        }
        self.agent_check = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AGENT_CHECK).await;
            let _ = this.update(cx, |this, cx| {
                this.agent_check = None;
                this.look_for_agent(cx);
            });
        }));
    }

    /// Reads which agent runs now, and when that changed, redraws and tells the tab (whose
    /// strip shows it).
    fn look_for_agent(&mut self, cx: &mut Context<Self>) {
        let agent = self.read_agent();
        if agent != self.agent {
            self.agent = agent;
            cx.emit(TerminalEvent::AgentChanged);
            cx.notify();
        }
    }

    /// The agent in the foreground. A local job is known by its process: the process group's
    /// leader (the command the shell runs), by its name, which the OS gives cheaply, and only
    /// when that changed, by its executable and arguments ([`detect`]). A title stands in only
    /// when the job set it, so one left by an agent that has exited doesn't count. An SSH
    /// session's remote programs can't be seen from here: only a title the session set says
    /// what runs there.
    fn read_agent(&mut self) -> Option<AgentKind> {
        #[cfg(test)]
        if let Some(fake) = self.fake_agent {
            return fake;
        }
        if self.exited {
            self.agent_job = None;
            return None;
        }
        if self.remote.is_some() {
            return self
                .title_job
                .and_then(|_| detect(None, Some(self.title.as_ref())));
        }
        let Some(job) = self.session.foreground_job() else {
            self.agent_job = None;
            return None;
        };
        let key = (job, process_name(job));
        if self.agent_job.as_ref().is_none_or(|(seen, _)| *seen != key) {
            self.agent_job = Some((key, process_args(job)));
        }
        let process = self.agent_job.as_ref().and_then(|(_, p)| p.as_ref());
        let title = (self.title_job == Some(Some(job))).then_some(self.title.as_ref());
        detect(process, title)
    }

    /// Makes the pane look as if `agent` ran in it, whatever really does (for tests).
    #[cfg(test)]
    pub(in crate::terminal) fn fake_agent(
        &mut self,
        agent: Option<AgentKind>,
        cx: &mut Context<Self>,
    ) {
        self.fake_agent = Some(agent);
        self.look_for_agent(cx);
    }
}
