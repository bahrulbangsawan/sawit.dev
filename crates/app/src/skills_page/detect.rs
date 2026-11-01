//! What detection finds off the UI thread (the agents' skills folders and what each has, the
//! package managers, `claude` and `git`), and what it says of each pack.

use gpui_kit::*;
use sawit_core::dev_tools::find_program;
use sawit_core::package_manager::{self, Available, Use};
use sawit_core::skills::{self, Method, Pack, Report, Status};

use super::{SkillsPage, State};
use crate::rt;

/// What detection finds, off the UI thread.
struct Detected {
    report: Report,
    claude: bool,
    git: bool,
}

impl SkillsPage {
    /// Detects the agents, what each has, and the package managers, `claude`
    /// and `git` (off the UI thread).
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let rx = rt::spawn_blocking(cx, || Detected {
            report: skills::report(),
            claude: find_program("claude").is_some(),
            git: find_program("git").is_some() || std::path::Path::new("/usr/bin/git").exists(),
        });
        let pms = rt::spawn(cx, package_manager::detect());
        cx.spawn(async move |this, cx| {
            let (Ok(detected), Ok(pms)) = (rx.await, pms.await) else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.detected(detected, pms, cx));
        })
        .detach();
        cx.notify();
    }

    fn detected(&mut self, detected: Detected, pms: Vec<Available>, cx: &mut Context<Self>) {
        // A sheet's rows are places in the old list, which may now hold others.
        if self.sheet.is_some() && detected.report.targets != self.report.targets {
            self.sheet = None;
            self.refocus = true;
        }
        self.report = detected.report;
        self.claude = detected.claude;
        self.git = detected.git;
        self.pm = package_manager::choose(
            &pms,
            self.pm,
            crate::package_managers::remembered(),
            Use::Run,
        );
        self.pms = pms;
        self.loading = false;
        // Detection now agrees with what ran.
        self.states
            .retain(|_, s| !matches!(s, State::Done(_) | State::Stopped));
        cx.notify();
    }

    pub(super) fn status(&self, pack: &Pack, t: usize) -> Status {
        self.report.status(pack, t)
    }

    /// The agents that have the pack, or part of it.
    pub(super) fn having(&self, pack: &Pack) -> Vec<usize> {
        (0..self.report.targets.len())
            .filter(|&t| self.status(pack, t).any())
            .collect()
    }

    pub(super) fn busy(&self, id: &str) -> bool {
        matches!(
            self.states.get(id),
            Some(State::Queued(_) | State::Running(_))
        )
    }

    /// Why `method` can't run here, when it can't: a program it needs is
    /// missing.
    pub(super) fn blocked(&self, method: Method) -> Option<String> {
        match method {
            Method::SkillsCli { .. } if self.pm.is_none() => Some(
                "The skills CLI runs with npx, pnpm dlx, bunx or yarn dlx, and none is installed: install Node.js, Bun or pnpm from the Dev Tools page."
                    .into(),
            ),
            Method::Plugin { .. } if !self.claude => Some(
                "Plugins install through Claude Code's own CLI, `claude`, which isn't installed: install Claude Code from the Dev Tools page."
                    .into(),
            ),
            Method::Copy { .. } if !self.git => {
                Some("The copy clones with git, which isn't installed: run xcode-select --install.".into())
            }
            _ => None,
        }
    }
}
