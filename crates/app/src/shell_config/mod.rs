//! Config Files sidebar: the shell startup files found on this machine
//! (`~/.zshrc`, `/etc/profile`, …) and every installed coding agent's
//! config files here and in the window's projects (`~/.claude/settings.json`,
//! `.codex/config.toml`, …), each agent's highest priority first. A click
//! opens one in the editor, a right-click offers the Files panel's commands
//! that fit a single file. "Save to GitHub" commits the user's own shell
//! files to a private repository, creating it the first time: it needs the
//! app's one GitHub sign-in, and is off until there is one.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::CoreError;
use sawit_core::agent_config::{self, AgentConfig};
use sawit_core::files;
use sawit_core::mcp_clients::Client;
use sawit_core::recent::tildify;
use sawit_core::shell_config::{self, BACKUP_REPO, Saved, Scope, ShellConfig};

use crate::files::ops::Op;
use crate::files::{
    CopyPath, Delete, Duplicate, OpenInDefaultApp, OpenInTerminal, RevealInFinder, Trash,
    TrashWithoutAsking,
};
use crate::git::account::{self, GitHubAccount};
use crate::rt;
use crate::states::{Banner, Placeholder};

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

    fn render_section(&self, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .h(px(26.))
            .mt_2()
            .px_2()
            .flex()
            .items_center()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(title)
            .into_any_element()
    }

    fn render_group(
        &self,
        group: Group,
        count: usize,
        open: bool,
        filtering: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let title = group.title();
        let (id, logo): (SharedString, AnyElement) = match &group {
            Group::Shell => (
                "config-group-shell".into(),
                Icon::new(Lucide::SquareTerminal)
                    .small()
                    .text_color(muted)
                    .into_any_element(),
            ),
            Group::Agent(client, _) => (
                format!("config-group-{title}").into(),
                crate::mcp_page::client_logo(*client, 16., cx),
            ),
        };
        h_flex()
            .id(id)
            .w_full()
            .h(px(28.))
            .px_2()
            .gap_1p5()
            .when(!filtering, |el| {
                el.cursor_pointer()
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(group.clone(), cx)))
            })
            .child(
                Icon::new(if open {
                    Lucide::ChevronDown
                } else {
                    Lucide::ChevronRight
                })
                .xsmall()
                .text_color(muted),
            )
            .child(logo)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let path = row.path.clone();
        let selected = self.selected.as_ref() == Some(&row.path);
        let focus = self.focus.clone();
        let writable = row.writable;
        // Both lines truncate; the tooltip has them whole.
        let full: SharedString = format!("{}\n{}", row.path.display(), row.detail).into();
        h_flex()
            .id(("config-row", ix))
            .w_full()
            .h(px(40.))
            // In line with the group's title, past its chevron.
            .pl(px(26.))
            .pr_2()
            .gap_2()
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            .on_click(cx.listener({
                let path = path.clone();
                move |this, _, window, cx| {
                    this.selected = Some(path.clone());
                    window.focus(&this.focus, cx);
                    cx.emit(ShellConfigEvent::OpenFile(path.clone()))
                }
            }))
            // The menu's commands act on the row clicked.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    this.selected = Some(path.clone());
                    cx.notify();
                }),
            )
            .context_menu(move |m, _, _| config_menu(m, focus.clone(), writable))
            // One width for both kinds of icon, so names line up.
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(match row.icon {
                        Some(icon) => Icon::new(icon).small().text_color(muted).into_any_element(),
                        None => crate::file_icons::file_icon(&row.path, cx),
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .truncate()
                            .when(row.dim, |el| el.text_color(muted))
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(row.detail.clone()),
                    ),
            )
            .when(!row.writable, |el| {
                el.child(
                    div()
                        .id(("config-lock", ix))
                        .flex_none()
                        .child(Icon::new(Lucide::Lock).xsmall().text_color(muted))
                        .tooltip(|window, cx| {
                            Tooltip::new("Saving asks for your admin password").build(window, cx)
                        }),
                )
            })
            .into_any_element()
    }
}

