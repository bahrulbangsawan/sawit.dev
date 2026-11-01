//! Creating the project: the create checked and handed to the workspace to run in a terminal,
//! watched until its job is over, then opened, or said to have failed.

use std::time::{Duration, Instant};

use gpui_kit::*;
use sawit_core::{new_project, stack_builder};

use super::{CreateJob, Creating, NEVER_RAN, NewProjectEvent, NewProjectPage, POLL};
use crate::terminal::{JobTag, TerminalPanel};

impl NewProjectPage {
    /// Whether a create runs.
    pub(super) fn busy(&self) -> bool {
        matches!(self.creating, Creating::Running { .. })
    }

    /// Checks the create can start and marks it running: the job for the workspace to run,
    /// or why it can't start (also shown on the page).
    pub fn begin_create(&mut self, cx: &mut Context<Self>) -> Result<CreateJob, String> {
        if self.busy() {
            return Err("A project is being created already.".into());
        }
        let dir = match new_project::target(&self.location, &self.stack) {
            Ok(dir) => dir,
            Err(why) => {
                self.problem = Some(why.clone());
                cx.notify();
                return Err(why);
            }
        };
        let key = format!("new-project:{}", dir.display());
        let name = stack_builder::format_project_name(&self.stack.project_name);
        let job = CreateJob {
            tag: JobTag {
                key: key.clone(),
                name: format!("New Project: {name}"),
            },
            location: self.location.clone(),
            dir: dir.clone(),
            command: stack_builder::run_command(&self.stack),
        };
        self.problem = None;
        self.creating = Creating::Running {
            dir,
            key,
            since: Instant::now(),
            seen: false,
        };
        self.keep(cx);
        cx.notify();
        Ok(job)
    }

    /// Create Project: asks the workspace to run the create.
    pub(super) fn create(&mut self, cx: &mut Context<Self>) {
        if let Ok(job) = self.begin_create(cx) {
            cx.emit(NewProjectEvent::Create(job));
        }
    }

    /// The create runs in `terminal`: watches its job until it ends.
    pub fn watch(&mut self, terminal: WeakEntity<TerminalPanel>, cx: &mut Context<Self>) {
        self.terminal = Some(terminal);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(over) = this.update(cx, |this, cx| this.poll(cx)) else {
                    return;
                };
                if over {
                    return;
                }
            }
        })
        .detach();
    }

    /// The terminal couldn't start the create: says why.
    pub fn not_started(&mut self, why: String, cx: &mut Context<Self>) {
        self.creating = Creating::Idle;
        self.problem = Some(why);
        cx.notify();
    }

    /// One look at the create's job; true once it is over and the page has said how it went.
    pub(super) fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let Creating::Running {
            key, since, seen, ..
        } = &mut self.creating
        else {
            return true;
        };
        let running = self
            .terminal
            .as_ref()
            .and_then(|t| t.upgrade())
            .and_then(|t| t.read(cx).job_status(key, cx))
            .map(|s| s.running.is_some());
        if !job_over(running, seen, since.elapsed()) {
            return false;
        }
        self.finish(cx);
        true
    }

    /// The create's job ended: opens the project when it was made, else says it failed.
    fn finish(&mut self, cx: &mut Context<Self>) {
        let Creating::Running { dir, .. } = &self.creating else {
            return;
        };
        let dir = dir.clone();
        if new_project::created(&dir) {
            self.creating = Creating::Done(dir.clone());
            cx.emit(NewProjectEvent::Open(dir));
        } else {
            self.creating = Creating::Failed { dir };
        }
        cx.notify();
    }
}

/// Whether the create's job is over: `running` is its terminal's state (gone: `None`; its
/// command running: `Some(true)`; the shell idle: `Some(false)`). Seen running once, it is over
/// when the shell is idle again; never seen running (it failed at once, between two looks), once
/// [`NEVER_RAN`] has passed.
pub(super) fn job_over(running: Option<bool>, seen: &mut bool, waited: Duration) -> bool {
    match running {
        None => true,
        Some(true) => {
            *seen = true;
            false
        }
        Some(false) => *seen || waited >= NEVER_RAN,
    }
}
