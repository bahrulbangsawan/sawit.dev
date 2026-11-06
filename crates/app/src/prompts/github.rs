//! The prompt library on GitHub, at the foot of the Prompts sidebar: the repository (a private
//! `sawit-prompts` unless the user names another), Back Up Now, Restore, and an optional backup a
//! little after each change. A prompt changed both here and on GitHub is listed with Keep Mine and
//! Keep GitHub's, never overwritten.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::prompt_sync::{self, BackupSettings, PromptSync, SyncOutcome};

use super::panel::PromptsPanel;
use crate::git::account::{self, SignInToGitHub};
use crate::rt;

/// How long after the last change an automatic backup waits.
const AUTO_DELAY: Duration = Duration::from_secs(30);

/// The backup's state in the sidebar.
pub(super) struct PromptsGitHub {
    pub settings: BackupSettings,
    pub repo: Entity<InputState>,
    pub busy: Option<&'static str>,
    pub message: Option<(String, bool)>,
    pub conflicts: Vec<PromptSync>,
    /// The library as last backed up or restored, so an automatic backup skips no-ops.
    pub synced_library: Option<sawit_core::prompts::Library>,
    pub auto_scheduled: bool,
}

fn settings_path() -> PathBuf {
    AppPaths::default_macos().root.join(prompt_sync::SETTINGS)
}

fn state_path() -> PathBuf {
    AppPaths::default_macos().root.join(prompt_sync::SYNC_STATE)
}

impl PromptsGitHub {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let settings = BackupSettings::load(&settings_path());
        let name = settings.repo.clone();
        let repo = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(prompt_sync::DEFAULT_REPO);
            s.set_value(name, window, cx);
            s
        });
        PromptsGitHub {
            settings,
            repo,
            busy: None,
            message: None,
            conflicts: Vec::new(),
            synced_library: None,
            auto_scheduled: false,
        }
    }
}

impl PromptsPanel {
    /// The repository as typed, else the default; kept for next time.
    fn backup_settings(&mut self, cx: &mut Context<Self>) -> BackupSettings {
        let typed = self.github.repo.read(cx).value().trim().to_string();
        self.github.settings.repo = if typed.is_empty() {
            prompt_sync::DEFAULT_REPO.into()
        } else {
            typed
        };
        let _ = self.github.settings.save(&settings_path());
        self.github.settings.clone()
    }

