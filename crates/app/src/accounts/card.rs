//! The sign-in card on the Accounts page: Sign In… for a signed-out account, then Claude Code's
//! links, the code field, the time left, Cancel and Start Again.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::{Account, tracking};

use super::login::Phase;
use super::page::AccountsPage;
use super::ui;
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
            .id("account-sign-in-card")
            .gap_3()
            .px_4()
            .py_3()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar);
        let title = |icon: Lucide, text: &'static str, cx: &App| {
            h_flex()
                .gap_2()
                .child(
                    Icon::new(icon)
                        .small()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(div().text_sm().font_semibold().child(text))
        };
        let Some((phase, manual, callback, left)) = login else {
            return card.child(
                h_flex()
                    .gap_3()
                    .child(ui::icon_tile(Lucide::LogIn, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child("This account is signed out"),
                            )
                            .child(
                                div().text_xs().text_color(muted).child(
                                    "Sign in with your browser to use it and read its limits.",
                                ),
                            ),
                    )
                    .child(
                        Button::new("account-sign-in")
                            .small()
                            .primary()
                            .icon(Lucide::LogIn)
                            .label("Sign In…")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.update(cx, |m, cx| m.start_login(&id, cx))
                            })),
                    ),
            );
        };
        let again = id.clone();
        let restart = cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.model.update(cx, |m, cx| m.start_login(&again, cx))
        });
        match phase {
            Phase::Done => card.child(steps(2, cx)).child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(cx.theme().success)
                    .child(Icon::new(Lucide::CircleCheck).small())
                    .child("Signed in. Checking the account…"),
            ),
            Phase::Failed(why) if !why_empty(&why) => card
                .child(title(Lucide::CircleX, "The sign-in failed", cx))
                .child(Banner::error("account-login-failed", why))
                .child(
                    h_flex().child(
                        Button::new("account-login-again")
                            .small()
                            .icon(Lucide::RotateCcw)
                            .label("Start Again")
                            .on_click(restart),
                    ),
                ),
            Phase::Expired | Phase::Failed(_) => card
                .child(title(Lucide::Clock, "The sign-in expired", cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("Sign-in links last a few minutes. Start again for a new one."),
                )
                .child(
                    h_flex().child(
                        Button::new("account-login-again")
                            .small()
                            .icon(Lucide::RotateCcw)
                            .label("Start Again")
                            .on_click(restart),
                    ),
                ),
            phase => {
                let url = callback.clone().or(manual.clone());
                let step = if matches!(phase, Phase::Submitted) {
                    1
                } else {
                    0
                };
                card.child(
                    h_flex()
                        .child(title(Lucide::LogIn, "Sign in", cx))
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("account-login-left")
                                .role(Role::Label)
                                .aria_label(format!("Expires in {} minutes", left.div_ceil(60)))
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Expires in {}:{:02}", left / 60, left % 60)),
                        ),
                )
                .child(steps(step, cx))
                .child(div().text_xs().text_color(muted).whitespace_normal().child(
                    "Approve in your browser and this finishes by itself. Or copy the link, open it anywhere, and paste back the code it shows.",
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(Button::new("account-login-open").small().primary().icon(Lucide::Globe).label("Open in Browser").disabled(url.is_none()).on_click(move |_, _, cx| {
                            if let Some(u) = &url {
                                cx.open_url(u);
                            }
                        }))
                        .child(Button::new("account-login-copy").small().outline().icon(Lucide::Copy).label("Copy Link").disabled(manual.is_none()).on_click(move |_, _, cx| {
                            if let Some(u) = &manual {
                                cx.write_to_clipboard(ClipboardItem::new_string(u.clone()));
                            }
                        })),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().font_medium().child("Code from the sign-in page"))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().flex_1().child(Input::new(&self.code).small().aria_label("Code from the Sign-In Page")))
                                .child(Button::new("account-login-submit").small().label("Complete").on_click(cx.listener(|this, _, window, cx| {
                                    let code = this.code.read(cx).value().to_string();
                                    this.code.update(cx, |s, cx| s.set_value("", window, cx));
                                    this.model.update(cx, |m, cx| m.submit_code(&code, cx));
                                }))),
                        ),
                )
                .child(
                    h_flex().child(
                        Button::new("account-login-cancel")
                            .xsmall()
                            .ghost()
                            .label("Cancel Sign-In")
                            .on_click(cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.cancel_login(cx)))),
                    ),
                )
            }
        }
    }
}

/// The sign-in's steps, with `current` (0–2) under way: a check on each step done, the step's
/// number on the others, and its name beside it, so the state is never colour alone.
fn steps(current: usize, cx: &App) -> impl IntoElement {
    const NAMES: [&str; 3] = ["Open the browser", "Paste the code", "Done"];
    let theme = cx.theme();
    h_flex()
        .id("account-login-steps")
        .role(Role::Label)
        .aria_label(format!("Step {} of 3: {}", current + 1, NAMES[current]))
        .gap_2()
        .children(NAMES.iter().enumerate().map(|(ix, name)| {
            let done = ix < current || current == 2;
            let now = ix == current && current < 2;
            h_flex()
                .gap_1p5()
                .when(ix > 0, |el| {
                    el.child(div().w(px(16.)).h(px(1.)).bg(theme.border))
                })
                .child(
                    div()
                        .size(px(18.))
                        .flex_none()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .font_semibold()
                        .map(|el| {
                            if done {
                                el.bg(theme.success.opacity(0.18))
                                    .text_color(theme.success)
                                    .child(Icon::new(Lucide::Check).xsmall())
                            } else if now {
                                el.bg(theme.primary)
                                    .text_color(theme.primary_foreground)
                                    .child((ix + 1).to_string())
                            } else {
                                el.bg(theme.accent)
                                    .text_color(theme.muted_foreground)
                                    .child((ix + 1).to_string())
                            }
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if now {
                            theme.foreground
                        } else {
                            theme.muted_foreground
                        })
                        .when(now, |el| el.font_medium())
                        .child(*name),
                )
        }))
}

fn why_empty(s: &str) -> bool {
    s.trim().is_empty()
}
