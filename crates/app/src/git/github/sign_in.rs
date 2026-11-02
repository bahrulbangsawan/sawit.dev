//! The GitHub tab's sign-in card (the device flow's code and steps, or what signing in gives),
//! and the banner for a sign-in that lacks a scope Sawit asks for.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github;

use super::GitHubView;
use crate::git::account::SignIn;
use crate::states::Banner;

impl GitHubView {
    pub(super) fn render_sign_in(&self, cx: &mut Context<Self>) -> AnyElement {
        let account = self.account.read(cx);
        let signed_in = account.is_signed_in();
        let notice = account.notice().map(String::from);
        let muted = cx.theme().muted_foreground;
        let card = match &account.sign_in {
            SignIn::Waiting(device) => {
                let (code, url) = (device.user_code.clone(), device.verification_uri.clone());
                let page = url.trim_start_matches("https://").to_string();
                let copied = self.code_copied;
                v_flex()
                    .gap_5()
                    .child(card_heading(
                        "Enter this code on GitHub",
                        "Sawit is waiting for you to approve it.",
                        cx,
                    ))
                    .child(
                        div()
                            .w_full()
                            .py_4()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .text_center()
                            .text_3xl()
                            .font_semibold()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(code.clone()),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(step(1, "Copy the code above.".into(), cx))
                            .child(step(2, format!("Paste it at {page}."), cx))
                            .child(step(3, "Click Authorize, then come back here.".into(), cx)),
                    )
                    .child(
                        Button::new("gh-open-device")
                            .primary()
                            .w_full()
                            .icon(if copied { Lucide::Check } else { Lucide::Copy })
                            .label(if copied {
                                "Copied, Opening GitHub…"
                            } else {
                                "Copy Code & Open GitHub"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                                cx.open_url(&url);
                                this.flash_copied(cx);
                            })),
                    )
                    .child(
                        h_flex()
                            .pt_3()
                            .gap_2()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .text_sm()
                            .text_color(muted)
                            .child(Spinner::new().small())
                            .child(div().flex_1().child("Waiting for approval"))
                            .child(
                                Button::new("gh-cancel-sign-in")
                                    .ghost()
                                    .small()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.account.update(cx, |a, cx| a.cancel_sign_in(cx))
                                    })),
                            ),
                    )
            }
            state => {
                let starting = matches!(state, SignIn::Starting);
                let failed = match state {
                    SignIn::Failed(e) => Some(e.clone()),
                    _ => None,
                };
                let perk = |text: &'static str, cx: &App| {
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(
                            Icon::new(Lucide::Check)
                                .small()
                                .text_color(cx.theme().success),
                        )
                        .child(text)
                };
                let scopes = github::SCOPES
                    .iter()
                    .map(|(scope, _)| *scope)
                    .collect::<Vec<_>>()
                    .join(", ");
                v_flex()
                    .gap_5()
                    .child(card_heading(
                        "Connect your GitHub account",
                        "One sign-in for every GitHub feature in Sawit.",
                        cx,
                    ))
                    .when_some(notice, |el, notice| {
                        el.child(Banner::error("gh-sign-in-notice", notice).on_dismiss(
                            cx.listener(|this, _, _, cx| {
                                this.account.update(cx, |a, cx| a.dismiss_notice(cx))
                            }),
                        ))
                    })
                    .child(
                        v_flex()
                            .gap_2()
                            .child(perk("Pull requests and issues, with their timelines", cx))
                            .child(perk("Comment, close and merge", cx))
                            .child(perk("Clone your repositories", cx))
                            .child(perk("Push and pull over HTTPS without password prompts", cx))
                            .child(perk("Save your shell files to a private repository", cx)),
                    )
                    .when_some(failed, |el, e| el.child(Banner::error("gh-sign-in-error", e)))
                    .child(
                        Button::new("gh-sign-in")
                            .primary()
                            .w_full()
                            .icon(Lucide::Github)
                            .label(if starting { "Contacting GitHub…" } else { "Sign In with GitHub" })
                            .loading(starting)
                            .disabled(starting)
                            .on_click(cx.listener(|this, _, _, cx| this.account.update(cx, |a, cx| a.sign_in(cx)))),
                    )
                    .when(signed_in, |el| {
                        el.child(
                            Button::new("gh-sign-in-not-now")
                                .ghost()
                                .small()
                                .w_full()
                                .label("Keep the Current Sign-In")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.account.update(cx, |a, cx| a.cancel_sign_in(cx))
                                })),
                        )
                    })
                    .child(
                        h_flex()
                            .items_start()
                            .gap_1p5()
                            .text_xs()
                            .text_color(muted)
                            .child(div().pt(px(2.)).child(Icon::new(Lucide::KeyRound).xsmall()))
                            .child(div().flex_1().min_w_0().child(format!(
                                "Sawit asks GitHub for {scopes}: repo for your repositories, pull requests and issues, workflow to push changes to GitHub Actions workflows, read:user for your name and avatar."
                            ))),
                    )
                    .child(
                        h_flex()
                            .items_start()
                            .gap_1p5()
                            .text_xs()
                            .text_color(muted)
                            .child(div().pt(px(2.)).child(Icon::new(Lucide::Lock).xsmall()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child("Sawit never sees your password. Your sign-in is kept in the macOS Keychain."),
                            ),
                    )
            }
        };
        div()
            .id("gh-sign-in-page")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .min_h_full()
                    .p_6()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(400.))
                            .p_6()
                            .rounded(cx.theme().radius * 2.)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .child(card),
                    ),
            )
            .into_any_element()
    }

    /// The copy button says "Copied" for a moment.
    fn flash_copied(&mut self, cx: &mut Context<Self>) {
        self.code_copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(2500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.code_copied = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Says what the sign-in lacks of what Sawit asks for (one from before Sawit asked for
    /// `workflow`), with a new sign-in to allow it.
    pub(super) fn render_missing_scopes(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let missing = self.account.read(cx).missing_scopes();
        if missing.is_empty() {
            return None;
        }
        let what: Vec<&str> = github::SCOPES
            .iter()
            .filter(|(scope, _)| missing.contains(scope))
            .map(|(_, needed_for)| *needed_for)
            .collect();
        Some(
            Banner::info(
                "gh-missing-scopes",
                format!(
                    "Your sign-in predates what Sawit asks GitHub for now: {} ({}). Sign in again to allow it.",
                    missing.join(", "),
                    what.join("; ")
                ),
            )
            .action(
                Button::new("gh-sign-in-again")
                    .xsmall()
                    .label("Sign In Again")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.account.update(cx, |a, cx| a.sign_in(cx))
                    })),
            ),
        )
    }
}

/// The GitHub mark, a title and one line under it, for the sign-in card.
fn card_heading(title: &'static str, subtitle: &'static str, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_3()
        .child(
            div()
                .flex_none()
                .size(px(40.))
                .rounded_full()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(Lucide::Github).size(px(20.))),
        )
        // Wraps within the card rather than running past its edge.
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(div().text_base().font_semibold().child(title))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
}

/// A numbered step of the sign-in.
fn step(n: u8, text: String, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_2p5()
        .text_sm()
        .child(
            div()
                .flex_none()
                .size(px(20.))
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .flex()
                .items_center()
                .justify_center()
                .child(n.to_string()),
        )
        .child(text)
}
