//! Permissions page: what agents may do on this Mac. Claude Code's permission rules in each of
//! its settings files (user and each profile, each open project's shared and local file, the
//! managed one read only), added, removed and moved between Allow, Ask and Deny; which of
//! Sawit's MCP tools need `confirm` and what the rules make of each; the other agents'
//! permission settings, read only; and the macOS permissions Sawit needs, with buttons to their
//! System Settings panes. Every change shows its diff in a sheet first; Save backs the file up
//! and writes only that change.

mod render;
mod sections;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use gpui_kit::component::input::InputState;
use gpui_kit::*;
use sawit_core::permissions::{
    self as perms, Bucket, Change, OtherAgent, Plan, RulesFile, ToolState,
};

/// A change waiting in the sheet: what it does, to which file, and its plan.
pub(crate) struct Pending {
    pub title: String,
    pub plan: Plan,
}

pub struct PermissionsPage {
    /// The open projects whose files show.
    projects: Vec<PathBuf>,
    files: Vec<RulesFile>,
    tools: Vec<ToolState>,
    others: Vec<OtherAgent>,
    /// The rule field of each file, by its path.
    inputs: Vec<(PathBuf, Entity<InputState>)>,
    pending: Option<Pending>,
    /// What the last change did, or why it couldn't.
    message: Option<(bool, String)>,
    focus: FocusHandle,
    sheet_focus: FocusHandle,
}

impl PermissionsPage {
    pub fn new(projects: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            projects,
            files: Vec::new(),
            tools: Vec::new(),
            others: Vec::new(),
            inputs: Vec::new(),
            pending: None,
            message: None,
            focus: cx.focus_handle(),
            sheet_focus: cx.focus_handle(),
        };
        page.reload();
        page
    }

    /// Shows `projects`' files (the window's open projects), reading every file again.
    pub fn set_projects(&mut self, projects: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.projects = projects;
        self.reload();
        cx.notify();
    }

    /// Reads the files, the tools' states and the other agents' settings again.
    pub(crate) fn reload(&mut self) {
        self.files = perms::claude_files(
            &perms::claude_profiles(),
            &self.projects,
            std::path::Path::new(perms::MANAGED),
        );
        self.tools = perms::sawit_tool_states(&self.files);
        self.others = perms::other_agents(&self.projects);
    }

    /// The rule field of the file at `path`, made on first use.
    pub(crate) fn input(
        &mut self,
        path: &PathBuf,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some((_, input)) = self.inputs.iter().find(|(p, _)| p == path) {
            return input.clone();
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                // Its placeholder is its name for accessibility, so it names the file.
                .placeholder(format!("New Rule for {label}"))
        });
        self.inputs.push((path.clone(), input.clone()));
        input
    }

    /// Plans `change` to the file at `path` and opens the sheet with its diff; or says why
    /// it can't be made.
    pub(crate) fn propose(&mut self, path: PathBuf, change: Change, cx: &mut Context<Self>) {
        let label = self
            .files
            .iter()
            .find(|f| f.path == path)
            .map(|f| f.scope.label())
            .unwrap_or_default();
        match perms::plan(&path, &change) {
            Ok(plan) => {
                self.pending = Some(Pending {
                    title: format!("{} · {label}", change.describe()),
                    plan,
                });
                self.message = None;
            }
            Err(e) => self.message = Some((false, e)),
        }
        cx.notify();
    }

    /// Adds the rule typed in `path`'s field to `to`.
    pub(crate) fn add_typed(
        &mut self,
        path: PathBuf,
        to: Bucket,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self
            .files
            .iter()
            .find(|f| f.path == path)
            .map(|f| f.scope.label())
            .unwrap_or_default();
        let input = self.input(&path, &label, window, cx);
        let rule = input.read(cx).value().trim().to_string();
        if rule.is_empty() {
            self.message = Some((false, "Type a rule first.".into()));
            cx.notify();
            return;
        }
        self.propose(path, Change::Add { rule, to }, cx);
    }

    /// Asks for `rule` on Sawit's tool in the default profile's user settings: added there, or
    /// moved when it's in another list there.
    pub(crate) fn set_tool(&mut self, tool: &str, to: Bucket, cx: &mut Context<Self>) {
        let Some(file) = self
            .files
            .iter()
            .find(|f| f.scope == perms::Scope::User(None))
        else {
            return;
        };
        let rule = format!("mcp__sawit__{tool}");
        let change = match file.rules.bucket_of(&rule) {
            Some(_) => Change::Move { rule, to },
            None => Change::Add { rule, to },
        };
        self.propose(file.path.clone(), change, cx);
    }

    /// Save in the sheet: writes the planned change (after a backup), then reads everything
    /// again.
    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match perms::save(&pending.plan) {
            Ok(()) => {
                let backup = if pending.plan.before.is_empty() {
                    String::new()
                } else {
                    format!(
                        " The file as it was is in {}.",
                        crate::catalog::tilde(&perms::backup_path(&pending.plan.path))
                    )
                };
                self.message = Some((true, format!("Saved: {}.{backup}", pending.title)));
                if let Some((_, input)) = self.inputs.iter().find(|(p, _)| *p == pending.plan.path)
                {
                    input.update(cx, |i, cx| i.set_value("", window, cx));
                }
            }
            Err(e) => self.message = Some((false, e.to_string())),
        }
        self.reload();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

impl Focusable for PermissionsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
