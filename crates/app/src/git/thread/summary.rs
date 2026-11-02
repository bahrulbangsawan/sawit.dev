//! The Summary tab (a pull request's branches, changes and mergeability, then the description)
//! and the Changes tab (its whole diff).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::diff::Diff;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::markdown::card;
use super::{Load, ThreadView};
use crate::states::Placeholder;

impl ThreadView {
    pub(super) fn render_summary(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let facts: Vec<(&str, String)> = match &self.pull {
            Some(p) => vec![
                ("Branches", format!("{} → {}", p.head, p.base)),
                (
                    "Changes",
                    format!(
                        "{} file{}, +{} −{}",
                        p.changed_files,
                        if p.changed_files == 1 { "" } else { "s" },
                        p.additions,
                        p.deletions
                    ),
                ),
                (
                    "Mergeable",
                    match (p.merged, p.mergeable) {
                        (true, _) => "Already merged".into(),
                        (_, Some(true)) => "Yes, no conflicts".into(),
                        (_, Some(false)) => "No, it has conflicts with the base branch".into(),
                        (_, None) => "GitHub is still checking".into(),
                    },
                ),
            ],
            None => Vec::new(),
        };
        div()
            .id("gh-summary")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .when(!facts.is_empty(), |el| {
                        el.child(v_flex().gap_1().children(facts.into_iter().map(|(k, v)| {
                            h_flex()
                                .gap_3()
                                .text_sm()
                                .child(div().w(px(90.)).flex_none().text_color(muted).child(k))
                                .child(v)
                        })))
                    })
                    .child(card(
                        format!("gh-summary-body-{}", self.item.number).into(),
                        &self.item.body,
                        cx,
                    )),
            )
            .into_any_element()
    }

    pub(super) fn render_changes(&self) -> AnyElement {
        match &self.diff {
            Some(Load::Ready(Some(diff))) => Diff::new(diff).size_full().into_any_element(),
            Some(Load::Ready(None)) => Placeholder::new(Lucide::File, "No file changes")
                .detail("This pull request doesn't change any files.")
                .into_any_element(),
            Some(Load::Failed(e)) => Placeholder::error("Unable to load the changes")
                .detail(e.clone())
                .into_any_element(),
            _ => Placeholder::loading("Loading the changes…").into_any_element(),
        }
    }
}
