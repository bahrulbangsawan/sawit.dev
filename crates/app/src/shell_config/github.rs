//! Save Shell Files to GitHub: the user's own shell files committed to a private repository with
//! the app's one GitHub sign-in, asking first when lines look like secrets, and the footer that
//! offers it.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::CoreError;
use sawit_core::shell_config::{self, BACKUP_REPO, Saved, Scope};

use super::ShellConfigPanel;
use crate::git::account;
use crate::rt;

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
    pub(super) fn github_failed(&mut self, e: CoreError, token: &str, cx: &mut Context<Self>) {
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

    pub(super) fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
pub(super) fn secrets_question(plan: &shell_config::BackupPlan) -> Option<(String, String)> {
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
