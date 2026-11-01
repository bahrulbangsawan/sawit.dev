//! The Skills page's parts around its rows: the header with the agents found, the toolbar, and
//! the list card with its column titles and pager.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::skills::{Category, PACKS, Pack};

use super::{Filter, SkillsPage, Sort, plural};
use crate::catalog::{self, tilde};

impl SkillsPage {
    // Rendering

    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let chips = self.report.targets.iter().map(|t| {
            h_flex()
                .id(SharedString::from(format!(
                    "skills-agent-{}",
                    t.dir.display()
                )))
                .h(px(24.))
                .px_2()
                .gap_1p5()
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .when_some(t.clients.first(), |el, client| {
                    el.child(crate::mcp_page::client_logo(*client, 14., cx))
                })
                .child(t.label.clone())
                .tooltip({
                    let dir = tilde(&t.dir);
                    move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(dir.clone()).build(window, cx)
                    }
                })
        });
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xl().font_semibold().child("Skills"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .whitespace_normal()
                                    .child(
                                        "Agent skills from their official sources, installed into the agents on this Mac \
                                         the way each source documents. See the commands before they run; start a new \
                                         session of the agent to pick a skill up.",
                                    ),
                            ),
                    )
                    .child(
                        h_flex().flex_none().gap_2().child(
                            Button::new("skills-reload")
                                .ghost()
                                .small()
                                .icon(Lucide::RefreshCw)
                                .accessibility_label("Detect Again")
                                .tooltip(if self.running.is_none() {
                                    "Detect Again"
                                } else {
                                    "Detect Again When the Installs Finish"
                                })
                                .loading(self.loading)
                                // A detection can reorder the agents under a running script.
                                .disabled(self.running.is_some())
                                .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                        ),
                    ),
            )
            .when(!self.loading, |el| {
                el.child(
                    h_flex()
                        .gap_1p5()
                        .flex_wrap()
                        .child(
                            div()
                                .pr_1()
                                .text_xs()
                                .font_medium()
                                .text_color(muted)
                                .child(if self.report.targets.is_empty() {
                                    "No agents with skills found on this Mac.".to_string()
                                } else {
                                    format!(
                                        "Installs into {} on this Mac:",
                                        plural(self.report.targets.len(), "skills folder", "skills folders")
                                    )
                                }),
                        )
                        .children(chips),
                )
            })
    }

    /// Search, the filter tabs with their counts, category and sort.
    pub(super) fn render_toolbar(
        &self,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let scope: Vec<&Pack> = PACKS.iter().filter(|p| self.in_scope(p, query)).collect();
        let choices = Filter::ALL
            .iter()
            .map(|f| {
                (
                    f.label(),
                    scope.iter().filter(|p| self.passes(p, *f)).count(),
                )
            })
            .collect();
        let current = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut categories = vec!["All Categories"];
        categories.extend(Category::ALL.iter().map(|c| c.name()));
        let category = self
            .category
            .and_then(|c| Category::ALL.iter().position(|x| *x == c))
            .map_or(0, |ix| ix + 1);
        let sort = Sort::ALL.iter().position(|s| *s == self.sort).unwrap_or(0);
        h_flex()
            .gap_2()
            .flex_wrap()
            .when_some(self.search.as_ref(), |el, search| {
                el.child(catalog::search(search, cx))
            })
            .child(catalog::tabs(
                "skills-filter",
                choices,
                current,
                cx.listener(|this, ix: &usize, _, cx| {
                    this.filter = Filter::ALL[*ix];
                    this.paging.page = 0;
                    this.cursor = None;
                    cx.notify();
                }),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .ml_auto()
                    .child(catalog::menu(
                        "skills-category",
                        Lucide::Funnel,
                        categories[category].to_string(),
                        categories,
                        category,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.category = ix.checked_sub(1).map(|ix| Category::ALL[ix]);
                            this.paging.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    ))
                    .child(catalog::menu(
                        "skills-sort",
                        Lucide::ArrowUpDown,
                        format!("Sort: {}", self.sort.label()),
                        Sort::ALL.iter().map(|s| s.label()).collect(),
                        sort,
                        cx.listener(|this, ix: &usize, _, cx| {
                            this.sort = Sort::ALL[*ix];
                            this.paging.page = 0;
                            this.cursor = None;
                            cx.notify();
                        }),
                    )),
            )
    }

    /// The list card: column titles, a page of packs and the pager, or the
    /// empty state.
    pub(super) fn render_list(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rows = self.shown(query);
        let total = rows.len();
        let card = v_flex()
            .debug_selector(|| "skills-list".into())
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        if total == 0 {
            let title = if query.is_empty() {
                "No skills here".to_string()
            } else {
                format!("No skills match “{query}”")
            };
            return card
                .child(catalog::empty(
                    title,
                    Button::new("skills-clear-filters")
                        .outline()
                        .small()
                        .icon(Lucide::FunnelX)
                        .label("Clear Filters")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.clear_filters(window, cx)),
                        ),
                    cx,
                ))
                .into_any_element();
        }
        let selected = self.cursor.and_then(|c| rows.get(c)).map(|p| p.id);
        let rows: Vec<_> = rows.into_iter().enumerate().collect();
        let (rows, paging) = catalog::page_of(rows, self.paging);
        card.child(catalog::header(None, "Skill", cx))
            .children(
                rows.into_iter()
                    .map(|(ix, p)| self.render_row(ix, p, selected == Some(p.id), cx)),
            )
            .child(catalog::pager(
                "skills",
                paging,
                total,
                if total == 1 { "pack" } else { "packs" },
                self.cursor,
                cx.listener(|this, paging: &catalog::Paging, _, cx| {
                    // A turn of the page leaves the selected row behind; a new size keeps it shown.
                    if paging.size == this.paging.size {
                        this.cursor = None;
                    }
                    this.paging = *paging;
                    cx.notify();
                }),
                cx,
            ))
            .into_any_element()
    }
}
