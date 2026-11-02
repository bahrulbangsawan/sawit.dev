//! The Ports sidebar as drawn: Dev or All, select all, refresh, the filter, stopping what is
//! selected (or force-stopping what didn't exit), and the list.

use std::collections::HashMap;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::PortsPanel;
use crate::states::{Banner, Placeholder};

impl Render for PortsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.spinner_shown = self.loading;
        let visible = self.visible(cx);
        let sawit: HashMap<u16, String> = self
            .servers
            .read(cx)
            .registry
            .instances
            .iter()
            .map(|i| (i.port, i.name.clone()))
            .collect();
        // "Select all" only picks dev servers, never Docker Desktop.
        let selectable: Vec<u32> = visible
            .iter()
            .filter(|j| j.dev && !j.is_docker())
            .map(|j| j.root)
            .collect();
        let all_selected =
            !selectable.is_empty() && selectable.iter().all(|p| self.selected.contains(p));
        let selected = self.selected.len();
        let hidden = self.jobs.iter().filter(|j| !j.dev).count();
        let rows: Vec<AnyElement> = visible
            .iter()
            .map(|j| self.render_job(j, &sawit, cx))
            .collect();
        let muted = cx.theme().muted_foreground;
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child("LISTENING PORTS"),
                    )
                    .child(
                        Button::new("ports-dev")
                            .ghost()
                            .xsmall()
                            .label("Dev")
                            .selected(!self.show_all)
                            .tooltip("Dev Servers, Databases and Docker")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_all = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ports-all")
                            .ghost()
                            .xsmall()
                            .label("All")
                            .selected(self.show_all)
                            .tooltip(format!(
                                "Also Other Apps That Listen ({hidden} Hidden)"
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_all = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ports-select-all")
                            .ghost()
                            .xsmall()
                            .icon(if all_selected {
                                Lucide::SquareCheck
                            } else {
                                Lucide::Square
                            })
                            .tooltip("Select All Dev Servers (Except Docker)")
                            .disabled(selectable.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if all_selected {
                                    this.selected.clear();
                                } else {
                                    this.selected.extend(selectable.iter().copied());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ports-refresh")
                            .ghost()
                            .xsmall()
                            .icon(IconName::RotateCw)
                            .tooltip("Refresh")
                            .loading(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.filter).small().cleanable(true))
                    .when(selected > 0, |el| {
                        el.child(
                            Button::new("ports-stop-selected")
                                .danger()
                                .xsmall()
                                .icon(Lucide::X)
                                .label(format!("Stop {selected} Selected"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let roots: Vec<u32> = this.selected.iter().copied().collect();
                                    this.stop_jobs(&roots, cx)
                                })),
                        )
                    })
                    .when(!self.stubborn.is_empty(), |el| {
                        el.child(
                            Button::new("ports-force")
                                .danger()
                                .outline()
                                .xsmall()
                                .icon(Lucide::Zap)
                                .label(format!("Force Stop {} (SIGKILL)", self.stubborn.len()))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let pids = std::mem::take(&mut this.stubborn);
                                    let n = pids.len();
                                    let what = format!(
                                        "{n} {}",
                                        if n == 1 { "process" } else { "processes" }
                                    );
                                    this.signal(pids, true, what, cx)
                                })),
                        )
                    }),
            )
            .when_some(self.notice.clone(), |el, n| {
                el.child(Banner::info("ports-notice", n).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    },
                )))
            })
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("ports-error", e))
            })
            .child(
                div()
                    .id("ports-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(!self.loaded && self.error.is_none(), |el| {
                        el.child(Placeholder::loading("Checking open ports…"))
                    })
                    .when(self.loaded && visible.is_empty(), |el| {
                        let filtering = !self.filter.read(cx).value().trim().is_empty();
                        el.child(if filtering {
                            Placeholder::new(IconName::Search, "No matching processes")
                                .detail("Nothing matches this port, command or folder.")
                                .action(
                                    Button::new("clear-ports-filter")
                                        .small()
                                        .label("Clear Filter")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.filter
                                                .update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                )
                        } else if !self.show_all {
                            Placeholder::new(Lucide::Radio, "No dev servers running")
                                .detail("Run bun run dev, npm run dev, pnpm dev or any other server and it shows up here.")
                        } else {
                            Placeholder::new(Lucide::Radio, "No open ports")
                                .detail("None of your processes are listening on a TCP port. Start a server and it shows up here.")
                        })
                    }),
            )
    }
}
