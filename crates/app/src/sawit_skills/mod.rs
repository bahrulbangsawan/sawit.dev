//! Sawit Skills in the app: where they stand on this Mac (the footer's button and the Skills
//! page's card read it), and the runs that install, update, add them to a project or remove
//! them, one at a time, each ending in a dialog that lists what changed, with paths.

mod button;
mod card;

use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::sawit_skills::{self, Level, Outcome, Scope};

use crate::rt;

pub use button::footer_button;
pub use card::card;

actions!(
    sawit,
    [
        /// Adds the Sawit block to the window's project: its workspace knows which one.
        AddSawitSkillsToProject
    ]
);

/// What a run does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// Every agent on this Mac: install, or update what is there.
    Install,
    /// The block in one project's instruction file.
    Project,
    Remove,
}

impl Job {
    /// "Installing Skills…", for a busy button.
    pub fn doing(self) -> &'static str {
        match self {
            Job::Install => "Installing Skills…",
            Job::Project => "Adding to the Project…",
            Job::Remove => "Removing Skills…",
        }
    }
}

/// Sawit Skills on this Mac, as last detected, and the run going on.
pub struct SawitSkills {
    /// `None` until the first detection ends.
    pub level: Option<Level>,
    /// What the button's tooltip adds: how many agents and files are current.
    pub detail: String,
    pub running: Option<Job>,
    /// Counts detections, so an older one ending late doesn't overwrite a newer one's.
    generation: u64,
}

struct GlobalSawitSkills(Entity<SawitSkills>);

impl Global for GlobalSawitSkills {}

pub fn init(cx: &mut App) {
    let status = cx.new(|_| SawitSkills {
        level: None,
        detail: String::new(),
        running: None,
        generation: 0,
    });
    cx.set_global(GlobalSawitSkills(status));
    refresh(cx);
}

/// The state every window's footer and the Skills page observe.
pub fn status(cx: &App) -> Entity<SawitSkills> {
    cx.global::<GlobalSawitSkills>().0.clone()
}

/// [`status`], when [`init`] has run (not in a test that makes a page on its own).
pub fn try_status(cx: &App) -> Option<Entity<SawitSkills>> {
    cx.try_global::<GlobalSawitSkills>().map(|g| g.0.clone())
}

/// The `sawit-mcp` beside the app, as the MCP page installs it.
fn server() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("sawit-mcp")))
        .unwrap_or_else(|| PathBuf::from("sawit-mcp"))
}

/// Detects again, off the main thread: after a run, after the Skills page installs or removes a
/// pack, and when the app starts.
pub fn refresh(cx: &mut App) {
    let Some(status) = try_status(cx) else {
        return;
    };
    let generation = status.update(cx, |s, _| {
        s.generation += 1;
        s.generation
    });
    let server = server();
    let detected = rt::spawn_blocking(cx, move || sawit_skills::state(&server));
    cx.spawn(async move |cx| {
        let Ok(state) = detected.await else { return };
        status.update(cx, |s, cx| {
            if s.generation == generation {
                s.level = Some(state.level);
                s.detail = state.detail();
                cx.notify();
            }
        });
    })
    .detach();
}

/// Runs `job` (for `project` when it is [`Job::Project`]) unless a run is going on, then shows
/// what changed in `window`.
pub fn run(job: Job, project: Option<PathBuf>, window: AnyWindowHandle, cx: &mut App) {
    let status = status(cx);
    if status.read(cx).running.is_some() {
        return;
    }
    let scope = match (job, project) {
        (Job::Project, Some(dir)) => Scope::Project(dir),
        (Job::Project, None) => {
            let _ = window.update(cx, |_, window, cx| {
                let answer = window.prompt(
                    PromptLevel::Info,
                    "No project is open",
                    Some("Open a project's folder in this window, then add Sawit Skills to it."),
                    &["OK"],
                    cx,
                );
                cx.spawn(async move |_| {
                    let _ = answer.await;
                })
                .detach();
            });
            return;
        }
        _ => Scope::Global,
    };
    status.update(cx, |s, cx| {
        s.running = Some(job);
        cx.notify();
    });
    let (paths, server) = (AppPaths::default_macos(), server());
    let removing = job == Job::Remove;
    let ran = rt::spawn(cx, async move {
        if removing {
            sawit_skills::remove(&scope, &paths).await
        } else {
            sawit_skills::install(&scope, &paths, &server).await
        }
    });
    cx.spawn(async move |cx| {
        let result = ran
            .await
            .unwrap_or_else(|_| Err("The run stopped before it finished.".into()));
        cx.update(|cx| {
            status.update(cx, |s, cx| {
                s.running = None;
                cx.notify();
            });
            refresh(cx);
            show(job, result, window, cx);
        });
    })
    .detach();
}

/// The dialog after a run: what changed, part by part, with every path.
fn show(job: Job, result: Result<Outcome, String>, window: AnyWindowHandle, cx: &mut App) {
    let removing = job == Job::Remove;
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let (level, title, detail) = match result {
        Ok(outcome) => {
            let mut lines = outcome.lines(&home);
            if !removing && outcome.changes() > 0 {
                lines.push(String::new());
                lines.push("Start a new session of each agent to pick it up.".into());
            }
            let level = match outcome.failures() {
                0 => PromptLevel::Info,
                _ => PromptLevel::Warning,
            };
            (level, outcome.summary(removing), lines.join("\n"))
        }
        Err(e) => (
            PromptLevel::Critical,
            match job {
                Job::Remove => "Unable to Remove Sawit Skills".to_string(),
                _ => "Unable to Install Sawit Skills".to_string(),
            },
            e,
        ),
    };
    let _ = window.update(cx, |_, window, cx| {
        let answer = window.prompt(level, &title, Some(&detail), &["OK"], cx);
        cx.spawn(async move |_| {
            let _ = answer.await;
        })
        .detach();
    });
}

/// Asks before removing: what goes, and that an MCP entry that was there before stays.
pub fn confirm_remove(window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    let answer = window.prompt(
        PromptLevel::Warning,
        "Remove Sawit Skills from every agent?",
        Some(
            "The Sawit skills come out of each agent's skills folder (only Sawit's own copies), and the Sawit block out of each agent's instruction file; everything else in those files stays. A file Sawit made is deleted once it holds nothing else.\n\nSawit's MCP server comes out of the AI clients Install Skills added it to; an entry that was there before stays.",
        ),
        &["Remove", "Cancel"],
        cx,
    );
    cx.spawn(async move |cx| {
        if matches!(answer.await, Ok(0)) {
            cx.update(|cx| run(Job::Remove, None, handle, cx));
        }
    })
    .detach();
}
