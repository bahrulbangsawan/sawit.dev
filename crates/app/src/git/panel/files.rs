//! The Git sidebar's changed files: merge conflicts, staged and unstaged changes under their
//! headings, each file staged, unstaged or discarded from its row, or opened as a diff.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::git::{self, FileChange};

use super::{GitPanel, GitPanelEvent};

const ROW_HEIGHT: f32 = 26.;

#[derive(Clone)]
pub(super) enum Row {
    Header(Section, usize),
    File(Section, FileChange),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Conflicts,
    Staged,
    Changes,
}

impl GitPanel {
    pub(super) fn build_rows(&self, cx: &App) -> Vec<Row> {
        let Some(status) = &self.store.read(cx).status else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (section, files) in [
            (
                Section::Conflicts,
                status.conflicted().cloned().collect::<Vec<_>>(),
            ),
            (Section::Staged, status.staged().cloned().collect()),
            (Section::Changes, status.unstaged().cloned().collect()),
        ] {
            if files.is_empty() {
                continue;
            }
            rows.push(Row::Header(section, files.len()));
            rows.extend(files.into_iter().map(|f| Row::File(section, f)));
        }
        rows
    }

    fn stage(&mut self, file: &FileChange, cx: &mut Context<Self>) {
        let path = file.path.clone();
        self.store.update(cx, |s, cx| {
            s.run("Staging…", cx, move |repo, _| async move {
                git::stage(&repo, &[path]).await.map(|_| String::new())
            })
        });
    }

    fn unstage(&mut self, file: &FileChange, cx: &mut Context<Self>) {
        let mut paths = vec![file.path.clone()];
        paths.extend(file.from.clone());
        self.store.update(cx, |s, cx| {
            s.run("Unstaging…", cx, move |repo, _| async move {
                git::unstage(&repo, &paths).await.map(|_| String::new())
            })
        });
    }

    fn discard(&mut self, file: FileChange, window: &mut Window, cx: &mut Context<Self>) {
        let (title, button) = if file.untracked() {
            (format!("Delete “{}”?", file.path), "Delete File")
        } else {
            (
                format!("Discard changes to “{}”?", file.path),
                "Discard Changes",
            )
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &title,
            Some("This can't be undone."),
            &[button, "Cancel"],
            cx,
        );
        let store = self.store.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                store.update(cx, |s, cx| {
                    s.run("Discarding…", cx, move |repo, _| async move {
                        git::discard(&repo, &file).await.map(|_| String::new())
                    })
                });
            }
        })
        .detach();
    }

    pub(super) fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let busy = self.store.read(cx).busy.is_some();
        match row {
            Row::Header(section, count) => {
                let (label, action): (&str, Option<(&str, &str, bool)>) = match section {
                    Section::Conflicts => ("MERGE CONFLICTS", None),
                    Section::Staged => ("STAGED", Some(("Unstage All", "unstage-all", false))),
                    Section::Changes => ("CHANGES", Some(("Stage All", "stage-all", true))),
                };
                h_flex()
                    .id(("git-header", ix))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child(div().flex_1().child(format!("{label} · {count}")))
                    .when_some(action, |el, (tooltip, id, stage)| {
                        el.child(
                            Button::new(id)
                                .ghost()
                                .xsmall()
                                .disabled(busy)
                                .icon(if stage { Lucide::Plus } else { Lucide::Minus })
                                .tooltip(tooltip)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.store.update(cx, |s, cx| {
                                        s.run(
                                            if stage { "Staging…" } else { "Unstaging…" },
                                            cx,
                                            move |repo, _| async move {
                                                if stage {
                                                    git::stage_all(&repo).await?;
                                                } else {
                                                    git::unstage_all(&repo).await?;
                                                }
                                                Ok(String::new())
                                            },
                                        )
                                    })
                                })),
                        )
                    })
                    .into_any_element()
            }
            Row::File(section, file) => {
                let section = *section;
                let staged = section == Section::Staged;
                let letter = if file.conflicted {
                    '!'
                } else if staged {
                    file.index
                } else if file.untracked() {
                    'U'
                } else {
                    file.worktree
                };
                let color = match letter {
                    'A' | 'U' => cx.theme().success,
                    'D' | '!' => cx.theme().danger,
                    'R' | 'C' => cx.theme().info,
                    _ => cx.theme().warning,
                };
                let what = match letter {
                    'A' => "Added",
                    'U' => "Untracked",
                    'D' => "Deleted",
                    'R' => "Renamed",
                    'C' => "Copied",
                    '!' => "Conflicted",
                    _ => "Modified",
                };
                let (name, folder) = match file.path.rsplit_once('/') {
                    Some((dir, name)) => (name.to_string(), dir.to_string()),
                    None => (file.path.clone(), String::new()),
                };
                let (open, act, discard) = (file.clone(), file.clone(), file.clone());
                let group: SharedString = format!("git-row-{ix}").into();
                h_flex()
                    .id(("git-file", ix))
                    .group(group.clone())
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .pl_3()
                    .pr_1()
                    .gap_1p5()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .tooltip({
                        let path = file.path.clone();
                        move |window, cx| Tooltip::new(format!("{path} · {what}")).build(window, cx)
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(GitPanelEvent::Diff {
                            file: open.clone(),
                            staged,
                        })
                    }))
                    .child(crate::file_icons::file_icon(
                        std::path::Path::new(&file.path),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().flex_none().max_w_full().truncate().child(name))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(folder),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_0p5()
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .when(section == Section::Changes, |el| {
                                el.child(
                                    Button::new(("git-discard", ix))
                                        .ghost()
                                        .xsmall()
                                        .disabled(busy)
                                        .icon(Lucide::Undo2)
                                        .tooltip(if discard.untracked() {
                                            "Delete File"
                                        } else {
                                            "Discard Changes"
                                        })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.discard(discard.clone(), window, cx);
                                        })),
                                )
                            })
                            .child(
                                Button::new(("git-stage", ix))
                                    .ghost()
                                    .xsmall()
                                    .disabled(busy)
                                    .icon(if staged { Lucide::Minus } else { Lucide::Plus })
                                    .tooltip(if staged {
                                        "Unstage"
                                    } else if section == Section::Conflicts {
                                        "Mark as Resolved"
                                    } else {
                                        "Stage"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if staged {
                                            this.unstage(&act, cx);
                                        } else {
                                            this.stage(&act, cx);
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .text_xs()
                            .font_semibold()
                            .text_color(color)
                            .child(letter.to_string()),
                    )
                    .into_any_element()
            }
        }
    }
}
