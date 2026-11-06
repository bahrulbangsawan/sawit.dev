//! The welcome screen's Set Up: what Sawit works best with and this Mac lacks, ticked to begin
//! with, installed only when the user clicks. Today the GitHub CLI: "installed by default"
//! means preselected here, never installed silently.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::*;

use super::{Welcome, WelcomeEvent};

/// Set Up was put away with Not Now in this run of Sawit: new windows don't offer it again.
static HIDDEN: AtomicBool = AtomicBool::new(false);

impl Welcome {
    /// Set Up: what Sawit works best with and this Mac lacks, ticked, installed only when the
    /// user says so. Today the GitHub CLI.
    pub(super) fn render_setup(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.gh_missing || HIDDEN.load(Ordering::Relaxed) {
            return None;
        }
        let muted = cx.theme().muted_foreground;
        Some(
            Self::section("SET UP", cx).child(
                v_flex()
                    .px_2()
                    .gap_2()
                    .child(
                        Checkbox::new("welcome-setup-gh")
                            .accessibility_label("GitHub CLI")
                            .checked(self.gh_picked)
                            .label("GitHub CLI")
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.gh_picked = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .pl(px(24.))
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child("gh for pull requests, issues and Actions in the terminal. Installs with Homebrew."),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("welcome-install-gh")
                                    .primary()
                                    .small()
                                    .icon(Lucide::Download)
                                    .label("Install GitHub CLI")
                                    .disabled(!self.gh_picked)
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(WelcomeEvent::InstallGitHubCli)
                                    })),
                            )
                            .child(
                                Button::new("welcome-setup-later")
                                    .ghost()
                                    .small()
                                    .label("Not Now")
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        HIDDEN.store(true, Ordering::Relaxed);
                                        cx.notify();
                                    })),
                            ),
                    ),
            ),
        )
    }
}
