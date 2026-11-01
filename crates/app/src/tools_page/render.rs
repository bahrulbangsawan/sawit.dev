//! The Dev Tools page's parts around its rows: the header with Install Missing and Upgrade
//! All, the login shell's problem, the toolbar, the selection bar, the queue's progress, and the
//! list card with its column titles and pager.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::dev_tools::{Category, TOOLS, Tool, find};

use super::{Filter, Job, Sort, ToolsPage, category_name, logo};
use crate::catalog;

impl ToolsPage {
    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let missing = self.missing();
        let upgradable = self.upgradable();
        h_flex()
            .gap_4()
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_xl().font_semibold().child("Dev Tools"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(
                                "Official installers, run one at a time in your login shell, prerequisites first. \
                                 Open a new terminal tab afterwards to pick up the new PATH.",
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(
                        Button::new("tools-reload")
                            .ghost()
                            .small()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Detect Again")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
                    .when(!upgradable.is_empty(), |el| {
                        el.child(
                            Button::new("tools-upgrade-all")
                                .outline()
                                .small()
                                .icon(Lucide::CircleArrowUp)
                                .label(format!("Upgrade All ({})", upgradable.len()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.upgrade(&upgradable, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("tools-install-all")
                            .primary()
                            .small()
                            .icon(Lucide::Download)
                            .label(format!("Install Missing ({})", missing.len()))
                            // Unknown which are missing: it could run installers over copies
                            // that are there.
                            .disabled(missing.is_empty() || self.loading || self.problem.is_some())
                            .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&missing, cx))),
                    ),
            )
    }

    /// Says when the login shell couldn't tell which tools are installed,
    /// instead of a page of "Not installed".
    pub(super) fn render_problem(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let problem = self.problem.as_ref().filter(|_| !self.loading)?;
        let warning = cx.theme().warning;
        Some(
            h_flex()
                .px_4()
                .py_2()
                .gap_2()
                .items_start()
                .rounded(cx.theme().radius_lg)
                .border_1()
                .border_color(warning.opacity(0.35))
                .bg(warning.opacity(0.08))
                .text_sm()
                .child(
                    div()
                        .pt(px(2.))
                        .child(Icon::new(Lucide::TriangleAlert).small().text_color(warning)),
                )
                .child(div().flex_1().min_w_0().whitespace_normal().child(format!(
                    "Couldn't ask your login shell which tools are installed ({problem}). Only tools in \
                     their usual folders show as installed, so Install Missing is off until Detect Again \
                     works."
                )))
                .into_any_element(),
        )
    }

    /// Search, the filter tabs with their counts, category and sort.
    pub(super) fn render_toolbar(
        &self,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let scope: Vec<&Tool> = TOOLS.iter().filter(|t| self.in_scope(t, query)).collect();
        let choices = Filter::ALL
            .iter()
            .map(|f| {
                (
                    f.label(),
                    scope.iter().filter(|t| self.passes(t, *f)).count(),
                )
            })
            .collect();
        let current = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut categories = vec!["All Categories"];
        categories.extend(Category::ALL.iter().map(|c| category_name(*c)));
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
                "tools-filter",
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
                        "tools-category",
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
                        "tools-sort",
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

    /// "3 selected" with Clear and Install, while anything is ticked.
    pub(super) fn render_selection(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let selected: Vec<&'static str> = TOOLS
            .iter()
            .filter(|t| self.selected.contains(t.id))
            .map(|t| t.id)
            .collect();
        if selected.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let extra = self
            .plan_for(&selected)
            .len()
            .saturating_sub(selected.len());
        Some(
            h_flex()
                .px_4()
                .py_2()
                .gap_2()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.primary.opacity(0.35))
                .bg(theme.primary.opacity(0.08))
                .text_sm()
                .child(
                    Icon::new(Lucide::SquareCheck)
                        .small()
                        .text_color(theme.primary),
                )
                .child(
                    div()
                        .font_medium()
                        .child(format!("{} selected", selected.len())),
                )
                .when(extra > 0, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("+ {} needed first", extra)),
                    )
                })
                .child(div().flex_1())
                .child(
                    Button::new("tools-clear-selection")
                        .ghost()
                        .small()
                        .icon(Lucide::X)
                        .label("Clear")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected.clear();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("tools-install-selected")
                        .primary()
                        .small()
                        .icon(Lucide::Download)
                        .label(format!("Install {}", selected.len() + extra))
                        .on_click(cx.listener(move |this, _, _, cx| this.enqueue(&selected, cx))),
                )
                .into_any_element(),
        )
    }

