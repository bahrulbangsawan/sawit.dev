//! Writing a new issue, or a pull request from the current branch, in the GitHub tab.

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github;

use super::{GitHubView, Load};
use crate::rt;
use crate::states::Banner;

impl GitHubView {
    pub(super) fn compose(&mut self, pull: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.composing = Some(pull);
        self.thread = None;
        self.error = None;
        let title = if pull {
            self.store
                .read(cx)
                .status
                .as_ref()
                .and_then(|s| s.branch.clone())
                .map(|b| b.replace(['-', '_', '/'], " "))
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.new_title.update(cx, |s, cx| {
            s.set_value(title, window, cx);
            s.focus(window, cx);
        });
        self.new_body
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(token), Some((owner, name)), Some(pull)) =
            (self.token(cx), self.slug(cx), self.composing)
        else {
            return;
        };
        let title = self.new_title.read(cx).value().trim().to_string();
        let body = self.new_body.read(cx).value().to_string();
        let head = self
            .store
            .read(cx)
            .status
            .as_ref()
            .and_then(|s| s.branch.clone());
        if title.is_empty() {
            return;
        }
        if pull && head.is_none() {
            self.error = Some(
                "Switch to a branch first: a pull request is opened from your current branch."
                    .into(),
            );
            cx.notify();
            return;
        }
        self.working = true;
        self.error = None;
        cx.notify();
        let used = token.clone();
        let rx = rt::spawn(cx, async move {
            if pull {
                let head = head.unwrap_or_default();
                let base = github::default_branch(&token, &owner, &name).await?;
                if head == base {
                    return Err(sawit_core::CoreError::GitHub(format!(
                        "You're on {base}, the default branch. Create a branch with your changes, push it, then open the pull request."
                    )));
                }
                github::create_pull(&token, &owner, &name, &head, &base, &title, &body).await
            } else {
                github::create_issue(&token, &owner, &name, &title, &body).await
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = false;
                match result {
                    Ok(item) => {
                        let number = item.number;
                        if let Load::Ready(items) = &mut this.items
                            && this.show_open
                        {
                            items.insert(0, item);
                        }
                        this.composing = None;
                        this.open_thread(number, window, cx);
                    }
                    Err(e) if Self::refused(&e, &used, cx) => {}
                    Err(e) => {
                        let mut message = e.to_string();
                        if pull && message.contains("422") {
                            message.push_str(" Push your branch first, and check that a pull request for it isn't already open.");
                        }
                        this.error = Some(message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_compose(&self, pull: bool, cx: &mut Context<Self>) -> AnyElement {
        let branch = self
            .store
            .read(cx)
            .status
            .as_ref()
            .and_then(|s| s.branch.clone());
        let ready = !self.new_title.read(cx).value().trim().is_empty() && !self.working;
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(div().text_lg().font_semibold().child(if pull { "New Pull Request" } else { "New Issue" }))
            .when(pull, |el| {
                el.child(div().text_sm().text_color(cx.theme().muted_foreground).child(match branch {
                    Some(b) => format!("From {b} into the default branch. Push {b} first so GitHub has your commits."),
                    None => "Switch to a branch first.".into(),
                }))
            })
            .child(Input::new(&self.new_title))
            .child(Textarea::new(&self.new_body).h(px(200.)).aria_label("Description"))
            .when_some(self.error.clone(), |el, e| el.child(Banner::error("gh-compose-error", e)))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("gh-compose-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.composing = None;
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("gh-compose-create")
                            .primary()
                            .small()
                            .label(if pull { "Create Pull Request" } else { "Create Issue" })
                            .loading(self.working)
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    ),
            )
            .into_any_element()
    }
}