    /// Back Up Now (and the automatic backup): the prompts changed here go up. `keep_local`: ids
    /// changed on both sides whose version here wins (Keep Mine).
    pub(super) fn back_up(&mut self, keep_local: Vec<String>, cx: &mut Context<Self>) {
        let Some(token) = account::global(cx).read(cx).token().map(String::from) else {
            return;
        };
        if self.github.busy.is_some() {
            return;
        }
        let settings = self.backup_settings(cx);
        let library = self.store.read(cx).library.clone();
        self.github.busy = Some("Backing Up…");
        self.github.message = None;
        cx.notify();
        let rx = rt::spawn(cx, {
            let token = token.clone();
            let library = library.clone();
            async move {
                prompt_sync::back_up(&token, &library, &settings, &state_path(), &keep_local).await
            }
        });
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.github.busy = None;
                match outcome {
                    Ok(done) => {
                        this.github.synced_library = Some(library);
                        this.done(&done, "Backed up", cx);
                    }
                    Err(e) => this.failed(e, &token, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Restore: asks, then brings in the prompts changed on GitHub. `keep_github`: ids changed on
    /// both sides whose GitHub version wins (Keep GitHub's).
    pub(super) fn restore(
        &mut self,
        keep_github: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(token) = account::global(cx).read(cx).token().map(String::from) else {
            return;
        };
        if self.github.busy.is_some() {
            return;
        }
        let settings = self.backup_settings(cx);
        let detail = if keep_github.is_empty() {
            format!(
                "Prompts changed on GitHub (github.com/…/{}) replace the ones here, and prompts only on GitHub come back. Prompts you changed here since the last backup stay as they are.",
                settings.repo
            )
        } else {
            "Your version of these prompts is replaced with GitHub's.".to_string()
        };
        let answer =
            crate::confirm::Confirm::new("Restore prompts from GitHub?", "Restore Prompts")
                .detail(detail)
                .ask(window, cx);
        cx.spawn_in(window, async move |this, cx| {
            if !answer.await {
                return;
            }
            let Ok(rx) = this.update(cx, |this, cx| {
                this.github.busy = Some("Restoring…");
                this.github.message = None;
                cx.notify();
                let mut library = this.store.read(cx).library.clone();
                let token = token.clone();
                rt::spawn(cx, async move {
                    let done = prompt_sync::restore(
                        &token,
                        &mut library,
                        &settings,
                        &state_path(),
                        &keep_github,
                    )
                    .await;
                    done.map(|d| (d, library))
                })
            }) else {
                return;
            };
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.github.busy = None;
                match outcome {
                    Ok(Ok((done, restored))) => {
                        this.store.update(cx, |s, cx| {
                            for p in restored.items {
                                if done.synced_ids.contains(&p.id) {
                                    s.put(p, cx);
                                }
                            }
                        });
                        this.github.synced_library = Some(this.store.read(cx).library.clone());
                        this.done(&done, "Restored", cx);
                    }
                    Ok(Err(e)) => this.failed(e, &token, cx),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn done(&mut self, done: &SyncOutcome, verb: &str, _: &mut Context<Self>) {
        let n = done.synced.len();
        let what = if n == 1 {
            "1 prompt".to_string()
        } else {
            format!("{n} prompts")
        };
        let mut text = if n == 0 {
            format!("Nothing to sync with {}.", done.repo)
        } else {
            format!("{verb} {what} ({}).", done.repo)
        };
        if done.created_repo {
            text = format!("Created the private repository {}. {text}", done.repo);
        }
        if !done.conflicts.is_empty() {
            text.push_str(&format!(
                " {} changed both here and on GitHub: choose below.",
                done.conflicts.len()
            ));
        }
        self.github.conflicts = done.conflicts.clone();
        self.github.message = Some((text, false));
    }

    fn failed(&mut self, e: sawit_core::CoreError, token: &str, cx: &mut Context<Self>) {
        account::note_refusal(&e, token, cx);
        self.github.message = Some((e.to_string(), true));
    }

    /// A change to the library: with automatic backup on, backs up a little after the last one.
    pub(super) fn library_changed(&mut self, cx: &mut Context<Self>) {
        if !self.github.settings.auto || self.github.auto_scheduled {
            return;
        }
        if account::global(cx).read(cx).token().is_none() {
            return;
        }
        self.github.auto_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTO_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.github.auto_scheduled = false;
                let now = &this.store.read(cx).library;
                if this.github.synced_library.as_ref() != Some(now) {
                    this.back_up(Vec::new(), cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn render_github(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let account = account::global(cx);
        let signed_in = account.read(cx).is_signed_in();
        let busy = self.github.busy;
        v_flex()
            .p_2()
            .gap_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(!signed_in, |el| {
                el.child(
                    Button::new("prompts-sign-in")
                        .outline()
                        .small()
                        .w_full()
                        .icon(Lucide::Github)
                        .label("Sign In to Back Up Prompts")
                        .tooltip("Back up your prompts to a private GitHub repository")
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(SignInToGitHub), cx)
                        }),
                )
            })
            .when(signed_in, |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(Lucide::Github).xsmall().text_color(muted))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.github.repo).xsmall()),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("prompts-back-up")
                                .outline()
                                .small()
                                .flex_1()
                                .icon(Lucide::Upload)
                                .label(
                                    busy.filter(|b| b.starts_with("Back"))
                                        .unwrap_or("Back Up Now"),
                                )
                                .disabled(busy.is_some())
                                .tooltip(
                                    "Back Up the Prompts Changed Here to the Private Repository",
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.back_up(Vec::new(), cx)),
                                ),
                        )
                        .child(
                            Button::new("prompts-restore")
                                .outline()
                                .small()
                                .flex_1()
                                .icon(Lucide::Download)
                                .label(busy.filter(|b| b.starts_with("Rest")).unwrap_or("Restore"))
                                .disabled(busy.is_some())
                                .tooltip("Bring In the Prompts Changed on GitHub")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.restore(Vec::new(), window, cx)
                                })),
                        ),
                )
                .child(
                    Checkbox::new("prompts-auto-backup")
                        .label("Back Up Automatically")
                        .checked(self.github.settings.auto)
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            this.github.settings.auto = *on;
                            let _ = this.github.settings.save(&settings_path());
                            if *on {
                                this.library_changed(cx);
                            }
                            cx.notify();
                        })),
                )
            })
            .when_some(self.github.message.clone(), |el, (text, error)| {
                el.child(
                    div()
                        .text_xs()
                        .whitespace_normal()
                        .text_color(if error { cx.theme().danger } else { muted })
                        .child(text),
                )
            })
            .children(self.github.conflicts.iter().map(|c| {
                let (mine, theirs) = (c.id.clone(), c.id.clone());
                let title = if c.title.is_empty() {
                    c.id.clone()
                } else {
                    c.title.clone()
                };
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .truncate()
                            .child(format!("“{title}” changed here and on GitHub")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "prompt-keep-mine-{}",
                                    c.id
                                )))
                                .ghost()
                                .xsmall()
                                .label("Keep Mine")
                                .tooltip("Back Up This Mac's Version over GitHub's")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.back_up(vec![mine.clone()], cx),
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "prompt-keep-github-{}",
                                    c.id
                                )))
                                .ghost()
                                .xsmall()
                                .label("Keep GitHub's")
                                .tooltip("Replace This Mac's Version with GitHub's")
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.restore(vec![theirs.clone()], window, cx)
                                    },
                                )),
                            ),
                    )
            }))
    }
}
