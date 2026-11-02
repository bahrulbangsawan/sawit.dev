//! One row of the Run panel: a script or saved command, whether it runs, the ports it listens on,
//! and Start, Stop, Restart and Edit.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Item, RunEvent, RunPanel};
use crate::terminal::Origin;

impl RunPanel {
    pub(super) fn render_item(&self, ix: usize, item: Item, cx: &mut Context<Self>) -> AnyElement {
        let (open, running, pgrps) = {
            let terminal = self.terminal.read(cx);
            let status: Vec<_> = item
                .keys()
                .filter_map(|k| terminal.job_status(k, cx))
                .collect();
            let pgrps: Vec<u32> = status.iter().filter_map(|s| s.running).collect();
            (!status.is_empty(), !pgrps.is_empty(), pgrps)
        };
        let restarting = item
            .keys()
            .next()
            .is_some_and(|k| self.restarting.contains(k));
        let mut ports: Vec<u16> = self
            .ports
            .read(cx)
            .jobs()
            .iter()
            .filter(|j| pgrps.iter().any(|p| j.root == *p || j.pids.contains(p)))
            .flat_map(|j| j.ports.iter().map(|(port, _)| *port))
            .collect();
        ports.sort_unstable();
        ports.dedup();
        let muted = cx.theme().muted_foreground;
        let dot = if running {
            cx.theme().success
        } else if open {
            muted
        } else {
            muted.opacity(0.35)
        };
        let many = item.launches.len();
        let tip: SharedString = item
            .launches
            .iter()
            .map(|l| l.command.clone())
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        let first_key = item.keys().next().unwrap_or_default().to_string();
        let saved = item.saved.clone();
        let item = std::rc::Rc::new(item);
        h_flex()
            .id(("run-item", ix))
            .w_full()
            .px_2()
            .py_1()
            .gap_1p5()
            .items_start()
            .text_sm()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .hover(|el| el.bg(cx.theme().list_hover))
            .when(open, |el| el.cursor_pointer())
            .on_click(cx.listener({
                let key = first_key.clone();
                move |_, _, _, cx| {
                    if open {
                        cx.emit(RunEvent::Show(key.clone()));
                    }
                }
            }))
            .child(
                div()
                    .flex_none()
                    .mt(px(7.))
                    .size(px(7.))
                    .rounded_full()
                    .bg(dot),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .id(("run-name", ix))
                                    .font_semibold()
                                    .truncate()
                                    .child(item.name.clone())
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                            .build(window, cx)
                                    }),
                            )
                            .when(many > 1, |el| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .px_1()
                                        .rounded(px(3.))
                                        .text_xs()
                                        .text_color(muted)
                                        .bg(cx.theme().secondary)
                                        .child(format!("{many} terminals")),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(item.detail.clone()),
                    )
                    .when(!ports.is_empty(), |el| {
                        el.child(h_flex().flex_wrap().gap_1().children(ports.into_iter().map(
                            |port| {
                                div()
                                    .id(("run-port", ix * 100_000 + port as usize))
                                    .px_1()
                                    .rounded(px(3.))
                                    .text_xs()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(cx.theme().success)
                                    .bg(cx.theme().success.opacity(0.12))
                                    .cursor_pointer()
                                    .child(format!(":{port}"))
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(format!(
                                            "Open http://localhost:{port}"
                                        ))
                                        .build(window, cx)
                                    })
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        cx.open_url(&format!("http://localhost:{port}"))
                                    })
                            },
                        )))
                    }),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_0p5()
                    .when_some(saved, |el, id| {
                        el.child(
                            Button::new(("run-edit", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Pencil)
                                .tooltip("Edit Command")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.open_editor(Some(id.clone()), window, cx)
                                })),
                        )
                    })
                    .when(running, |el| {
                        el.child(
                            Button::new(("run-restart", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::RotateCcw)
                                .loading(restarting)
                                .tooltip(if restarting {
                                    "Restarting once it has stopped"
                                } else {
                                    "Restart"
                                })
                                .on_click(cx.listener({
                                    let item = item.clone();
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.restart(&item, cx)
                                    }
                                })),
                        )
                    })
                    .child(if running {
                        Button::new(("run-stop", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::CircleStop)
                            .tooltip("Stop (Ctrl-C)")
                            .on_click(cx.listener({
                                let item = item.clone();
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.stop(&item, cx)
                                }
                            }))
                    } else {
                        Button::new(("run-start", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Play)
                            .tooltip(if many > 1 {
                                format!("Run in {many} Terminals")
                            } else {
                                "Run in Terminal".to_string()
                            })
                            .on_click(cx.listener({
                                let item = item.clone();
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.start(&item, Origin::User, cx)
                                }
                            }))
                    }),
            )
            .into_any_element()
    }
}
