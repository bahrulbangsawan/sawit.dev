//! One changed file's diff in a center tab (staged or not), kept current as
//! the file changes, with Stage / Unstage and Open File.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::diff::{Diff, DiffFile, DiffState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git::{self, FileChange};

use super::store::GitStore;
use crate::rt;
use crate::states::Placeholder;

pub enum DiffViewEvent {
    OpenFile(std::path::PathBuf),
}

impl EventEmitter<DiffViewEvent> for DiffView {}

pub struct DiffView {
    store: Entity<GitStore>,
    pub file: FileChange,
    pub staged: bool,
    patch: Option<String>,
    diff: Option<Entity<DiffState>>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl DiffView {
    pub fn new(store: Entity<GitStore>, file: FileChange, staged: bool, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&store, |this, _, cx| this.reload(cx))];
        let mut this = Self {
            store,
            file,
            staged,
            patch: None,
            diff: None,
            error: None,
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    pub fn key(file: &FileChange, staged: bool) -> SharedString {
        format!("git-diff\u{0}{}\u{0}{staged}", file.path).into()
    }

    pub fn title(&self) -> SharedString {
        let name = self.file.path.rsplit('/').next().unwrap_or(&self.file.path);
        let side = if self.staged { "staged" } else { "changes" };
        format!("{name} ({side})").into()
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.store.read(cx).repo.clone() else {
            return;
        };
        let (file, staged) = (self.file.clone(), self.staged);
        let previous = self.patch.clone();
        // Parsing a big diff (lockfiles, generated code) takes long enough
        // to freeze the UI, so it happens next to the `git diff` call.
        let rx = rt::spawn(cx, async move {
            let patch = git::diff(&repo, &file, staged).await?;
            if previous.as_ref() == Some(&patch) {
                return Ok(None);
            }
            let parsed = tokio::task::spawn_blocking({
                let patch = patch.clone();
                move || DiffFile::parse(&patch).map_err(|e| e.message().to_string())
            })
            .await
            .unwrap_or_else(|_| Err("The diff parser stopped.".into()));
            Ok::<_, sawit_core::CoreError>(Some((patch, parsed)))
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(None) => return,
                    Ok(Some((patch, parsed))) => {
                        this.error = None;
                        this.diff = match parsed {
                            Ok(files) if !files.is_empty() => Some(cx.new(|cx| DiffState::new(files, cx))),
                            Ok(_) => None,
                            Err(e) => {
                                this.error = Some(format!("Unable to show this diff. {e}"));
                                None
                            }
                        };
                        this.patch = Some(patch);
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let busy = store.busy.is_some();
        let repo = store.repo.clone();
        let staged = self.staged;
        let deleted = self.file.worktree == 'D' || (staged && self.file.index == 'D');
        let toolbar = h_flex()
            .px_3()
            .py_1p5()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(crate::file_icons::file_icon(std::path::Path::new(&self.file.path), cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(match &self.file.from {
                        Some(from) => format!("{from} → {}", self.file.path),
                        None => self.file.path.clone(),
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if staged { "Staged" } else { "Not staged" }),
            )
            .when_some(repo.filter(|_| !deleted), |el, repo| {
                let path = repo.join(&self.file.path);
                el.child(
                    Button::new("diff-open-file")
                        .ghost()
                        .small()
                        .icon(Lucide::FileCode)
                        .label("Open File")
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(DiffViewEvent::OpenFile(path.clone())))),
                )
            })
            .child(
                Button::new("diff-stage")
                    .small()
                    .icon(if staged { Lucide::Minus } else { Lucide::Plus })
                    .label(if staged { "Unstage" } else { "Stage" })
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut paths = vec![this.file.path.clone()];
                        if staged {
                            paths.extend(this.file.from.clone());
                        }
                        this.store.update(cx, |s, cx| {
                            s.run(if staged { "Unstaging…" } else { "Staging…" }, cx, move |repo, _| async move {
                                if staged {
                                    git::unstage(&repo, &paths).await?;
                                } else {
                                    git::stage(&repo, &paths).await?;
                                }
                                Ok(String::new())
                            })
                        })
                    })),
            );
        let body: AnyElement = match (&self.error, &self.diff, &self.patch) {
            (Some(e), _, _) => Placeholder::error("Unable to show the diff").detail(e.clone()).into_any_element(),
            (_, Some(diff), _) => Diff::new(diff).size_full().into_any_element(),
            (_, None, None) => Placeholder::loading("Reading the diff…").into_any_element(),
            (_, None, Some(_)) => Placeholder::new(Lucide::Check, "No differences")
                .detail(if staged {
                    "Nothing in this file is staged now."
                } else {
                    "This file has no unstaged changes now."
                })
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
    }
}
