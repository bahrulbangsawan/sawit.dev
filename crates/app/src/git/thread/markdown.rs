//! The thread's Markdown: a description, comment or review body in a bordered card, and a review's
//! comment on a line under its file and line.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::text::TextView;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::github::ReviewComment;

/// Markdown in a bordered box.
pub(super) fn card(id: SharedString, body: &str, cx: &App) -> impl IntoElement {
    div()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .text_sm()
        .child(if body.trim().is_empty() {
            div()
                .text_color(cx.theme().muted_foreground)
                .child("No description.")
                .into_any_element()
        } else {
            markdown(id, body)
        })
}

fn markdown(id: SharedString, body: &str) -> AnyElement {
    TextView::markdown(id, body.to_string())
        .selectable(true)
        .on_link_click(|url, _, _, cx| {
            if url.starts_with("https://") {
                cx.open_url(url);
            }
        })
        .into_any_element()
}

/// A review comment on a line: the file and line, then the comment.
pub(super) fn inline_comment(id: SharedString, comment: &ReviewComment, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .bg(cx.theme().secondary)
                .border_b_1()
                .border_color(cx.theme().border)
                .text_xs()
                .child(Icon::new(Lucide::FileCode).xsmall().text_color(muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(comment.path.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(muted)
                        .child(match comment.line {
                            Some(line) => format!("line {line}"),
                            None => "outdated".into(),
                        }),
                ),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .text_sm()
                .child(markdown(id, &comment.body)),
        )
        .into_any_element()
}
