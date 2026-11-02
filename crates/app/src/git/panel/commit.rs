//! The Git sidebar's commit box: the message (⌘↩ commits), Amend, and the Commit button, which
//! commits every change when nothing is staged.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git::{self, Status};

use super::GitPanel;

impl GitPanel {
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).value().trim().to_string();
        let Some(status) = self.store.read(cx).status.clone() else {
            return;
        };
        if message.is_empty() {
            self.store.update(cx, |s, cx| {
                s.error = Some("Write a commit message first.".into());
                cx.notify();
            });
            return;
        }
        let amend = self.amend;
        // Like VS Code: with nothing staged, commit every change.
        let stage_all = status.staged().next().is_none() && !amend;
        if stage_all && status.unstaged().next().is_none() {
            self.store.update(cx, |s, cx| {
                s.error = Some("There's nothing to commit.".into());
                cx.notify();
            });
            return;
        }
        self.store.update(cx, |s, cx| {
            s.run("Committing…", cx, move |repo, _| async move {
                if stage_all {
                    git::stage_all(&repo).await?;
                }
                git::commit(&repo, &message, amend).await?;
                Ok(String::new())
            })
        });
        self.amend = false;
        self.message.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn toggle_amend(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = on;
        cx.notify();
        if !on || !self.message.read(cx).value().trim().is_empty() {
            return;
        }
        let Some(repo) = self.store.read(cx).repo.clone() else {
            return;
        };
        let rx = crate::rt::spawn(cx, async move { git::last_message(&repo).await });
        let message = self.message.clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(text)) = rx.await {
                let _ = message.update_in(cx, |s, window, cx| {
                    if s.value().trim().is_empty() {
                        s.set_value(text, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn render_commit_box(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let store = self.store.read(cx);
        let no_status = Status::default();
        let status = store.status.as_ref().unwrap_or(&no_status);
        let busy = store.busy.is_some();
        let staged = status.staged().count();
        let changed = status.unstaged().count();
        let conflicts = status.conflicted().count();
        let label = if self.amend {
            "Amend Last Commit".to_string()
        } else if staged > 0 {
            format!("Commit {staged} File{}", if staged == 1 { "" } else { "s" })
        } else if changed > 0 {
            format!("Commit All {changed}")
        } else {
            "Commit".to_string()
        };
        let empty = self.message.read(cx).value().trim().is_empty();
        let commit_tooltip = match commit_keys(cx) {
            keys if keys.is_empty() => "Commit".to_string(),
            keys => format!("Commit ({keys})"),
        };
        v_flex()
            .p_2()
            .gap_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .key_context("GitCommitMessage")
                    .on_action(cx.listener(|this, _: &crate::git::Commit, window, cx| {
                        this.commit(window, cx)
                    }))
                    .child(
                        Textarea::new(&self.message)
                            .h(px(64.))
                            .aria_label("Commit message"),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("git-amend")
                            .label("Amend")
                            .checked(self.amend)
                            .tooltip("Replace the last commit instead of adding one")
                            .disabled(status.head.is_none())
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.toggle_amend(*checked, window, cx)
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("git-commit")
                            .primary()
                            .small()
                            .icon(Lucide::Check)
                            .label(label)
                            .disabled(
                                busy || empty
                                    || conflicts > 0
                                    || (staged + changed == 0 && !self.amend),
                            )
                            .map(|button| {
                                if conflicts > 0 {
                                    button.tooltip("Resolve the conflicts first")
                                } else {
                                    button.tooltip(commit_tooltip)
                                }
                            })
                            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    ),
            )
    }
}

/// The keys that commit from the message box, as the keymap binds them (`⌘↩`), or none.
fn commit_keys(cx: &App) -> SharedString {
    crate::keymap::keys_for_path(&crate::git::Commit, &["GitCommitMessage", "Input"], cx)
}

/// The commit message box's placeholder, with the keys that commit in it as the keymap binds
/// them.
pub(in crate::git) fn commit_placeholder(cx: &App) -> String {
    match commit_keys(cx) {
        keys if keys.is_empty() => "Commit message".into(),
        keys => format!("Commit message ({keys} to commit)"),
    }
}
