//! A Your Repositories row: the repository's tile, its name with its language, stars, fork and
//! archive tags over its description, its visibility over its last push, and Clone.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{Repo, ago, now};

use crate::catalog::{self, Mark};
use crate::git::github::GitHubView;

/// How wide a description's tooltip grows before its lines wrap.
const TOOLTIP_WIDTH: f32 = 360.;

/// A repository's tile: archived, else a fork, else private, else public.
fn repo_icon(repo: &Repo) -> Lucide {
    if repo.archived {
        Lucide::Archive
    } else if repo.fork {
        Lucide::GitFork
    } else if repo.private {
        Lucide::Lock
    } else {
        Lucide::BookMarked
    }
}

/// "Pushed 1 day ago", or "Never pushed" when GitHub has no push for it.
fn pushed(repo: &Repo) -> String {
    if repo.pushed_at > 0 {
        format!("Pushed {}", ago(repo.pushed_at, now()))
    } else {
        "Never pushed".into()
    }
}

impl GitHubView {
    /// The row of `repo`, `ix` its place in the whole list. The description wraps to two lines at
    /// most; its tooltip holds all of it.
    pub(super) fn render_repo_row(
        &self,
        ix: usize,
        repo: &Repo,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let cloning = self.cloning.as_deref() == Some(repo.full_name.as_str());
        let another_cloning = self.cloning.is_some() && !cloning;

        let name = v_flex()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    // A name too long for the column wraps rather than running under Status.
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .font_medium()
                            .whitespace_normal()
                            .child(repo.full_name.clone()),
                    )
                    .when_some(repo.language.clone(), |el, language| {
                        el.child(catalog::tag(None, language, cx))
                    })
                    .when(repo.stars > 0, |el| {
                        el.child(catalog::tag(Some(Lucide::Star), repo.stars.to_string(), cx))
                    })
                    .when(repo.fork, |el| el.child(catalog::tag(None, "Fork", cx)))
                    .when(repo.archived, |el| {
                        el.child(catalog::tag(None, "Archived", cx))
                    }),
            )
            .when_some(repo.description.clone(), |el, about| {
                let about = SharedString::from(about);
                el.child(
                    div()
                        .id(("gh-repo-about", ix))
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .line_clamp(2)
                        .text_ellipsis()
                        .tooltip({
                            let about = about.clone();
                            move |window, cx| {
                                let about = about.clone();
                                Tooltip::element(move |_, _| {
                                    div()
                                        .max_w(px(TOOLTIP_WIDTH))
                                        .whitespace_normal()
                                        .child(about.clone())
                                })
                                .build(window, cx)
                            }
                        })
                        .child(about),
                )
            });

        let (icon, visibility) = if repo.private {
            (Lucide::Lock, "Private")
        } else {
            (Lucide::Globe, "Public")
        };
        let status = v_flex()
            .gap_1()
            .items_start()
            .child(catalog::pill(Mark::Icon(icon), visibility, muted))
            .child(div().text_xs().text_color(muted).child(pushed(repo)));

        let clone = Button::new(("gh-clone", ix))
            .outline()
            .small()
            .icon(Lucide::Download)
            .label(if cloning { "Cloning…" } else { "Clone…" })
            // It asks for a folder first, which the ellipsis says, and names the repository for
            // a screen reader, which hears the buttons without their rows.
            .accessibility_label(format!("Clone {}…", repo.full_name))
            .tooltip("Clone into a Folder and Open It")
            .loading(cloning)
            .disabled(another_cloning)
            .on_click(cx.listener({
                let repo = repo.clone();
                move |this, _, window, cx| this.clone_repo(repo.clone(), window, cx)
            }));

        let tile = Icon::new(repo_icon(repo))
            .size(px(18.))
            .text_color(muted)
            .into_any_element();
        catalog::list_item(
            catalog::columns(None, catalog::tile(tile, cx), name, status, clone),
            ("gh-repo", ix),
            repo.full_name.clone(),
        )
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .hover(|el| el.bg(theme.list_hover))
        .into_any_element()
    }
}
