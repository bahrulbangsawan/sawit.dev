//! The CLI's version on the New Project page: the one in use beside the title, "Upgrade to
//! vX.Y.Z" when npm has a newer `create-better-t-stack` (asked at most once a day, silent when
//! offline), and "Go back to v3.49.0" once upgraded. Upgrading reads the new release's options
//! first and hides the page's options it no longer takes.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::instances::AppPaths;
use sawit_core::stack_builder::CLI_VERSION;
use sawit_core::stack_builder::version::{self, CliVersions, RELEASES_URL};

use super::NewProjectPage;

/// What the page knows of the CLI's versions, and an upgrade under way or failed.
#[derive(Default)]
pub(super) struct CliState {
    pub versions: CliVersions,
    /// The version an upgrade is reading the options of.
    pub upgrading: Option<String>,
    /// Why the last upgrade didn't happen.
    pub error: Option<String>,
    /// The version an agent's `stack_create` asked for, for its one create.
    pub requested: Option<String>,
}

/// Seconds since 1970, for the daily check.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl NewProjectPage {
    /// The version Create Project runs: an agent's for its create, else the user's choice,
    /// else the pinned one.
    pub(super) fn cli_version(&self) -> String {
        self.cli
            .requested
            .clone()
            .unwrap_or_else(|| self.cli.versions.in_use().to_string())
    }

    /// An agent's `stack_create` with `cli_version`: that version for the next create only.
    pub fn request_cli_version(&mut self, version: Option<String>) {
        self.cli.requested = version.filter(|v| version::stable(v).is_some());
    }

    /// Reads the kept versions, then asks npm for the latest when the last check is a day old.
    pub(super) fn check_cli(&mut self, paths: AppPaths, cx: &mut Context<Self>) {
        self.cli.versions = CliVersions::load(&paths);
        let rx = crate::rt::spawn(cx, async move { version::check(&paths, now()).await });
        cx.spawn(async move |this, cx| {
            if let Ok(versions) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.cli.versions = versions;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Upgrade: reads `version`'s options with the stack's package manager, then uses it.
    fn upgrade_cli(&mut self, target: String, cx: &mut Context<Self>) {
        let Some(paths) = self.paths.clone() else {
            return;
        };
        self.cli.upgrading = Some(target.clone());
        self.cli.error = None;
        cx.notify();
        let pm = self.stack.package_manager;
        let rx = crate::rt::spawn(
            cx,
            async move { version::upgrade(&paths, pm, &target).await },
        );
        cx.spawn(async move |this, cx| {
            let done = rx
                .await
                .unwrap_or_else(|_| Err("The upgrade stopped.".into()));
            let _ = this.update(cx, |this, cx| {
                this.cli.upgrading = None;
                match done {
                    Ok(versions) => this.cli.versions = versions,
                    Err(e) => this.cli.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Go back to the pinned version.
    fn go_back_cli(&mut self, cx: &mut Context<Self>) {
        if let Some(paths) = &self.paths {
            match version::go_back(paths) {
                Ok(versions) => self.cli.versions = versions,
                Err(e) => self.cli.error = Some(e),
            }
        }
        cx.notify();
    }

    /// The line under the page's title: the version in use, and Upgrade or Go Back.
    pub(super) fn render_cli_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let v = &self.cli.versions;
        let chosen = v.chosen.is_some();
        let label = if chosen {
            format!("Using Better-T-Stack v{}", v.in_use())
        } else {
            format!("Better-T-Stack v{}", v.in_use())
        };
        let upgrade = v.upgrade().map(String::from);
        let busy = self.cli.upgrading.is_some() || self.busy();
        let diff = v
            .diff
            .as_ref()
            .filter(|d| chosen && !d.is_empty())
            .map(|d| d.summary());
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        crate::catalog::label("new-project-cli-version", label)
                            .text_xs()
                            .font_medium()
                            .text_color(theme.muted_foreground),
                    )
                    .when_some(upgrade, |row, latest| {
                        let target = latest.clone();
                        row.child(
                            Button::new("new-project-cli-upgrade")
                                .outline()
                                .xsmall()
                                .icon(Lucide::CircleArrowUp)
                                .label(match &self.cli.upgrading {
                                    Some(_) => format!("Checking v{latest}…"),
                                    None => format!("Upgrade to v{latest}"),
                                })
                                .loading(self.cli.upgrading.is_some())
                                .disabled(busy)
                                .tooltip(
                                    "Read the new release's options, then create projects with it",
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.upgrade_cli(target.clone(), cx)
                                })),
                        )
                    })
                    .when(chosen, |row| {
                        row.child(
                            Button::new("new-project-cli-back")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Undo2)
                                .label(format!("Go Back to v{CLI_VERSION}"))
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| this.go_back_cli(cx))),
                        )
                    })
                    .when(chosen || v.upgrade().is_some(), |row| {
                        row.child(
                            Button::new("new-project-cli-notes")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::ExternalLink)
                                .label("Release Notes")
                                .on_click(|_, _, cx| cx.open_url(RELEASES_URL)),
                        )
                    }),
            )
            .when_some(diff, |col, diff| {
                col.child(
                    crate::catalog::label(
                        "new-project-cli-diff",
                        format!("{diff} Options the release doesn't take are hidden."),
                    )
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .whitespace_normal(),
                )
            })
            .when_some(self.cli.error.clone(), |col, error| {
                col.child(
                    crate::catalog::label(
                        "new-project-cli-error",
                        format!("{error} Sawit keeps v{}.", v.in_use()),
                    )
                    .text_xs()
                    .text_color(theme.danger)
                    .whitespace_normal(),
                )
            })
    }
}
