//! Creating the project: the create checked and handed to the workspace to run in a terminal,
//! watched until its job is over, then opened (or, from Create Project Only, only made), or said
//! to have failed.

use std::time::Instant;

use gpui_kit::*;
use sawit_core::{new_project, stack_builder};

use super::{CreateJob, Creating, NEVER_RAN, NewProjectEvent, NewProjectPage, POLL, SETTLE};
use crate::terminal::{JobTag, TerminalPanel};

impl NewProjectPage {
    /// Whether a create runs.
    pub(super) fn busy(&self) -> bool {
        matches!(self.creating, Creating::Running { .. })
    }

    /// Checks the create can start and marks it running, to open the project once made when
    /// `open` (Create Project; Create Project Only leaves it made): the job for the workspace to
    /// run, or why it can't start (also shown on the page).
    pub fn begin_create(
        &mut self,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Result<CreateJob, String> {
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
            idle: None,
            open,
        };
        self.keep(cx);
        cx.notify();
        Ok(job)
    }

    /// Create Project: asks the workspace to run the create, then opens the project.
    pub(super) fn create(&mut self, cx: &mut Context<Self>) {
        self.create_and(true, cx);
    }

    /// Create Project Only: the same create, leaving the project made but not opened.
    pub(super) fn create_only(&mut self, cx: &mut Context<Self>) {
        self.create_and(false, cx);
    }

    /// Asks the workspace to run the create; the project opens once made when `open`.
    pub(super) fn create_and(&mut self, open: bool, cx: &mut Context<Self>) {
        if let Ok(job) = self.begin_create(open, cx) {
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
            dir,
            key,
            since,
            seen,
            idle,
            ..
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
        let made = new_project::created(dir);
        if !job_over(running, made, seen, idle, *since, Instant::now()) {
            return false;
        }
        self.finish(cx);
        true
    }

    /// The create's job ended: opens the project when it was made (and asked to), else says it
    /// failed.
    fn finish(&mut self, cx: &mut Context<Self>) {
        let Creating::Running { dir, open, .. } = &self.creating else {
            return;
        };
        let (dir, open) = (dir.clone(), *open);
        if new_project::created(&dir) {
            self.creating = Creating::Done {
                dir: dir.clone(),
                open,
            };
            if open {
                cx.emit(NewProjectEvent::Open(dir));
            }
        } else {
            self.creating = Creating::Failed { dir, open };
        }
        cx.notify();
    }

    /// Open Project, after Create Project Only: opens the project made.
    pub(super) fn open_made(&mut self, cx: &mut Context<Self>) {
        let Creating::Done { dir, open: false } = &self.creating else {
            return;
        };
        let dir = dir.clone();
        self.creating = Creating::Done {
            dir: dir.clone(),
            open: true,
        };
        cx.emit(NewProjectEvent::Open(dir));
        cx.notify();
    }
}

/// Whether the create's job is over: `running` is its terminal's state (gone: `None`; a command
/// running: `Some(true)`; the shell idle: `Some(false)`), `made` whether the project is there.
/// Seen running once, it is over when the shell is idle again with the project made, or idle
/// for [`SETTLE`] without it: what ran may have been the login shell's own startup, the create
/// still to come, and a command running meanwhile starts the wait over. Never seen running (it
/// failed at once, between two looks), once [`NEVER_RAN`] has passed since `since`.
pub(super) fn job_over(
    running: Option<bool>,
    made: bool,
    seen: &mut bool,
    idle: &mut Option<Instant>,
    since: Instant,
    now: Instant,
) -> bool {
    match running {
        None => true,
        Some(true) => {
            *seen = true;
            *idle = None;
            false
        }
        Some(false) if !*seen => now.duration_since(since) >= NEVER_RAN,
        Some(false) => made || now.duration_since(*idle.get_or_insert(now)) >= SETTLE,
    }
}
