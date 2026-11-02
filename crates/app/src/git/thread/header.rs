//! The thread's header: its title, state, author and branches, its actions (refresh, open on
//! GitHub, close or reopen, merge) and the tabs.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{ago, now};

use super::{Load, Tab, ThreadView};

impl ThreadView {
    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let item = &self.item;
        let muted = cx.theme().muted_foreground;
        let merged = self.pull.as_ref().is_some_and(|p| p.merged);
        let (state_label, state_color) = match (item.open, merged) {
            (true, _) => ("Open", cx.theme().success),
            (false, true) => ("Merged", cx.theme().magenta),
            (false, false) => ("Closed", cx.theme().danger),
        };
        let can_merge = item.pull
            && item.open
            && self
                .pull
                .as_ref()
                .is_some_and(|p| p.mergeable != Some(false));
        let url = item.url.clone();
        let open = item.open;
        let pull = item.pull;
        v_flex()
            .px_4()
            .pt_2()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_semibold()
                            .child(item.title.clone()),
                    )
                    .child(
                        Button::new("gh-reload")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Refresh")
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .child(
                        Button::new("gh-open-web")
                            .ghost()
                            .small()
                            .icon(Lucide::ExternalLink)
                            .tooltip("Open on GitHub")
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .when(!merged, |el| {
                        el.child(
                            Button::new("gh-toggle-open")
                                .small()
                                .label(match (open, pull) {
                                    (true, true) => "Close Pull Request",
                                    (true, false) => "Close Issue",
                                    _ => "Reopen",
                                })
                                .disabled(self.working)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.set_open(!open, cx)),
                                ),
                        )
                    })
                    .when(can_merge, |el| {
                        el.child(
                            Button::new("gh-merge")
                                .primary()
                                .small()
                                .icon(Lucide::GitMerge)
                                .label("Merge")
                                .disabled(self.working)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.merge(window, cx)),
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(
                        div()
                            .px_1p5()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(state_color)
                            .text_color(state_color)
                            .child(state_label),
                    )
                    .child(format!(
                        "#{} · {} opened {}",
                        item.number,
                        item.author,
                        ago(item.created_at, now())
                    ))
                    .when_some(self.pull.clone(), |el, p| {
                        el.child(
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(format!("{} → {}", p.head, p.base)),
                        )
                    }),
            )
            .child(self.render_tabs(cx))
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let entries = match &self.events {
            Load::Ready(_) => Some(self.entries.len() + 1),
            _ => None,
        };
        let tab = |id: &'static str, which: Tab, current: Tab, cx: &App| {
            let active = which == current;
            h_flex()
                .id(id)
                .h(px(34.))
                .px_2()
                .gap_1p5()
                .text_sm()
                .cursor_pointer()
                .border_b_2()
                .border_color(if active {
                    cx.theme().primary
                } else {
                    transparent_black()
                })
                .text_color(if active {
                    cx.theme().foreground
                } else {
                    cx.theme().muted_foreground
                })
                .hover(|el| el.text_color(cx.theme().foreground))
        };
        let current = self.tab;
        let count = |n: String, cx: &App| div().text_color(cx.theme().muted_foreground).child(n);
        h_flex()
            .mt_1()
            .gap_2()
            .child(
                tab("gh-tab-summary", Tab::Summary, current, cx)
                    .child("Summary")
                    .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Summary, cx))),
            )
            .when(self.item.pull, |el| {
                let stats = self.pull.as_ref().map(|p| (p.additions, p.deletions));
                el.child(
                    tab("gh-tab-changes", Tab::Changes, current, cx)
                        .child("Changes")
                        .when_some(stats, |el, (add, del)| {
                            el.child(
                                div()
                                    .text_color(cx.theme().success)
                                    .child(format!("+{add}")),
                            )
                            .child(div().text_color(cx.theme().danger).child(format!("−{del}")))
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Changes, cx))),
                )
            })
            .child(
                tab("gh-tab-timeline", Tab::Timeline, current, cx)
                    .child("Timeline")
                    .when_some(entries, |el, n| el.child(count(n.to_string(), cx)))
                    .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Timeline, cx))),
            )
    }
}
