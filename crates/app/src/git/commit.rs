//! One commit in its own center tab: full message, author, parents, and
//! every file it changed.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::diff::{Diff, DiffFile, DiffState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git::{self, Commit, CommitDetail};
use sawit_core::github::{ago, now};

use super::store::GitStore;
use crate::rt;
use crate::states::Placeholder;

pub struct CommitView {
    pub commit: Commit,
    detail: Option<Result<CommitDetail, String>>,
    diff: Option<Entity<DiffState>>,
    copied: bool,
}

impl CommitView {
    pub fn new(store: Entity<GitStore>, commit: Commit, cx: &mut Context<Self>) -> Self {
        if let Some(repo) = store.read(cx).repo.clone() {
            let hash = commit.hash.clone();
            let rx = rt::spawn(cx, async move { git::show(&repo, &hash).await });
            cx.spawn(async move |this, cx| {
                let Ok(result) = rx.await else {
                    return;
                };
                let _ = this.update(cx, |this, cx| {
                    this.diff = result
                        .as_ref()
                        .ok()
                        .and_then(|d| DiffFile::parse(&d.patch).ok())
                        .filter(|files| !files.is_empty())
                        .map(|files| cx.new(|cx| DiffState::new(files, cx)));
                    this.detail = Some(result.map_err(|e| e.to_string()));
                    cx.notify();
                });
            })
            .detach();
        }
        Self {
            commit,
            detail: None,
            diff: None,
            copied: false,
        }
    }

    pub fn key(hash: &str) -> SharedString {
        format!("git-commit\u{0}{hash}").into()
    }

    /// Short hash and subject, for the tab.
    pub fn title(&self) -> SharedString {
        let mut subject: String = self.commit.subject.chars().take(32).collect();
        if self.commit.subject.chars().count() > 32 {
            subject.push('…');
        }
        format!("{} {subject}", self.commit.short()).into()
    }

    fn copy_hash(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.commit.hash.clone()));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for CommitView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let commit = &self.commit;
        let muted = cx.theme().muted_foreground;
        let detail = match &self.detail {
            None => Ok(None),
            Some(Ok(d)) => Ok(Some(d)),
            Some(Err(e)) => Err(e.clone()),
        };
        let body = detail
            .as_ref()
            .ok()
            .copied()
            .flatten()
            .and_then(|d| {
                // The subject is the heading; show only what follows it.
                let rest = d.body.strip_prefix(commit.subject.as_str()).unwrap_or(&d.body).trim();
                (!rest.is_empty()).then(|| rest.to_string())
            });
        let committed_by = detail
            .as_ref()
            .ok()
            .copied()
            .flatten()
            .filter(|d| d.committer != commit.author)
            .map(|d| format!(" · committed by {}", d.committer));
        let parents = commit
            .parents
            .iter()
            .map(|p| p[..p.len().min(7)].to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let header = v_flex()
            .px_4()
            .py_3()
            .gap_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().text_lg().font_semibold().child(commit.subject.clone()))
                    .child(
                        Button::new("commit-copy-hash")
                            .ghost()
                            .small()
                            .icon(if self.copied { Lucide::Check } else { Lucide::Copy })
                            .label(commit.short().to_string())
                            .tooltip(if self.copied { "Copied" } else { "Copy Full Hash" })
                            .on_click(cx.listener(|this, _, _, cx| this.copy_hash(cx))),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .text_xs()
                    .text_color(muted)
                    .child(format!(
                        "{} <{}> · {}{}",
                        commit.author,
                        commit.email,
                        ago(commit.time, now()),
                        committed_by.unwrap_or_default()
                    ))
                    .when(!parents.is_empty(), |el| {
                        el.child(format!(
                            "{} {parents}",
                            if commit.parents.len() > 1 { "parents" } else { "parent" }
                        ))
                    }),
            )
            .when_some(body, |el, body| {
                el.child(
                    div()
                        .id("commit-body")
                        .mt_1()
                        .max_h(px(220.))
                        .overflow_y_scroll()
                        .text_sm()
                        .whitespace_normal()
                        .child(body),
                )
            });
        let changes: AnyElement = match (&detail, &self.diff) {
            (Err(e), _) => Placeholder::error("Unable to read this commit").detail(e.clone()).into_any_element(),
            (Ok(None), _) => Placeholder::loading("Reading the commit…").into_any_element(),
            (Ok(Some(_)), Some(diff)) => Diff::new(diff).size_full().into_any_element(),
            (Ok(Some(_)), None) => Placeholder::new(Lucide::File, "No file changes")
                .detail("This commit doesn't change any files.")
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(header)
            .child(div().flex_1().min_h_0().child(changes))
    }
}
