//! The Timeline tab: who opened the thread and everything since, filtered to comments, reviews or
//! commits, on a rail that joins the entries' icons, with more loaded on request.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{self, Entry, Event};

use super::{Load, ThreadView};
use crate::states::Placeholder;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Filter {
    All,
    Comments,
    Reviews,
    Commits,
}

impl Filter {
    fn keeps(self, entry: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Comments => matches!(entry, Entry::Event(Event::Commented { .. })),
            Filter::Reviews => matches!(entry, Entry::Event(Event::Reviewed { .. })),
            Filter::Commits => matches!(entry, Entry::Pushed { .. }),
        }
    }
}

impl ThreadView {
    /// Groups `events` into the timeline rows again: call it whenever they change.
    pub(super) fn regroup(&mut self) {
        self.entries = match &self.events {
            Load::Ready(events) => github::group(events),
            _ => Vec::new(),
        };
    }

    pub(super) fn render_timeline(&self, cx: &mut Context<Self>) -> AnyElement {
        let events = match &self.events {
            Load::Loading => {
                return Placeholder::loading("Loading the timeline…").into_any_element();
            }
            Load::Failed(e) => {
                return Placeholder::error("Unable to load the timeline")
                    .detail(e.clone())
                    .action(
                        Button::new("gh-timeline-retry")
                            .small()
                            .label("Try Again")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .into_any_element();
            }
            Load::Ready(_) => &self.entries,
        };
        let count = |f: Filter| match f {
            // Commits, not pushes.
            Filter::Commits => events
                .iter()
                .map(|e| match e {
                    Entry::Pushed { commits, .. } => commits.len(),
                    _ => 0,
                })
                .sum(),
            _ => events.iter().filter(|e| f.keeps(e)).count() + usize::from(f == Filter::All),
        };
        let filter = self.filter;
        let chip = |id: &'static str,
                    label: &'static str,
                    which: Filter,
                    n: usize,
                    cx: &mut Context<Self>| {
            let active = filter == which;
            h_flex()
                .id(id)
                .px_2()
                .py_0p5()
                .gap_1p5()
                .rounded(cx.theme().radius)
                .text_sm()
                .cursor_pointer()
                .when(active, |el| {
                    el.bg(cx.theme().secondary)
                        .text_color(cx.theme().foreground)
                })
                .when(!active, |el| {
                    el.text_color(cx.theme().muted_foreground)
                        .hover(|el| el.text_color(cx.theme().foreground))
                })
                .child(label)
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(n.to_string()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filter = which;
                    cx.notify();
                }))
        };
        let mut chips = h_flex()
            .p_0p5()
            .gap_0p5()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(chip(
                "gh-filter-all",
                "All",
                Filter::All,
                count(Filter::All),
                cx,
            ))
            .child(chip(
                "gh-filter-comments",
                "Comments",
                Filter::Comments,
                count(Filter::Comments),
                cx,
            ));
        if self.item.pull {
            chips = chips
                .child(chip(
                    "gh-filter-reviews",
                    "Reviews",
                    Filter::Reviews,
                    count(Filter::Reviews),
                    cx,
                ))
                .child(chip(
                    "gh-filter-commits",
                    "Commits",
                    Filter::Commits,
                    count(Filter::Commits),
                    cx,
                ));
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        if filter == Filter::All {
            rows.push(self.render_opened(cx));
        }
        for (ix, entry) in events.iter().enumerate().filter(|(_, e)| filter.keeps(e)) {
            rows.push(self.render_entry(ix, entry, cx));
        }
        let last = rows.len().saturating_sub(1);
        let rows: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| rail(row, i == last, cx))
            .collect();
        div()
            .id("gh-timeline")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .child(h_flex().child(chips))
                    .when(rows.is_empty(), |el| {
                        el.child(
                            div()
                                .py_6()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Nothing of this kind yet."),
                        )
                    })
                    .child(v_flex().children(rows))
                    .when(self.more, |el| {
                        el.child(
                            h_flex().child(
                                Button::new("gh-timeline-more")
                                    .small()
                                    .label("Load More")
                                    .loading(self.loading_more)
                                    .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                            ),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// Draws the vertical line that joins the timeline's icons behind a row.
fn rail(row: AnyElement, last: bool, cx: &App) -> AnyElement {
    div()
        .relative()
        .pb_4()
        .when(!last, |el| {
            el.child(
                div()
                    .absolute()
                    .left(px(13.5))
                    .top(px(28.))
                    .bottom_0()
                    .w(px(1.))
                    .bg(cx.theme().border),
            )
        })
        .child(row)
        .into_any_element()
}
