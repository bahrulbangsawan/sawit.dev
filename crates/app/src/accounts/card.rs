//! The sign-in card on the Accounts page: Sign In… for a signed-out account, then Claude Code's
//! links, the code field, the time left, Cancel and Start Again.

use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::agent_accounts::{Account, tracking};

use super::login::Phase;
use super::page::AccountsPage;
use crate::states::Banner;

impl AccountsPage {
    /// Shows the `statusLine` before and after, then turns tracking on.
    pub(super) fn confirm_tracking(
        &mut self,
        a: Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.model.read(cx).root.clone();
        let (before, after) = match tracking::preview(&root, &a) {
            Ok(p) => p,
            Err(e) => {
                self.model.update(cx, |m, cx| {
                    m.error = Some(e.to_string());
                    cx.notify()
                });
                return;
            }
        };
        let show = |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default();
        let detail = format!(
            "Before:\n{}\n\nAfter:\n{}\n\nClaude Code runs Sawit on every status line update. It reads only the usage windows and keeps them on this Mac; your own status line keeps showing. settings.json is backed up first.",
            before.as_ref().map(show).unwrap_or_else(|| "(none)".into()),
            show(&after)
        );
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Track usage of “{}”?", a.name),
            Some(&detail),
            &["Turn On Tracking", "Cancel"],
            cx,
        );
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                model.update(cx, |m, cx| m.set_tracking(&a.id, true, cx));
            }
        })
        .detach();
    }

    pub(super) fn render_login(
        &self,
        a: &Account,
        login: Option<(Phase, Option<String>, Option<String>, u64)>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = a.id.clone();
        let muted = cx.theme().muted_foreground;
        let card = v_flex()
            .gap_2()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border);
        let Some((phase, manual, callback, left)) = login else {
            return card
                .child(div().text_sm().child("This account is signed out."))
                .child(
                    Button::new("account-sign-in")
                        .small()
                        .primary()
                        .label("Sign In…")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.update(cx, |m, cx| m.start_login(&id, cx))
                        })),
                );
        };
        let again = id.clone();
        match phase {
            Phase::Done => card.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().success)
                    .child("Signed in. Checking the account…"),
            ),
            Phase::Failed(why) if !why_empty(&why) => card
                .child(Banner::error("account-login-failed", why))
                .child(
                    Button::new("account-login-again")
                        .small()
                        .label("Start Again")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.update(cx, |m, cx| m.start_login(&again, cx))
                        })),
                ),
            Phase::Expired | Phase::Failed(_) => card
                .child(div().text_sm().child("The sign-in expired."))
                .child(
                    Button::new("account-login-again")
                        .small()
                        .label("Start Again")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.update(cx, |m, cx| m.start_login(&again, cx))
                        })),
                ),
            _ => {
                let url = callback.clone().or(manual.clone());
                card.child(div().text_sm().child("Sign in with your browser: approve there and this finishes by itself, or copy the link, open it anywhere and paste back the code it shows."))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(Button::new("account-login-open").small().primary().label("Open in Browser").disabled(url.is_none()).on_click(move |_, _, cx| {
                                if let Some(u) = &url {
                                    cx.open_url(u);
                                }
                            }))
                            .child(Button::new("account-login-copy").small().outline().label("Copy Link").disabled(manual.is_none()).on_click(move |_, _, cx| {
                                if let Some(u) = &manual {
                                    cx.write_to_clipboard(ClipboardItem::new_string(u.clone()));
                                }
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(div().flex_1().child(Input::new(&self.code).small()))
                            .child(Button::new("account-login-submit").small().label("Complete").on_click(cx.listener(|this, _, window, cx| {
                                let code = this.code.read(cx).value().to_string();
                                this.code.update(cx, |s, cx| s.set_value("", window, cx));
                                this.model.update(cx, |m, cx| m.submit_code(&code, cx));
                            }))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .text_color(muted)
                            .child(div().flex_1().child(format!("Expires in {}:{:02}", left / 60, left % 60)))
                            .child(Button::new("account-login-cancel").xsmall().ghost().label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.cancel_login(cx))),
                            )),
                    )
            }
        }
    }
}

fn why_empty(s: &str) -> bool {
    s.trim().is_empty()
}
