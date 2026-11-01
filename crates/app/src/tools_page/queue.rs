//! The Dev Tools queue: installs, upgrades and uninstalls, prerequisites first, run one at a
//! time in the login shell with their output kept per tool; Retry, Cancel and Stop; and quitting,
//! which stops the run and waits until it was told to end.

use futures::StreamExt as _;
use futures::channel::oneshot;
use futures::future::abortable;
use gpui_kit::*;
use sawit_core::dev_tools::{
    Tool, dependents, find, in_terminal, needed_by_warning, run, uninstall,
};

use super::{Job, LOG_LINES, State, ToolsEvent, ToolsPage};
use crate::rt;

impl ToolsPage {
    pub(super) fn upgrade(&mut self, ids: &[&'static str], cx: &mut Context<Self>) {
        self.upgrades.extend(ids);
        self.enqueue(ids, cx);
    }

    /// Queues `wanted` (and what they need) and starts the queue.
    pub(super) fn enqueue(&mut self, wanted: &[&'static str], cx: &mut Context<Self>) {
        let tools = self.plan_for(wanted);
        if self.running.is_none() && self.queue.is_empty() {
            self.batch = (0, 0);
        }
        for tool in tools {
            self.selected.remove(tool.id);
            if tool.interactive {
                self.states.insert(tool.id, State::InTerminal);
                cx.emit(ToolsEvent::RunInTerminal(self.command(tool)));
                self.upgrades.remove(tool.id);
                continue;
            }
            let for_ = (!wanted.contains(&tool.id))
                .then(|| {
                    wanted
                        .iter()
                        .filter_map(|w| find(w))
                        .find(|w| self.plan_for(&[w.id]).iter().any(|t| t.id == tool.id))
                        .map(|w| w.name)
                })
                .flatten();
            self.states.insert(tool.id, State::Queued { for_ });
            self.queue.push_back((tool, Job::Install));
            self.batch.1 += 1;
        }
        self.pump(cx);
        cx.notify();
    }

    /// Asks, then queues `tool`'s uninstaller (or hands it to the terminal
    /// when it asks for a password).
    pub(super) fn confirm_uninstall(
        &mut self,
        tool: &'static Tool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.uninstall_command(tool) else {
            return;
        };
        self.ask_uninstall(tool, command, window, cx);
    }

    /// Asks, then queues `command`, which removes `tool` (or hands it to the
    /// terminal when it asks for a password).
    pub(super) fn ask_uninstall(
        &mut self,
        tool: &'static Tool,
        command: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A copy installed outside what it needs (PureMac from its disk image) doesn't need it.
        let needed_by: Vec<&Tool> = dependents(tool.id)
            .into_iter()
            .filter(|t| self.installed(t.id) && self.outside(t.id).is_none())
            .collect();
        let mut detail = format!("Runs: {command}");
        if !needed_by.is_empty() {
            // Per kind: a cask's app keeps working, without Homebrew's updates.
            detail = format!("{}.\n\n{detail}", needed_by_warning(&needed_by));
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Uninstall {}?", tool.name),
            Some(&detail),
            &["Uninstall", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    if in_terminal(tool, &command) {
                        this.states.insert(tool.id, State::InTerminal);
                        cx.emit(ToolsEvent::RunInTerminal(command));
                        cx.notify();
                        return;
                    }
                    if this.running.is_none() && this.queue.is_empty() {
                        this.batch = (0, 0);
                    }
                    this.selected.remove(tool.id);
                    this.states.insert(tool.id, State::Queued { for_: None });
                    this.queue.push_back((tool, Job::Uninstall(command)));
                    this.batch.1 += 1;
                    this.pump(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Retry: runs the job that failed again. An install or upgrade (or one
    /// skipped for a missing prerequisite) is queued again; an uninstall asks
    /// again, as the trash button does, then runs the same command. Never the
    /// installer after an uninstall: that would put back what the user removes.
    pub(super) fn retry(
        &mut self,
        tool: &'static Tool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.states.get(tool.id) {
            Some(State::Failed(_, Some(Job::Uninstall(command)))) => {
                let command = command.clone();
                self.ask_uninstall(tool, command, window, cx);
            }
            Some(State::Failed(_, Some(Job::Install)) | State::Skipped(_)) => {
                self.enqueue(&[tool.id], cx)
            }
            _ => {}
        }
    }

    /// Starts the next queued installer unless one is running.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let (tool, job) = loop {
            let Some((tool, job)) = self.queue.pop_front() else {
                // The queue is done: let detection confirm the results.
                if self.batch.1 > 0 {
                    self.reload(cx);
                }
                return;
            };
            if matches!(job, Job::Uninstall(_)) {
                break (tool, job);
            }
            match tool.needs.iter().find(|n| !self.installed(n)) {
                Some(need) => {
                    let name = find(need).map_or(*need, |t| t.name);
                    self.states.insert(tool.id, State::Skipped(name));
                    self.batch.0 += 1;
                }
                None => break (tool, job),
            }
        };
        let id = tool.id;
        self.running = Some((id, job.clone()));
        let (state, command) = match &job {
            Job::Install => (State::Installing, self.command(tool)),
            Job::Uninstall(command) => (State::Uninstalling, command.clone()),
        };
        self.states.insert(id, state);
        self.logs.insert(id, vec![format!("$ {command}")]);
        let finished = job.clone();
        let (tx, mut lines) = futures::channel::mpsc::unbounded::<String>();
        let (run, stop) = abortable(async move {
            let on_line = move |line| {
                let _ = tx.unbounded_send(line);
            };
            match job {
                Job::Install => run(tool, &command, on_line).await,
                Job::Uninstall(command) => uninstall(tool, &command, on_line).await,
            }
            .map_err(|e| e.to_string())
        });
        self.stop = Some(stop);
        let (ended, ended_rx) = oneshot::channel::<()>();
        self.ended = Some(ended_rx);
        // `None` when it was stopped.
        let done = rt::spawn(cx, async move {
            let result = run.await.ok();
            // Only now: the run, and the process group it ends when dropped, are gone.
            drop(ended);
            result
        });
        cx.spawn(async move |this, cx| {
            // Ends when the installer exits and drops the sender.
            while let Some(line) = lines.next().await {
                let ok = this.update(cx, |this, cx| {
                    let log = this.logs.entry(id).or_default();
                    log.push(line);
                    if log.len() > LOG_LINES {
                        log.drain(..log.len() - LOG_LINES);
                    }
                    cx.notify();
                });
                if ok.is_err() {
                    return;
                }
            }
            let result = done
                .await
                .unwrap_or_else(|_| Some(Err("The installer stopped.".into())));
            let _ = this.update(cx, |this, cx| this.finish(id, finished, result, cx));
        })
        .detach();
    }

    pub(super) fn finish(
        &mut self,
        id: &'static str,
        job: Job,
        result: Option<Result<(), String>>,
        cx: &mut Context<Self>,
    ) {
        self.running = None;
        self.stop = None;
        self.ended = None;
        self.batch.0 += 1;
        match result {
            None => {
                self.states.insert(id, State::Stopped);
            }
            Some(Ok(())) => {
                self.upgrades.remove(id);
                let done = match job {
                    Job::Install => State::Done,
                    Job::Uninstall(_) => State::Removed,
                };
                self.states.insert(id, done);
                self.expanded.remove(id);
            }
            Some(Err(e)) => {
                self.states.insert(id, State::Failed(e, Some(job)));
                self.expanded.insert(id);
            }
        }
        self.pump(cx);
        cx.notify();
    }

    /// Stops the running installer or uninstaller and everything it
    /// started; the queue moves on.
    pub(super) fn stop_running(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
        cx.notify();
    }

    /// Quitting: stops the running job and what's queued, as closing the page
    /// does, and has the app wait (as long as it waits for quit handlers) until
    /// the job's process group was told to end. The page's drop, which also
    /// stops it, comes as the app lets go of its windows, and the app could
    /// exit before the stop got through: the installer, in a session of its
    /// own, then ran on with nobody to show its end.
    pub(super) fn stop_for_quit(&mut self) -> impl Future<Output = ()> + use<> {
        self.queue.clear();
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
        let ended = self.ended.take();
        async move {
            if let Some(ended) = ended {
                let _ = ended.await;
            }
        }
    }

    /// Drops everything still waiting; the running installer finishes.
    pub(super) fn stop_queue(&mut self, cx: &mut Context<Self>) {
        for (tool, _) in self.queue.drain(..) {
            self.states.remove(tool.id);
            self.upgrades.remove(tool.id);
        }
        self.batch.1 = self.batch.0 + usize::from(self.running.is_some());
        cx.notify();
    }

    pub(super) fn dequeue(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.queue.iter().position(|(t, _)| t.id == id) {
            self.queue.remove(ix);
            self.states.remove(id);
            self.upgrades.remove(id);
            self.batch.1 = self.batch.1.saturating_sub(1);
        }
        cx.notify();
    }
}