fn config_menu(menu: PopupMenu, focus: FocusHandle, writable: bool) -> PopupMenu {
    menu.action_context(focus)
        .menu("Open", Box::new(OpenConfig))
        .separator()
        .menu("Reveal in Finder", Box::new(RevealInFinder))
        .menu("Open in Default App", Box::new(OpenInDefaultApp))
        .menu("Open in Terminal", Box::new(OpenInTerminal))
        .separator()
        .menu_with_disabled("Duplicate", Box::new(Duplicate), !writable)
        .menu("Copy Path", Box::new(CopyPath))
        .separator()
        .menu_with_disabled("Trash", Box::new(Trash), !writable)
        .menu_with_disabled("Delete", Box::new(Delete), !writable)
}

/// Commands from the context menu and their shortcuts.
impl ShellConfigPanel {
    /// The selected file, if it is still listed and saving it needs no password.
    fn selected_writable(&self) -> Option<PathBuf> {
        let path = self.selected.as_ref()?;
        let shell = self.configs.iter().map(|c| (&c.path, c.writable));
        let agents = self.agents.iter().map(|c| (&c.path, c.writable));
        shell
            .chain(agents)
            .find(|(p, writable)| *p == path && *writable)
            .map(|(p, _)| p.clone())
    }

    /// Copies the file next to itself (`.zshrc copy`), as a backup.
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(from) = self.selected_writable() else {
            return;
        };
        let (Some(dir), Some(name)) = (from.parent(), from.file_name()) else {
            return;
        };
        let to = files::free_name(dir, &name.to_string_lossy(), false);
        self.run(move || Op::Copy { from, to }.run().map(drop), cx);
    }

    /// Moves the selected file to the Trash, with `ask` once the user says so (⌫; ⌘⌫ doesn't
    /// ask), as in the Files tab: nothing here undoes it.
    fn trash(&mut self, ask: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_writable() else {
            return;
        };
        if !ask {
            self.run(move || Op::Trash(path).run().map(drop), cx);
            return;
        }
        let detail = "You can put it back from the Trash in Finder.";
        let answer = crate::files::ask_to_trash(&path, detail, window, cx);
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |this, cx| {
                    this.run(move || Op::Trash(path).run().map(drop), cx)
                });
            }
        })
        .detach();
    }

    /// Deletes the selected file for good, after asking.
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_writable() else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &delete_question(
                &path,
                std::env::var_os("HOME").map(PathBuf::from).as_deref(),
            ),
            Some("It won't go to the Trash, and this can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    this.run(move || files::remove(&path).map_err(|e| e.to_string()), cx)
                });
            }
        })
        .detach();
    }

    /// Runs a file change off the main thread, then looks again.
    fn run(
        &mut self,
        f: impl FnOnce() -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        let rx = rt::spawn_blocking(cx, f);
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Err(e)) = outcome {
                    this.error = Some(e);
                }
                this.refresh(cx);
            });
        })
        .detach();
    }
}

