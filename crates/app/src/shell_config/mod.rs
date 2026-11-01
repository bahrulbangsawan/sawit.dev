//! Config Files sidebar: the shell startup files found on this machine
//! (`~/.zshrc`, `/etc/profile`, …) and every installed coding agent's
//! config files here and in the window's projects (`~/.claude/settings.json`,
//! `.codex/config.toml`, …), each agent's highest priority first. A click
//! opens one in the editor, a right-click offers the Files panel's commands
//! that fit a single file. "Save to GitHub" commits the user's own shell
//! files to a private repository, creating it the first time: it needs the
//! app's one GitHub sign-in, and is off until there is one.

mod commands;
mod github;
mod render;

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::agent_config::{self, AgentConfig};
use sawit_core::mcp_clients::Client;
use sawit_core::recent::tildify;
use sawit_core::shell_config::{self, ShellConfig};

use crate::files::{
    CopyPath, Delete, Duplicate, OpenInDefaultApp, RevealInFinder, Trash, TrashWithoutAsking,
};
use crate::git::account::{self, GitHubAccount};

actions!(config_files, [OpenConfig]);

/// The Files panel's shortcuts for the same commands. ↩ and ⌫ act on the list only while the
/// search box doesn't have the keyboard, like the keymap editor's table keys: the box passes on ↩,
/// and ⌫ with nothing before the cursor, and they would open the selected file or move it to the
/// Trash.
pub fn init(cx: &mut App) {
    let ctx = Some("ConfigFilesPanel");
    let list = Some("ConfigFilesPanel && !Input");
    cx.bind_keys([
        KeyBinding::new("enter", OpenConfig, list),
        KeyBinding::new("alt-cmd-r", RevealInFinder, ctx),
        KeyBinding::new("ctrl-shift-enter", OpenInDefaultApp, ctx),
        KeyBinding::new("cmd-d", Duplicate, ctx),
        KeyBinding::new("alt-cmd-c", CopyPath, ctx),
        KeyBinding::new("backspace", Trash, list),
        KeyBinding::new("cmd-backspace", TrashWithoutAsking, list),
        KeyBinding::new("alt-cmd-backspace", Delete, ctx),
    ]);
}

pub enum ShellConfigEvent {
    OpenFile(PathBuf),
    OpenTerminal(PathBuf),
}

impl EventEmitter<ShellConfigEvent> for ShellConfigPanel {}

/// A collapsible group of rows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Group {
    Shell,
    /// An agent's files, or with a profile that profile's own: profiles are
    /// alternatives, each its own group.
    Agent(Client, Option<String>),
}

impl Group {
    /// "Claude Code", or "Claude Code · work" for a profile.
    fn title(&self) -> String {
        match self {
            Group::Shell => "Shell".into(),
            Group::Agent(client, profile) => agent_config::group_title(*client, profile.as_deref()),
        }
    }
}

/// One file, as the list shows it.
struct Row {
    path: PathBuf,
    name: String,
    detail: String,
    /// `None` for the Files panel's icon for the file's type.
    icon: Option<Lucide>,
    writable: bool,
    /// The login shell doesn't read it.
    dim: bool,
}

impl Row {
    fn matches(&self, query: &str) -> bool {
        self.name.to_lowercase().contains(query)
            || self.detail.to_lowercase().contains(query)
            || self.path.to_string_lossy().to_lowercase().contains(query)
    }
}

pub struct ShellConfigPanel {
    configs: Vec<ShellConfig>,
    agents: Vec<AgentConfig>,
    /// The window's project folders, whose agent files are listed.
    projects: Vec<PathBuf>,
    login_shell: Option<String>,
    filter: Entity<InputState>,
    collapsed: Vec<Group>,
    /// The file the menu's commands act on.
    selected: Option<PathBuf>,
    error: Option<String>,
    /// Saving to GitHub is under way.
    saving: bool,
    /// The last save's outcome, and the repository's address.
    saved: Option<(String, Option<String>)>,
    /// The app's GitHub sign-in, which saving needs.
    account: Entity<GitHubAccount>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl ShellConfigPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search config files"));
        let account = account::global(cx);
        let subscriptions = vec![
            cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            // Save turns on and off with the one sign-in.
            cx.observe(&account, |_, _, cx| cx.notify()),
        ];
        Self {
            configs: shell_config::detect(),
            agents: agent_config::detect(&[]),
            projects: Vec::new(),
            login_shell: shell_config::login_shell(),
            filter,
            collapsed: Vec::new(),
            selected: None,
            error: None,
            saving: false,
            saved: None,
            account,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Looks again: files may have been created or removed since.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.configs = shell_config::detect();
        self.agents = agent_config::detect(&self.projects);
        cx.notify();
    }

    /// Lists the agent files of `projects` too, and looks again.
    pub fn set_projects(&mut self, projects: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.projects = projects;
        self.refresh(cx);
    }

    /// Whether the login shell reads files meant for `shell`.
    fn reads(&self, shell: &str) -> bool {
        shell_config::reads(self.login_shell.as_deref(), shell)
    }

    /// Every group that has files, with its rows.
    fn groups(&self) -> Vec<(Group, Vec<Row>)> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut groups = Vec::new();
        let shell: Vec<Row> = self
            .configs
            .iter()
            .map(|c| {
                let read = self.reads(c.shell);
                let detail = match (&self.login_shell, read) {
                    (Some(login), false) => format!("Not read by {login} · {}", c.note),
                    _ => c.note.to_string(),
                };
                Row {
                    path: c.path.clone(),
                    name: tildify(&c.path, home.as_deref()),
                    detail,
                    icon: Some(Lucide::FileTerminal),
                    writable: c.writable,
                    dim: !read,
                }
            })
            .collect();
        if !shell.is_empty() {
            groups.push((Group::Shell, shell));
        }
        for config in &self.agents {
            let group = Group::Agent(config.client, config.profile.clone());
            if groups.last().is_none_or(|(g, _)| *g != group) {
                groups.push((group, Vec::new()));
            }
            // A project's files by the project's name: `app/CLAUDE.md`.
            let name = match &config.project {
                Some(project) => {
                    let rel = config.path.strip_prefix(project).unwrap_or(&config.path);
                    format!("{}/{}", file_name(project), rel.display())
                }
                None => tildify(&config.path, home.as_deref()),
            };
            if let Some((_, rows)) = groups.last_mut() {
                rows.push(Row {
                    path: config.path.clone(),
                    name,
                    detail: format!("{} · {}", config.layer.label(), config.note),
                    icon: None,
                    writable: config.writable,
                    dim: false,
                });
            }
        }
        groups
    }

    fn toggle(&mut self, group: Group, cx: &mut Context<Self>) {
        if let Some(ix) = self.collapsed.iter().position(|g| *g == group) {
            self.collapsed.remove(ix);
        } else {
            self.collapsed.push(group);
        }
        cx.notify();
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
