//! One row of the Ports sidebar: a command that listens, with its ports (open in Sawit's browser),
//! its Local Explorer, its tool, folder and processes, a checkbox and Stop.

use std::collections::HashMap;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::cloudflare::explorer::Explorer;
use sawit_core::ports::PortJob;

use super::{PortsEvent, PortsPanel};

impl PortsPanel {
    pub(super) fn render_job(
        &self,
        j: &PortJob,
        sawit: &HashMap<u16, String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let root = j.root;
        let muted = cx.theme().muted_foreground;
        let checked = self.selected.contains(&root);
        let owner = j
            .ports
            .iter()
            .find_map(|(port, _)| sawit.get(port).cloned());
        let explorer_url = self
            .explorers
            .iter()
            .find(|e| e.pid == root)
            .map(Explorer::url);
        let tag = |text: String, color: Hsla| {
            div()
                .flex_none()
                .px_1()
                .rounded(px(3.))
                .text_xs()
                .text_color(color)
                .bg(color.opacity(0.12))
                .child(text)
        };
        let ports = h_flex()
            .flex_wrap()
            .gap_1()
            .children(j.ports.iter().map(|&(port, addr)| {
                div()
                    .id(("port-open", port as usize))
                    .px_1()
                    .rounded(px(3.))
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .bg(cx.theme().secondary)
                    .cursor_pointer()
                    .hover(|el| el.bg(cx.theme().accent))
                    .child(format!(":{port}"))
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(format!(
                            "Listening on {addr}:{port}. Click to open in Sawit's browser; ⌘-click for your default browser."
                        ))
                        .build(window, cx)
                    })
                    .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                        let url = format!("http://localhost:{port}");
                        if event.modifiers().platform {
                            cx.open_url(&url);
                        } else {
                            cx.emit(PortsEvent::OpenInBrowser(url));
                        }
                    }))
            }))
            .when_some(explorer_url, |el, url| {
                el.child(explorer_button(root, url, cx))
            });
        // "my-app · node server.js · 612"
        let folder = j
            .cwd
            .as_deref()
            .map(|c| c.rsplit('/').next().unwrap_or(c).to_string());
        let listener = j
            .listeners
            .iter()
            .find(|(pid, _)| *pid != root)
            .map(|(pid, cmd)| format!("{cmd} · {pid}"))
            .unwrap_or_else(|| root.to_string());
        let detail = folder
            .iter()
            .cloned()
            .chain(std::iter::once(listener))
            .collect::<Vec<_>>()
            .join(" · ");
        let full: SharedString = format!(
            "{}{}\n{} process{} (pids {})",
            j.cmdline,
            j.cwd
                .as_deref()
                .map(|c| format!("\nin {c}"))
                .unwrap_or_default(),
            j.pids.len(),
            if j.pids.len() == 1 { "" } else { "es" },
            j.pids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into();
        let docker = j.is_docker();
        let processes = j.pids.len();
        h_flex()
            .id(("port-job", root as usize))
            .w_full()
            .px_2()
            .py_1()
            .gap_1p5()
            .items_start()
            .text_sm()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .hover(|el| el.bg(cx.theme().list_hover))
            .child(
                Button::new(("port-check", root as usize))
                    .ghost()
                    .xsmall()
                    .icon(if checked {
                        Lucide::SquareCheck
                    } else {
                        Lucide::Square
                    })
                    .tooltip("Select")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.selected.remove(&root) {
                            this.selected.insert(root);
                        }
                        cx.notify();
                    })),
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
                                    .id(("port-label", root as usize))
                                    .font_semibold()
                                    .truncate()
                                    .child(j.label.clone())
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(full.clone())
                                            .build(window, cx)
                                    }),
                            )
                            .child(tag(j.tool.clone(), cx.theme().primary))
                            .when_some(owner, |el, name| {
                                el.child(tag(format!("Sawit · {name}"), cx.theme().success))
                            })
                            .when(docker, |el| {
                                el.child(tag("Docker".into(), cx.theme().warning))
                            }),
                    )
                    .child(ports)
                    .child(div().text_xs().text_color(muted).truncate().child(detail)),
            )
            .child(
                Button::new(("port-stop", root as usize))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::X)
                    .tooltip(if docker {
                        "Stop Docker Desktop".to_string()
                    } else if processes > 1 {
                        format!("Stop (SIGTERM to all {processes} processes)")
                    } else {
                        "Stop (SIGTERM)".to_string()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.stop_jobs(&[root], cx))),
            )
            .into_any_element()
    }
}

/// A dev server's "Local Explorer" action: Cloudflare's logo and its name, beside the ports.
/// A click opens it in Sawit's browser, a ⌘-click in the default browser.
fn explorer_button(root: u32, url: String, cx: &mut Context<PortsPanel>) -> impl IntoElement {
    let tip = format!(
        "Open Local Explorer ({url}) in Sawit's browser: browse and edit this dev server's KV, R2, D1, Durable Objects and Workflows. ⌘-click opens it in your default browser."
    );
    div()
        .id(("port-explorer", root as usize))
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded(px(3.))
        .text_xs()
        .bg(cx.theme().secondary)
        .cursor_pointer()
        .hover(|el| el.bg(cx.theme().accent))
        .child(crate::local_explorer::logo(12.))
        .child("Local Explorer")
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
        .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
            if event.modifiers().platform {
                cx.open_url(&url);
            } else {
                cx.emit(PortsEvent::OpenInBrowser(url.clone()));
            }
        }))
}
