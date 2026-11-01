//! The tab as drawn: the toolbar (the database's kind and write route, export, query and
//! paging), the filter, picks and update bars, a notice, and the grid or a placeholder.

use std::sync::atomic::Ordering;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::table::DataTable;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db;

use super::{PAGE_SIZE_LIST, PAGE_SIZES, TableView, plural};
use crate::catalog::{self, PageSize};
use crate::db::{DbEvent, logos, panel};
use crate::states::{Banner, Placeholder};

impl Render for TableView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pager = self.pager.clone();
        let busy = self.loading;
        let page_label = match pager.last_page() {
            Some(last) => format!("Page {} / {}", pager.page + 1, last + 1),
            None => format!("Page {}", pager.page + 1),
        };
        let nav = |id: &'static str, icon: Lucide, tooltip: &'static str, enabled: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tooltip)
                .disabled(!enabled || busy)
        };
        let toolbar = h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .border_b_1()
            .border_color(cx.theme().border)
            .child({
                let store = self.db.read(cx);
                let kind = self.kind(cx);
                let name: SharedString = store.accessible_name(&self.profile).into();
                div()
                    .id("table-kind")
                    .child(logos::logo(kind, 14.))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(name.clone()).build(window, cx)
                    })
            })
            .child(
                div()
                    .text_color(cx.theme().foreground)
                    .font_semibold()
                    .child(self.title.clone()),
            )
            .children(panel::route_badge(
                "table-route",
                self.db.read(cx).route(&self.profile),
                cx,
            ))
            .when(self.key(cx).is_none(), |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .px_1p5()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().secondary)
                        .child(Icon::new(Lucide::Lock).xsmall())
                        .child("read-only"),
                )
            })
            .child(div().flex_1())
            .map(|el| match &self.exporting {
                Some(e) => el
                    .child(format!(
                        "Exporting {}… {}",
                        e.format.label(),
                        plural(e.rows.load(Ordering::Relaxed), "row")
                    ))
                    .child(
                        Button::new("table-export-cancel")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .tooltip("Stop the Export")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(e) = &this.exporting {
                                    e.stop.store(true, Ordering::Relaxed);
                                }
                                cx.notify();
                            })),
                    ),
                None => el.child(self.export_menu(cx)),
            })
            .child(
                Button::new("page-query")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::SquareTerminal)
                    .label("Query")
                    .tooltip("Open in a SQL Editor")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let sql = format!(
                            "SELECT *\nFROM {}\nLIMIT 100;",
                            db::relation_sql_for(this.kind(cx), &this.schema, &this.table)
                        );
                        cx.emit(DbEvent::OpenSql {
                            profile: this.profile.clone(),
                            sql: Some(sql),
                        });
                    })),
            )
            .when(busy && self.cancel.is_some(), |el| {
                el.child(
                    Button::new("page-cancel")
                        .ghost()
                        .xsmall()
                        .label("Cancel")
                        .tooltip("Cancel the Query on the Server")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                )
            })
            .child(catalog::page_size_menu(
                PAGE_SIZE_LIST,
                PageSize::Rows(pager.size as usize),
                &PAGE_SIZES,
                Anchor::TopRight,
                cx.listener(|this, size: &PageSize, window, cx| {
                    if let PageSize::Rows(n) = *size {
                        this.set_page_size(n as u64, window, cx);
                    }
                }),
            ))
            .child(if self.waiting_reconnect {
                "Reconnecting…".to_string()
            } else if busy {
                "Loading…".to_string()
            } else if self.error.is_some() {
                String::new()
            } else {
                pager.label()
            })
            .child(
                nav(
                    "page-first",
                    Lucide::ChevronsLeft,
                    "First Page",
                    pager.has_prev(),
                )
                .on_click(cx.listener(|this, _, window, cx| this.go(0, window, cx))),
            )
            .child(
                nav(
                    "page-prev",
                    Lucide::ChevronLeft,
                    "Previous Page",
                    pager.has_prev(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let p = this.pager.page.saturating_sub(1);
                    this.go(p, window, cx)
                })),
            )
            .child(div().min_w(px(64.)).text_center().child(page_label))
            .child(
                nav(
                    "page-next",
                    Lucide::ChevronRight,
                    "Next Page",
                    pager.has_next(),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    let p = this.pager.page + 1;
                    this.go(p, window, cx)
                })),
            )
            .child(
                nav(
                    "page-last",
                    Lucide::ChevronsRight,
                    "Last Page",
                    pager.last_page().is_some_and(|l| pager.page < l),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(last) = this.pager.last_page() {
                        this.go(last, window, cx)
                    }
                })),
            )
            .child(
                Button::new("page-reload")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Reload")
                    .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
            );
        let narrowed = self.query.is_filtered();
        let customized = narrowed || self.query.sort.is_some();
        let reset = |id: &'static str, cx: &mut Context<Self>| {
            Button::new(id)
                .small()
                .icon(Lucide::FunnelX)
                .label("Reset Filters and Sort")
                .on_click(cx.listener(|this, _, window, cx| this.reset_query(window, cx)))
        };
        let body = if let Some(e) = self.error.clone() {
            Placeholder::error("Unable to load rows")
                .detail(if customized {
                    format!("{e}\nCheck the filter values and sort column, or reset them.")
                } else {
                    e
                })
                .when(customized, |p| {
                    p.action(reset("table-error-reset", cx).primary())
                })
                .action(
                    Button::new("table-retry")
                        .small()
                        .icon(Lucide::RefreshCw)
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                )
                .into_any_element()
        } else if !busy && pager.page_rows == 0 && self.grid.is_some() {
            if narrowed {
                Placeholder::new(IconName::Search, "No matching rows")
                    .detail("No row matches the search and filters.")
                    .action(reset("table-empty-reset", cx))
                    .into_any_element()
            } else if pager.page == 0 {
                Placeholder::new(Lucide::Table, "This table is empty")
                    .detail("Rows inserted into it will show up here when you reload.")
                    .into_any_element()
            } else {
                Placeholder::new(Lucide::Table, "No rows on this page")
                    .detail("Rows may have been deleted since the last page loaded.")
                    .into_any_element()
            }
        } else if let Some(grid) = &self.grid {
            DataTable::new(grid).stripe(true).small().into_any_element()
        } else {
            Placeholder::loading("Loading rows…").into_any_element()
        };
        let filter_bar = self.render_filter_bar(cx);
        let filter_panel = self.show_filters.then(|| self.render_filters(cx));
        let picks_bar = (!self.selection.is_empty()).then(|| self.render_picks_bar(cx));
        let update_form = self.updating.is_some().then(|| self.render_update_form(cx));
        let notice = self.notice.clone().map(|(failed, message)| {
            let banner = if failed {
                Banner::error("table-notice", message)
            } else {
                Banner::info("table-notice", message)
            };
            banner.on_dismiss(cx.listener(|this, _, _, cx| {
                this.notice = None;
                cx.notify();
            }))
        });
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(toolbar)
            .child(filter_bar)
            .children(filter_panel)
            .children(picks_bar)
            .children(update_form)
            .children(notice)
            .child(div().flex_1().min_h_0().child(body))
    }
}
