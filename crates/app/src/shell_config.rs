//! Config Files sidebar: the shell startup files found on this machine
//! (`~/.zshrc`, `/etc/profile`, …) and every installed coding agent's
//! config files here and in the window's projects (`~/.claude/settings.json`,
//! `.codex/config.toml`, …), each agent's highest priority first. A click
//! opens one in the editor, a right-click offers the Files panel's commands
//! that fit a single file. "Save to GitHub" commits the user's own shell
//! files to a private repository, creating it the first time.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_config::{self, AgentConfig};
use sawit_core::files;
use sawit_core::github;
use sawit_core::mcp_clients::Client;
use sawit_core::recent::tildify;
use sawit_core::shell_config::{self, BACKUP_REPO, Saved, Scope, ShellConfig};

use crate::files::ops::Op;
use crate::files::{
    CopyPath, Delete, Duplicate, OpenInDefaultApp, OpenInTerminal, RevealInFinder, Trash,
};
use crate::rt;
use crate::states::{Banner, Placeholder};

actions!(shell_config, [OpenConfig]);

/// The Files panel's shortcuts for the same commands. ↩ and ⌫ act on the list only while the
/// search box doesn't have the keyboard, like the keymap editor's table keys: the box passes on ↩,
/// and ⌫ with nothing before the cursor, and they would open the selected file or move it to the
/// Trash.
pub fn init(cx: &mut App) {
    let ctx = Some("ShellConfigPanel");
    let list = Some("ShellConfigPanel && !Input");
    cx.bind_keys([
        KeyBinding::new("enter", OpenConfig, list),
        KeyBinding::new("alt-cmd-r", RevealInFinder, ctx),
        KeyBinding::new("ctrl-shift-enter", OpenInDefaultApp, ctx),
        KeyBinding::new("cmd-d", Duplicate, ctx),
        KeyBinding::new("alt-cmd-c", CopyPath, ctx),
        KeyBinding::new("backspace", Trash, list),
        KeyBinding::new("alt-cmd-backspace", Delete, ctx),
    ]);
}

pub enum ShellConfigEvent {
    OpenFile(PathBuf),
    OpenTerminal(PathBuf),
    /// Saving needs a GitHub sign-in first.
    SignInToGitHub,
}

impl EventEmitter<ShellConfigEvent> for ShellConfigPanel {}

/// A collapsible group of rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Shell,
    Agent(Client),
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
    focus: FocusHandle,
    _subscription: Subscription,
}

impl ShellConfigPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search config files"));
        let subscription = cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
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
            focus: cx.focus_handle(),
            _subscription: subscription,
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
        match self.login_shell.as_deref() {
            Some(login) => login == shell || (login == "bash" && shell == "sh"),
            None => true,
        }
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
            let group = Group::Agent(config.client);
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
        let (id, title, logo): (SharedString, &str, AnyElement) = match group {
            Group::Shell => (
                "config-group-shell".into(),
                "Shell",
                Icon::new(Lucide::SquareTerminal)
                    .small()
                    .text_color(muted)
                    .into_any_element(),
            ),
            Group::Agent(client) => (
                format!("config-group-{}", client.name()).into(),
                client.name(),
                crate::mcp_page::client_logo(client, 16., cx),
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
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(group, cx)))
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
                    .child(title.to_string()),
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

    fn trash(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected_writable() else {
            return;
        };
        self.run(move || Op::Trash(path).run().map(drop), cx);
    }

    /// Deletes the selected file for good, after asking.
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_writable() else {
            return;
        };
        let name = file_name(&path);
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete “{name}” permanently?"),
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
        self.error = None;
        self.saved = None;
        let token = match github::load_token() {
            Ok(Some(token)) => token,
            Ok(None) => {
                self.saved = Some((
                    "Sign in to GitHub in the GitHub tab, then save again.".into(),
                    None,
                ));
                cx.emit(ShellConfigEvent::SignInToGitHub);
                cx.notify();
                return;
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        self.saving = true;
        cx.notify();
        let rx = rt::spawn(cx, {
            let token = token.clone();
            async move { shell_config::plan_backup(&token).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let plan = match rx.await {
                Ok(Ok(plan)) => plan,
                outcome => {
                    let _ = this.update(cx, |this, cx| {
                        this.saving = false;
                        if let Ok(Err(e)) = outcome {
                            this.error = Some(e.to_string());
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            // The first save creates the repository: say what's about to
            // leave this Mac when it looks like a secret.
            if plan.repo.is_none() && !plan.secrets.is_empty() {
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    let mut lines: Vec<&str> = plan.secrets.iter().take(6).map(String::as_str).collect();
                    let more = plan.secrets.len().saturating_sub(lines.len());
                    let more = format!("and {more} more");
                    if plan.secrets.len() > 6 {
                        lines.push(&more);
                    }
                    window.prompt(
                        PromptLevel::Warning,
                        "Save lines that look like secrets?",
                        Some(&format!(
                            "{}\n\nThey'll be stored in the new private repository github.com/{}/{BACKUP_REPO}. Anyone you give access to it can read them.",
                            lines.join("\n"),
                            plan.login
                        )),
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
                rt::spawn(cx, async move { shell_config::save_backup(&token, &plan).await })
            }) else {
                return;
            };
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match outcome {
                    Ok(Ok(saved)) => this.saved = Some((saved_message(&saved), Some(saved.url))),
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let user_files = self
            .configs
            .iter()
            .filter(|c| c.scope == Scope::User)
            .count();
        v_flex()
            .p_2()
            .gap_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
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
            )
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
            let title = match group {
                Group::Shell => "Shell",
                Group::Agent(client) => client.name(),
            };
            // Searching a group's name lists all of its files.
            let rows: Vec<&Row> = if title.to_lowercase().contains(&query) {
                rows.iter().collect()
            } else {
                rows.iter().filter(|r| r.matches(&query)).collect()
            };
            if rows.is_empty() {
                continue;
            }
            if matches!(group, Group::Agent(_)) && !agents_titled {
                agents_titled = true;
                items.push(self.render_section("AGENTS · HIGHEST PRIORITY FIRST", cx));
            }
            let open = filtering || !self.collapsed.contains(group);
            items.push(self.render_group(*group, rows.len(), open, filtering, cx));
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
            .key_context("ShellConfigPanel")
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
            .on_action(cx.listener(|this, _: &Trash, _, cx| this.trash(cx)))
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

    use super::{OpenConfig, ShellConfigPanel};
    use crate::files::Trash;

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
                    if name.starts_with("files::") || name.starts_with("shell_config::") {
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
        assert_eq!(*ran.borrow(), ["shell_config::OpenConfig", "files::Trash"]);

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
}
