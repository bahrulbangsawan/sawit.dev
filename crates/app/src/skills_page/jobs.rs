//! The Skills page's scripts: queued, run one at a time in the login shell with their output
//! kept per pack, stopped or taken out of the queue, and stopped by quitting; and the installs,
//! updates and removals that queue them.

use futures::StreamExt as _;
use futures::channel::oneshot;
use futures::future::abortable;
use gpui_kit::*;
use sawit_core::package_manager::PackageManager;
use sawit_core::skills::{self, Action, Pack};

use super::{Job, LOG_LINES, Pick, SkillsPage, State, plural};
use crate::rt;

impl SkillsPage {
    // Jobs

    /// Queues `lines` for `pack` and starts the queue.
    fn enqueue(
        &mut self,
        pack: &'static Pack,
        action: Action,
        lines: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.busy(pack.id) {
            return;
        }
        self.states.insert(pack.id, State::Queued(action));
        self.queue.push_back(Job {
            pack,
            action,
            lines,
        });
        self.pump(cx);
        cx.notify();
    }

    /// Starts the next queued script unless one is running.
    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let Some(job) = self.queue.pop_front() else {
            return;
        };
        let id = job.pack.id;
        self.states.insert(id, State::Running(job.action));
        let mut log = vec![format!("$ {}", job.lines.join("\n$ "))];
        log.truncate(LOG_LINES);
        self.logs.insert(id, log);
        self.running = Some(job.clone());
        let (tx, mut lines) = futures::channel::mpsc::unbounded::<String>();
        let (run, stop) = abortable({
            let (job, env) = (job.clone(), self.env.clone());
            async move {
                skills::run(job.pack, job.action, &job.lines, &env, move |line| {
                    let _ = tx.unbounded_send(line);
                })
                .await
                .map_err(|e| (e.to_string(), e.started))
            }
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
            // Ends when the script exits and drops the sender.
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
                .unwrap_or_else(|_| Some(Err(("The script stopped.".into(), true))));
            let _ = this.update(cx, |this, cx| this.finish(job, result, cx));
        })
        .detach();
    }

    /// What ran (`None` when it was stopped); a failure says whether the shell had started on the
    /// lines.
    fn finish(
        &mut self,
        job: Job,
        result: Option<Result<(), (String, bool)>>,
        cx: &mut Context<Self>,
    ) {
        self.running = None;
        self.stop = None;
        self.ended = None;
        let id = job.pack.id;
        match result {
            None => {
                self.states.insert(id, State::Stopped);
            }
            Some(Ok(())) => {
                self.states.insert(id, State::Done(job.action));
                self.expanded.remove(id);
            }
            Some(Err((e, started))) => {
                // It failed before the shell started (the pack's lock, writing the skill out):
                // the lines the log shows as run never ran.
                if !started {
                    let lines = format!("Not run:\n{}", job.lines.join("\n"));
                    self.logs.insert(id, vec![lines]);
                }
                self.states.insert(id, State::Failed(job.action, e));
                self.expanded.insert(id);
            }
        }
        // What it changed, read again; then the next one.
        self.reload(cx);
        self.pump(cx);
        cx.notify();
    }

    /// Stops the running script and everything it started; the queue moves on.
    pub(super) fn stop_running(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.stop.take() {
            stop.abort();
        }
        cx.notify();
    }

    pub(super) fn dequeue(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.queue.iter().position(|j| j.pack.id == id) {
            self.queue.remove(ix);
            self.states.remove(id);
        }
        cx.notify();
    }

    /// Quitting: stops the running script and what's queued, and has the app
    /// wait (as long as it waits for quit handlers) until its process group
    /// was told to end, as the Dev Tools page does.
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

    /// Install or Update from the sheet: queues the script for the ticked agents.
    pub(super) fn run_sheet(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(pack) = self.sheet.as_ref().map(|s| s.pack) else {
            return;
        };
        let pick = if action == Action::Update {
            Pick::Update
        } else {
            Pick::Install
        };
        match self.sheet_script(action, &self.picked(pick)) {
            Ok(lines) => {
                self.enqueue(pack, action, lines, cx);
                self.close_sheet(cx);
            }
            Err(e) => {
                self.states.insert(pack.id, State::Failed(action, e));
                cx.notify();
            }
        }
    }

    /// Asks, then removes `pack` from `targets`, naming them and what runs.
    pub(super) fn confirm_remove(
        &mut self,
        pack: &'static Pack,
        targets: Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let places: Vec<skills::Place> = targets
            .iter()
            .map(|&t| (&self.report.targets[t], &self.report.installed[t]))
            .collect();
        let lines = match skills::script(
            pack,
            pack.methods[0],
            Action::Remove,
            &places,
            PackageManager::Npm,
            &self.env,
        ) {
            Ok(lines) => lines,
            // Nothing of its own there (another pack's plugin brings its skills): say why.
            Err(why) => {
                let answer = window.prompt(
                    PromptLevel::Info,
                    &format!("Nothing to remove for {}", pack.name),
                    Some(&why),
                    &["OK"],
                    cx,
                );
                cx.spawn(async move |_, _| {
                    let _ = answer.await;
                })
                .detach();
                return;
            }
        };
        let names: Vec<&str> = targets
            .iter()
            .map(|&t| self.report.targets[t].label.as_str())
            .collect();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Remove {} from {}?",
                pack.name,
                plural(targets.len(), "agent", "agents")
            ),
            Some(&format!(
                "From: {}.\n\nRuns:\n{}",
                names.join(", "),
                lines.join("\n")
            )),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    this.sheet = None;
                    this.refocus = true;
                    this.enqueue(pack, Action::Remove, lines, cx);
                });
            }
        })
        .detach();
    }
}
