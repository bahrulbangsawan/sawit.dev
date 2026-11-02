//! What the open thread does on GitHub: send a comment, close or reopen it, merge a pull request.

use gpui_kit::*;
use sawit_core::github::{self, Event};

use super::{Load, Tab, ThreadEvent, ThreadView};
use crate::rt;

impl ThreadView {
    pub(super) fn send_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.comment.read(cx).value().trim().to_string();
        if body.is_empty() || self.working {
            return;
        }
        let (token, owner, name, number) = self.api();
        self.working = true;
        self.error = None;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            github::add_comment(&token, &owner, &name, number, &body).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = false;
                match result {
                    Ok(c) => {
                        if let Load::Ready(events) = &mut this.events {
                            events.push(Event::Commented {
                                author: c.author,
                                time: c.created_at,
                                body: c.body,
                            });
                        }
                        this.item.comments += 1;
                        this.comment.update(cx, |s, cx| s.set_value("", window, cx));
                        this.tab = Tab::Timeline;
                        cx.emit(ThreadEvent::Commented(number));
                    }
                    Err(e) if this.refused(&e, cx) => {}
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Closes or reopens it on GitHub.
    pub(super) fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        let (token, owner, name, number) = self.api();
        self.working = true;
        cx.notify();
        let rx = rt::spawn(cx, async move {
            github::set_open(&token, &owner, &name, number, open).await
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.working = false;
                match result {
                    Ok(()) => {
                        this.item.open = open;
                        cx.emit(ThreadEvent::StateChanged { number, open });
                        this.reload(cx);
                    }
                    Err(e) if this.refused(&e, cx) => {}
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (token, owner, name, number) = self.api();
        let (head, base) = self
            .pull
            .as_ref()
            .map(|p| (p.head.clone(), p.base.clone()))
            .unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Merge pull request #{number}?"),
            Some(&format!(
                "{head} is merged into {base} on GitHub with a merge commit."
            )),
            &["Merge Pull Request", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(rx) = this.update(cx, |this, cx| {
                this.working = true;
                this.error = None;
                cx.notify();
                rt::spawn(cx, async move {
                    github::merge_pull(&token, &owner, &name, number).await
                })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.working = false;
                match result {
                    Ok(Ok(())) => {
                        this.item.open = false;
                        cx.emit(ThreadEvent::Merged(number));
                        this.reload(cx);
                    }
                    Ok(Err(e)) if this.refused(&e, cx) => {}
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
}
