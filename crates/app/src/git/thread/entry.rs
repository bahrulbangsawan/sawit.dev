//! One entry of the timeline: who opened the thread, a push, a comment, a review with its inline
//! comments, or an action (merged, closed, labeled…), as an icon, an avatar, who, what and when.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{ActionKind, Entry, Event, ReviewState, ago, now};

use super::ThreadView;
use super::markdown::{card, inline_comment};

impl ThreadView {
    pub(super) fn render_opened(&self, cx: &mut Context<Self>) -> AnyElement {
        let item = &self.item;
        let (icon, what) = if item.pull {
            (Lucide::GitPullRequest, "opened this pull request")
        } else {
            (Lucide::CircleDot, "opened this issue")
        };
        let body = (!item.body.trim().is_empty()).then(|| {
            card(format!("gh-opened-{}", item.number).into(), &item.body, cx).into_any_element()
        });
        event_row(
            (icon, cx.theme().success),
            &item.author,
            what,
            item.created_at,
            body,
            cx,
        )
    }

    pub(super) fn render_entry(
        &self,
        ix: usize,
        entry: &Entry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        match entry {
            Entry::Pushed {
                author,
                time,
                commits,
            } => {
                let what = match commits.len() {
                    1 => "pushed 1 commit".to_string(),
                    n => format!("pushed {n} commits"),
                };
                let list = v_flex()
                    .gap_0p5()
                    .children(commits.iter().map(|(sha, message)| {
                        h_flex()
                            .gap_3()
                            .text_sm()
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(muted)
                                    .font_family(mono.clone())
                                    .child(sha.clone()),
                            )
                            .child(div().min_w_0().truncate().child(message.clone()))
                    }))
                    .into_any_element();
                event_row(
                    (Lucide::GitCommitHorizontal, muted),
                    author,
                    &what,
                    *time,
                    Some(list),
                    cx,
                )
            }
            Entry::Event(Event::Commented { author, time, body }) => {
                let body = card(
                    format!("gh-comment-{}-{ix}", self.item.number).into(),
                    body,
                    cx,
                )
                .into_any_element();
                event_row(
                    (Lucide::MessageSquare, muted),
                    author,
                    "commented",
                    *time,
                    Some(body),
                    cx,
                )
            }
            Entry::Event(Event::Reviewed {
                author,
                time,
                state,
                body,
                comments,
                ..
            }) => {
                let (icon, color, what) = match state {
                    ReviewState::Approved => (
                        Lucide::CircleCheck,
                        cx.theme().success,
                        "approved these changes",
                    ),
                    ReviewState::ChangesRequested => {
                        (Lucide::CircleAlert, cx.theme().danger, "requested changes")
                    }
                    ReviewState::Dismissed => {
                        (Lucide::CircleX, muted, "left a review that was dismissed")
                    }
                    ReviewState::Commented => (Lucide::Eye, muted, "reviewed"),
                };
                let content = v_flex()
                    .gap_2()
                    .when(!body.trim().is_empty(), |el| {
                        el.child(card(
                            format!("gh-review-{}-{ix}", self.item.number).into(),
                            body,
                            cx,
                        ))
                    })
                    .children(comments.iter().enumerate().map(|(c, comment)| {
                        inline_comment(
                            format!("gh-inline-{}-{ix}-{c}", self.item.number).into(),
                            comment,
                            cx,
                        )
                    }))
                    .into_any_element();
                let has_content = !body.trim().is_empty() || !comments.is_empty();
                event_row(
                    (icon, color),
                    author,
                    what,
                    *time,
                    has_content.then_some(content),
                    cx,
                )
            }
            Entry::Event(Event::Action {
                actor,
                time,
                kind,
                detail,
            }) => {
                let (icon, color, what) = match kind {
                    ActionKind::Merged => (
                        Lucide::GitMerge,
                        cx.theme().magenta,
                        format!("merged commit {detail}"),
                    ),
                    ActionKind::Closed => {
                        (Lucide::CircleX, cx.theme().danger, "closed this".into())
                    }
                    ActionKind::Reopened => (
                        Lucide::CircleDot,
                        cx.theme().success,
                        "reopened this".into(),
                    ),
                    ActionKind::Renamed => (
                        Lucide::Pencil,
                        muted,
                        format!("changed the title to “{detail}”"),
                    ),
                    ActionKind::Labeled => {
                        (Lucide::Tag, muted, format!("added the label {detail}"))
                    }
                    ActionKind::Unlabeled => {
                        (Lucide::Tag, muted, format!("removed the label {detail}"))
                    }
                    ActionKind::ReviewRequested => (
                        Lucide::Eye,
                        muted,
                        format!("requested a review from {detail}"),
                    ),
                    ActionKind::Assigned => (Lucide::UserPlus, muted, format!("assigned {detail}")),
                    ActionKind::ForcePushed => (
                        Lucide::GitCommitHorizontal,
                        cx.theme().warning,
                        "force-pushed the branch".into(),
                    ),
                    ActionKind::BranchDeleted => {
                        (Lucide::GitBranchMinus, muted, "deleted the branch".into())
                    }
                    ActionKind::ReadyForReview => {
                        (Lucide::Eye, muted, "marked this ready for review".into())
                    }
                    ActionKind::MarkedDraft => {
                        (Lucide::Pencil, muted, "marked this as a draft".into())
                    }
                    ActionKind::Referenced => {
                        (Lucide::Link, muted, format!("mentioned this in {detail}"))
                    }
                };
                event_row((icon, color), actor, &what, *time, None, cx)
            }
            // Folded into `Pushed` by `group`.
            Entry::Event(Event::Committed { .. }) => div().into_any_element(),
        }
    }
}

/// A timeline row: icon in a circle, avatar, who, what, when, and content.
fn event_row(
    (icon, color): (Lucide, Hsla),
    who: &str,
    what: &str,
    time: i64,
    content: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .flex_none()
                .size(px(28.))
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).small().text_color(color)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .child(
                    h_flex()
                        .h(px(28.))
                        .gap_2()
                        .text_sm()
                        .child(avatar(who, cx))
                        .child(div().font_semibold().child(who.to_string()))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(muted)
                                .child(what.to_string()),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(ago(time, now())),
                        ),
                )
                .children(content),
        )
        .into_any_element()
}

/// A round initial in a color picked from the name.
fn avatar(name: &str, cx: &App) -> impl IntoElement {
    let palette = [
        cx.theme().blue,
        cx.theme().green,
        cx.theme().yellow,
        cx.theme().magenta,
        cx.theme().cyan,
        cx.theme().red,
    ];
    let hash = name
        .bytes()
        .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    div()
        .flex_none()
        .size(px(18.))
        .rounded_full()
        .bg(palette[hash % palette.len()])
        .text_color(cx.theme().background)
        .text_xs()
        .font_semibold()
        .flex()
        .items_center()
        .justify_center()
        .child(initial)
}