    /// "Installing Node.js · 2 of 5" and a bar, while the queue runs.
    pub(super) fn render_progress(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (done, total) = self.batch;
        let running = self
            .running
            .as_ref()
            .and_then(|(id, job)| Some((find(id)?, job)));
        if running.is_none() && self.queue.is_empty() {
            return None;
        }
        let muted = cx.theme().muted_foreground;
        let fraction = if total == 0 {
            0.
        } else {
            done as f32 / total as f32
        };
        Some(
            v_flex()
                .gap_2()
                .px_4()
                .py_3()
                .rounded(cx.theme().radius_lg)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(Spinner::new().xsmall().color(muted))
                        .when_some(running, |el, (t, _)| el.child(logo(t.id, 16., cx)))
                        .child(div().flex_1().font_medium().child(match running {
                            Some((t, Job::Install)) => format!(
                                "{} {} · {} of {total}",
                                if self.upgrades.contains(t.id) {
                                    "Upgrading"
                                } else {
                                    "Installing"
                                },
                                t.name,
                                done + 1
                            ),
                            Some((t, Job::Uninstall(_))) => {
                                format!("Uninstalling {} · {} of {total}", t.name, done + 1)
                            }
                            None => format!("{done} of {total} done"),
                        }))
                        .when(!self.queue.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{} queued", self.queue.len())),
                            )
                            .child(
                                Button::new("tools-stop")
                                    .outline()
                                    .xsmall()
                                    .icon(Lucide::CircleStop)
                                    .label("Stop Queue")
                                    .tooltip("Cancel What's Waiting; the Current Install Finishes")
                                    .on_click(cx.listener(|this, _, _, cx| this.stop_queue(cx))),
                            )
                        }),
                )
                .child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().border)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(cx.theme().primary)
                                .w(relative(fraction.max(0.02))),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The list card: column titles, a page of rows and the pager, or the
    /// empty state.
    pub(super) fn render_list(
        &self,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let rows = self.shown(query);
        let total = rows.len();
        let selectable = self.selectable(&rows);
        let all = !selectable.is_empty() && selectable.iter().all(|id| self.selected.contains(id));
        let pick_all = Checkbox::new("tools-pick-all")
            .checked(all)
            .disabled(selectable.is_empty())
            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                for id in &selectable {
                    if *checked {
                        this.selected.insert(*id);
                    } else {
                        this.selected.remove(*id);
                    }
                }
                cx.notify();
            }));
        let selected = self.cursor.and_then(|c| rows.get(c)).map(|(_, t)| t.id);
        let (rows, paging) = catalog::page_of(rows, self.paging);
        let card = v_flex()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        if total == 0 {
            let title = match self.query(cx) {
                q if q.is_empty() => "No tools here".to_string(),
                q => format!("No tools match “{q}”"),
            };
            return card.child(catalog::empty(
                title,
                Button::new("tools-clear-filters")
                    .outline()
                    .small()
                    .icon(Lucide::FunnelX)
                    .label("Clear Filters")
                    .on_click(cx.listener(|this, _, window, cx| this.clear_filters(window, cx))),
                cx,
            ));
        }
        card.child(catalog::header(
            Some(pick_all.into_any_element()),
            "Tool",
            cx,
        ))
        .children(
            rows.into_iter()
                .map(|(ix, t)| self.render_row(ix, t, selected == Some(t.id), cx)),
        )
        .child(catalog::pager(
            "tools",
            paging,
            total,
            if total == 1 { "tool" } else { "tools" },
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
    }
}
