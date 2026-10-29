//! Shell Config sidebar: the shell startup files found on this machine
//! (`~/.zshrc`, `/etc/profile`, …); a click opens one in the editor, a
//! right-click offers the Files panel's commands that fit a single file.
//! "Save to GitHub" commits the user's own files to a private repository,
//! creating it the first time.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::files;
use sawit_core::github;
use sawit_core::recent::tildify;
use sawit_core::shell_config::{self, BACKUP_REPO, Saved, Scope, ShellConfig};

use crate::files::ops::Op;
use crate::files::{
    CopyPath, Delete, Duplicate, OpenInDefaultApp, OpenInTerminal, RevealInFinder, Trash,
};
use crate::rt;
use crate::states::{Banner, Placeholder};

actions!(shell_config, [OpenConfig]);

/// The Files panel's shortcuts for the same commands.
pub fn init(cx: &mut App) {
    let ctx = Some("ShellConfigPanel");
    cx.bind_keys([
        KeyBinding::new("enter", OpenConfig, ctx),
        KeyBinding::new("alt-cmd-r", RevealInFinder, ctx),
        KeyBinding::new("ctrl-shift-enter", OpenInDefaultApp, ctx),
        KeyBinding::new("cmd-d", Duplicate, ctx),
        KeyBinding::new("alt-cmd-c", CopyPath, ctx),
        KeyBinding::new("backspace", Trash, ctx),
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

pub struct ShellConfigPanel {
    configs: Vec<ShellConfig>,
    login_shell: Option<String>,
    /// The file the menu's commands act on.
    selected: Option<PathBuf>,
    error: Option<String>,
    /// Saving to GitHub is under way.
    saving: bool,
    /// The last save's outcome, and the repository's address.
    saved: Option<(String, Option<String>)>,
    focus: FocusHandle,
}

impl ShellConfigPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            configs: shell_config::detect(),
            login_shell: shell_config::login_shell(),
            selected: None,
            error: None,
            saving: false,
            saved: None,
            focus: cx.focus_handle(),
        }
    }

    /// Looks again: files may have been created or removed since.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.configs = shell_config::detect();
        cx.notify();
    }

    fn render_section(&self, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .h(px(26.))
            .mt_1()
            .px_2()
            .flex()
            .items_center()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(title)
            .into_any_element()
    }

    fn render_config(&self, ix: usize, config: &ShellConfig, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let name = tildify(&config.path, home.as_deref());
        let mine = self.login_shell.as_deref() == Some(config.shell);
        let path = config.path.clone();
        let selected = self.selected.as_ref() == Some(&config.path);
        let focus = self.focus.clone();
        let writable = config.writable;
        h_flex()
            .id(("shell-config", ix))
            .w_full()
            .h(px(42.))
            .px_2()
            .gap_2()
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
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
            .child(Icon::new(Lucide::FileTerminal).small().text_color(muted))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(config.note),
                    ),
            )
            .when(mine, |el| {
                el.child(
                    div()
                        .flex_none()
                        .px_1p5()
                        .rounded_full()
                        .text_xs()
                        .bg(cx.theme().success.opacity(0.15))
                        .text_color(cx.theme().success)
                        .child(config.shell),
                )
            })
            .when(!config.writable, |el| {
                el.child(
                    div()
                        .id(("shell-config-lock", ix))
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
        self.configs
            .iter()
            .find(|c| &c.path == path && c.writable)
            .map(|c| c.path.clone())
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
                    .label(if self.saving { "Saving to GitHub…" } else { "Save to GitHub" })
                    .loading(self.saving)
                    .disabled(self.saving || user_files == 0)
                    .tooltip(format!(
                        "Commit Your {user_files} Profile Files to the Private Repository {BACKUP_REPO}"
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
        let mut items = Vec::new();
        for (scope, title) in [
            (Scope::User, "YOUR PROFILE"),
            (Scope::System, "SYSTEM-WIDE"),
        ] {
            let configs: Vec<_> = self
                .configs
                .iter()
                .enumerate()
                .filter(|(_, c)| c.scope == scope)
                .collect();
            if configs.is_empty() {
                continue;
            }
            items.push(self.render_section(title, cx));
            for (ix, config) in configs {
                items.push(self.render_config(ix, config, cx));
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
                            .child(format!("Shell Config · {}", self.configs.len())),
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
                    .when(!empty, |el| el.children(items))
                    .when(empty, |el| {
                        el.child(
                            Placeholder::new(Lucide::FileTerminal, "No shell config files")
                                .compact()
                                .detail("Files like ~/.zshrc, ~/.bashrc and /etc/profile show up here once they exist."),
                        )
                    }),
            )
            .child(self.render_footer(cx))
    }
}