/// Saving the user's files to GitHub.
impl ShellConfigPanel {
    fn save_to_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        // The button is off while signed out; this holds for its keyboard path too.
        let Some(token) = self.account.read(cx).token().map(String::from) else {
            return;
        };
        self.error = None;
        self.saved = None;
        self.saving = true;
        cx.notify();
        let rx = rt::spawn(cx, {
            let token = token.clone();
            async move { shell_config::plan_backup(&token).await }
        });
        let used = token.clone();
        cx.spawn_in(window, async move |this, cx| {
            let plan = match rx.await {
                Ok(Ok(plan)) => plan,
                outcome => {
                    let _ = this.update(cx, |this, cx| {
                        this.saving = false;
                        if let Ok(Err(e)) = outcome {
                            this.github_failed(e, &used, cx);
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            // Say what's about to leave this Mac when it looks like a secret.
            if let Some((title, detail)) = secrets_question(&plan) {
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Warning,
                        &title,
                        Some(&detail),
                        &["Save Anyway", "Cancel"],
                        cx,
                    )
                }) else {
                    return;
                };
                if !matches!(answer.await, Ok(0)) {
                    let _ = this.update(cx, |this, cx| {
                        this.saving = false;
                        cx.notify();
                    });
                    return;
                }
            }
            let Ok(rx) = this.update(cx, |_, cx| {
                rt::spawn(
                    cx,
                    async move { shell_config::save_backup(&token, &plan).await },
                )
            }) else {
                return;
            };
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match outcome {
                    Ok(Ok(saved)) => this.saved = Some((saved_message(&saved), Some(saved.url))),
                    Ok(Err(e)) => this.github_failed(e, &used, cx),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A save GitHub didn't take: why. GitHub refusing the sign-in `token` itself (revoked,
    /// expired) signs the app out everywhere, which says so once; the button then turns off
    /// with Sign In to GitHub beside it, so this says nothing more.
    fn github_failed(&mut self, e: CoreError, token: &str, cx: &mut Context<Self>) {
        if !account::note_refusal(&e, token, cx) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }

    /// Why Save Shell Files to GitHub is off, or `None` when it's on.
    pub(crate) fn save_blocked(&self, cx: &App) -> Option<SharedString> {
        self.account
            .read(cx)
            .blocked("save your shell files to a private repository")
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let user_files = self
            .configs
            .iter()
            .filter(|c| c.scope == Scope::User)
            .count();
        let blocked = self.save_blocked(cx).is_some();
        v_flex()
            .p_2()
            .gap_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(account::gate(
                Button::new("shell-config-save-github")
                    .outline()
                    .small()
                    .w_full()
                    .icon(Lucide::Github)
                    .label(if self.saving {
                        "Saving to GitHub…"
                    } else {
                        "Save Shell Files to GitHub"
                    })
                    .loading(self.saving)
                    .disabled(self.saving || user_files == 0)
                    .tooltip(format!(
                        "Commit Your {user_files} Shell Files to the Private Repository {BACKUP_REPO}"
                    ))
                    .on_click(cx.listener(|this, _, window, cx| this.save_to_github(window, cx))),
                self.account.read(cx),
                "save your shell files to a private repository",
                cx,
            ))
            .when(blocked, |el| {
                el.child(account::sign_in_link("shell-config-sign-in").w_full())
            })
            .when_some(self.saved.clone(), |el, (message, url)| {
                el.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().flex_1().min_w_0().whitespace_normal().child(message))
                        .when_some(url, |el, url| {
                            el.child(
                                Button::new("shell-config-open-repo")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::ExternalLink)
                                    .tooltip("Open the Repository on GitHub")
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                            )
                        }),
                )
            })
    }
}

/// What to ask before a save that sends lines that look like secrets: on
/// every such save, as a key added since the last one goes up the same way.
fn secrets_question(plan: &shell_config::BackupPlan) -> Option<(String, String)> {
    if plan.secrets.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = plan.secrets.iter().take(6).cloned().collect();
    if plan.secrets.len() > 6 {
        lines.push(format!("and {} more", plan.secrets.len() - 6));
    }
    let repo = if plan.repo.is_some() {
        "the private repository"
    } else {
        "the new private repository"
    };
    Some((
        "Save lines that look like secrets?".into(),
        format!(
            "{}\n\nThey'll be stored in {repo} github.com/{}/{BACKUP_REPO}. Anyone you give access to it can read them.",
            lines.join("\n"),
            plan.login
        ),
    ))
}

fn saved_message(saved: &Saved) -> String {
    let files = if saved.files == 1 {
        "1 file".to_string()
    } else {
        format!("{} files", saved.files)
    };
    match (saved.created, saved.changed) {
        (true, _) => format!("Created the private repository {BACKUP_REPO} and saved {files}."),
        (false, true) => format!("Saved {files} to {BACKUP_REPO}."),
        (false, false) => format!("{BACKUP_REPO} already has these {files}; nothing to save."),
    }
}

/// The question before deleting `path`: by its path, as the list shows it,
/// since many files there share a name (every profile's `settings.json`).
fn delete_question(path: &Path, home: Option<&Path>) -> String {
    format!("Delete “{}” permanently?", tildify(path, home))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Render for ShellConfigPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let filtering = !query.is_empty();
        let groups = self.groups();
        let total: usize = groups.iter().map(|(_, rows)| rows.len()).sum();
        let mut items = Vec::new();
        let mut ix = 0;
        let mut agents_titled = false;
        for (group, rows) in &groups {
            let title = group.title();
            // Searching a group's name lists all of its files.
            let rows: Vec<&Row> = if title.to_lowercase().contains(&query) {
                rows.iter().collect()
            } else {
                rows.iter().filter(|r| r.matches(&query)).collect()
            };
            if rows.is_empty() {
                continue;
            }
            if matches!(group, Group::Agent(..)) && !agents_titled {
                agents_titled = true;
                items.push(self.render_section("AGENTS · HIGHEST PRIORITY FIRST", cx));
            }
            let open = filtering || !self.collapsed.contains(group);
            items.push(self.render_group(group.clone(), rows.len(), open, filtering, cx));
            if open {
                for row in rows {
                    items.push(self.render_row(ix, row, cx));
                    ix += 1;
                }
            }
        }
        let empty = items.is_empty();
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .key_context("ConfigFilesPanel")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &OpenConfig, _, cx| {
                if let Some(path) = this.selected.clone() {
                    cx.emit(ShellConfigEvent::OpenFile(path));
                }
            }))
            .on_action(cx.listener(|this, _: &RevealInFinder, _, cx| {
                if let Some(path) = &this.selected {
                    cx.reveal_path(path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInDefaultApp, _, cx| {
                if let Some(path) = &this.selected {
                    cx.open_with_system(path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInTerminal, _, cx| {
                if let Some(dir) = this.selected.as_deref().and_then(Path::parent) {
                    cx.emit(ShellConfigEvent::OpenTerminal(dir.to_path_buf()));
                }
            }))
            .on_action(cx.listener(|this, _: &Duplicate, _, cx| this.duplicate(cx)))
            .on_action(cx.listener(|this, _: &CopyPath, _, cx| {
                if let Some(path) = &this.selected {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        path.to_string_lossy().into_owned(),
                    ));
                }
            }))
            .on_action(cx.listener(|this, _: &Trash, w, cx| this.trash(true, w, cx)))
            .on_action(cx.listener(|this, _: &TrashWithoutAsking, w, cx| {
                this.trash(false, w, cx)
            }))
            .on_action(cx.listener(|this, _: &Delete, w, cx| this.delete(w, cx)))
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Config Files · {total}")),
                    )
                    .child(
                        Button::new("shell-config-refresh")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Look Again")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                div().p_2().border_b_1().border_color(cx.theme().border).child(
                    Input::new(&self.filter)
                        .small()
                        .prefix(
                            Icon::new(Lucide::Search)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground),
                        )
                        .cleanable(true),
                ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("shell-config-error", e).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.error = None;
                        cx.notify();
                    },
                )))
            })
            .child(
                div()
                    .id("shell-config-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb_2()
                    .when(!empty, |el| el.children(items))
                    .when(empty && filtering, |el| {
                        el.child(
                            Placeholder::new(Lucide::Search, "No matching files")
                                .compact()
                                .detail("Nothing matches this name, path or description.")
                                .action(
                                    Button::new("clear-config-filter")
                                        .small()
                                        .label("Clear Search")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.filter
                                                .update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                ),
                        )
                    })
                    .when(empty && !filtering, |el| {
                        el.child(
                            Placeholder::new(Lucide::FileTerminal, "No config files")
                                .compact()
                                .detail("Files like ~/.zshrc, ~/.claude/settings.json and ~/.codex/config.toml show up here once they exist."),
                        )
                    }),
            )
            .child(self.render_footer(cx))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::{Action, AppContext as _, FocusHandle, Focusable as _, TestAppContext};
    use sawit_core::CoreError;

    use super::{OpenConfig, ShellConfigPanel};
    use crate::files::Trash;

    fn me() -> sawit_core::github::Account {
        sawit_core::github::Account {
            login: "me".into(),
            ..Default::default()
        }
    }

    /// Save Shell Files to GitHub is off while signed out, saying why, with Sign In to GitHub
    /// beside it; the one sign-in turns it on in every panel at once, here two windows'.
    #[gpui_kit::test]
    fn saving_waits_for_the_one_sign_in(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let panels: Vec<_> = (0..2)
            .map(|_| {
                cx.update(|cx| {
                    gpui_kit::open_window(Default::default(), cx, |window, cx| {
                        cx.new(|cx| ShellConfigPanel::new(window, cx))
                    })
                    .unwrap()
                    .1
                })
            })
            .collect();
        cx.run_until_parked();
        for panel in &panels {
            cx.update(|cx| {
                assert_eq!(
                    panel.read(cx).save_blocked(cx).as_deref(),
                    Some("Sign in to GitHub to save your shell files to a private repository")
                )
            });
        }
        let account = cx.update(crate::git::account::global);
        account.update(cx, |a, cx| a.signed_in("gho_1".into(), me(), None, cx));
        cx.run_until_parked();
        for panel in &panels {
            cx.update(|cx| assert_eq!(panel.read(cx).save_blocked(cx), None));
        }
    }

    /// Final seams m5: GitHub refusing the sign-in Sawit saved (revoked, expired) is like having
    /// none: the app signs out everywhere and says so once (the account's notice), and Save
    /// turns off with Sign In to GitHub beside it, so the panel says nothing of its own. Other
    /// failures still say why, and sign nobody out.
    #[gpui_kit::test]
    fn a_refused_github_sign_in_signs_out_everywhere(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let (_, panel) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                cx.new(|cx| ShellConfigPanel::new(window, cx))
            })
            .unwrap()
        });
        let account = cx.update(crate::git::account::global);
        account.update(cx, |a, cx| a.signed_in("gho_1".into(), me(), None, cx));
        let refused = CoreError::GitHub(sawit_core::github::REVOKED.into());
        panel.update(cx, |p, cx| p.github_failed(refused, "gho_1", cx));
        cx.run_until_parked();
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.error, None, "it said more than the account's notice");
            assert!(p.save_blocked(cx).is_some(), "Save is still on");
            assert_eq!(
                account.read(cx).notice(),
                Some(crate::git::account::REVOKED_NOTICE)
            );
        });

        account.update(cx, |a, cx| a.signed_in("gho_2".into(), me(), None, cx));
        let offline = "Unable to reach GitHub. Check your connection and try again.";
        panel.update(cx, |p, cx| {
            p.github_failed(CoreError::GitHub(offline.into()), "gho_2", cx)
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(panel.read(cx).error.as_deref(), Some(offline));
            assert!(account.read(cx).is_signed_in());
        });
    }

    /// The panel lists many files of one name (each profile's `settings.json`, a project's
    /// `.claude/settings.json`): deleting one for good says which, by its path.
    #[test]
    fn deleting_names_the_file_by_its_path() {
        use std::path::Path;
        let home = Some(Path::new("/Users/me"));
        assert_eq!(
            super::delete_question(Path::new("/Users/me/.claude-work/settings.json"), home),
            "Delete “~/.claude-work/settings.json” permanently?"
        );
        assert_eq!(
            super::delete_question(Path::new("/work/app/.claude/settings.json"), home),
            "Delete “/work/app/.claude/settings.json” permanently?"
        );
    }

    /// A key added to `~/.zshrc` after the first save is pointed out on the next save too,
    /// into the repository that's there by then (or one the user made), not only the first.
    #[test]
    fn every_save_with_secrets_asks_first() {
        let plan = |repo: bool, secrets: &[&str]| sawit_core::shell_config::BackupPlan {
            login: "me".into(),
            repo: repo.then(|| sawit_core::github::OwnRepo {
                private: true,
                default_branch: "main".into(),
                html_url: "https://github.com/me/sawit-shell-config".into(),
            }),
            secrets: secrets.iter().map(|s| s.to_string()).collect(),
        };
        let (title, detail) = super::secrets_question(&plan(true, &["~/.zshrc:12 ANTHROPIC_KEY"]))
            .expect("didn't ask");
        assert_eq!(title, "Save lines that look like secrets?");
        assert!(
            detail.starts_with("~/.zshrc:12 ANTHROPIC_KEY\n\n"),
            "{detail}"
        );
        assert!(
            detail.contains("github.com/me/sawit-shell-config"),
            "{detail}"
        );
        assert!(!detail.contains("new private"), "{detail}");
        let (_, first) = super::secrets_question(&plan(false, &["~/.zshrc:1 X_TOKEN"])).unwrap();
        assert!(first.contains("the new private repository"), "{first}");
        assert_eq!(super::secrets_question(&plan(true, &[])), None);
    }

    /// Typing a search runs none of the list's commands. The search box passes on ↩, and ⌫ with
    /// nothing before the cursor (an emptied box), to the bindings around it, where the list's
    /// would open the selected file or move it to the Trash.
    #[gpui_kit::test]
    fn typing_in_the_search_box_runs_no_file_command(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let (window, panel) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                cx.new(|cx| ShellConfigPanel::new(window, cx))
            })
            .unwrap()
        });
        let ran = Rc::new(RefCell::new(Vec::new()));
        let _observer = cx.update(|cx| {
            let ran = ran.clone();
            cx.observe_keystrokes(move |e, _, _| {
                if let Some(action) = &e.action {
                    let name = action.name();
                    if name.starts_with("files::") || name.starts_with("config_files::") {
                        ran.borrow_mut().push(name);
                    }
                }
            })
        });
        let focus = |cx: &mut TestAppContext, handle: FocusHandle| {
            cx.update_window(window, |_, window, cx| window.focus(&handle, cx))
                .unwrap();
            cx.run_until_parked();
        };

        let search = cx.update(|cx| panel.read(cx).filter.focus_handle(cx));
        focus(cx, search);
        cx.simulate_keystrokes(
            window,
            "z s h backspace backspace backspace backspace enter",
        );
        cx.run_until_parked();
        assert_eq!(*ran.borrow(), Vec::<&str>::new(), "the search box ran them");

        // From the list they are the selected file's (none here, so nothing changes).
        let list = cx.update(|cx| panel.read(cx).focus.clone());
        focus(cx, list.clone());
        cx.simulate_keystrokes(window, "enter backspace");
        cx.run_until_parked();
        assert_eq!(*ran.borrow(), ["config_files::OpenConfig", "files::Trash"]);

        // The right-click menu, which looks its keys up from the list, still shows them.
        let key = |cx: &mut TestAppContext, action: &dyn Action| {
            cx.update_window(window, |_, window, _| {
                window
                    .highest_precedence_binding_for_action_in(action, &list)
                    .map(|b| b.keystrokes()[0].key().to_string())
            })
            .unwrap()
        };
        assert_eq!(key(cx, &OpenConfig).as_deref(), Some("enter"));
        assert_eq!(key(cx, &Trash).as_deref(), Some("backspace"));
    }

    /// ⌫ asks before it moves a config file to the Trash, as in the Files tab (FILES-1): one
    /// key meant for a text field moved `~/.zshrc` away without a word, and nothing here undoes
    /// a Trash.
    #[gpui_kit::test]
    fn backspace_asks_before_moving_a_config_file_to_the_trash(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::files::init(cx);
            super::init(cx);
        });
        let (window, panel) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                cx.new(|cx| ShellConfigPanel::new(window, cx))
            })
            .unwrap()
        });
        let zshrc = std::path::PathBuf::from("/sawit-test-no-such-home/.zshrc");
        cx.update(|cx| {
            panel.update(cx, |p, _| {
                p.configs = vec![sawit_core::shell_config::ShellConfig {
                    path: zshrc.clone(),
                    shell: "zsh",
                    note: "Interactive zsh",
                    scope: sawit_core::shell_config::Scope::User,
                    writable: true,
                }];
                p.selected = Some(zshrc.clone());
            })
        });
        let list = cx.update(|cx| panel.read(cx).focus.clone());
        cx.update_window(window, |_, window, cx| window.focus(&list, cx))
            .unwrap();
        cx.run_until_parked();

        // A Trash this started would fail the test at once: no runtime is installed.
        cx.simulate_keystrokes(window, "backspace");
        let (question, _) = cx.pending_prompt().expect("⌫ asks");
        assert_eq!(question, "Move “.zshrc” to the Trash?");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(panel.read(cx).error, None));
    }
}
