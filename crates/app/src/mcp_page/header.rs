//! The page's header: its title, Detect Again, Sign In to All, and the clients found.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_clients::{Category, Client};

use super::logos::client_logo;
use super::{McpPage, plural};
use crate::catalog::{self, Mark};

impl McpPage {
    // Rendering

    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let pending = self.pending_sign_ins(None).len();
        let checking = !self.checking.is_empty();
        let mut clients: Vec<(Client, usize)> = Vec::new();
        for t in &self.targets {
            match clients.iter_mut().find(|(c, _)| *c == t.client) {
                Some((_, n)) => *n += 1,
                None => clients.push((t.client, 1)),
            }
        }
        clients.sort_by_key(|(c, _)| {
            (
                c.category() == Category::Editor,
                Client::ALL.iter().position(|x| x == c),
            )
        });
        let chips = clients.into_iter().map(|(client, n)| {
            h_flex()
                .h(px(24.))
                .px_2()
                .gap_1p5()
                .rounded_full()
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .child(client_logo(client, 14., cx))
                .child(client.name())
                .when(n > 1, |el| {
                    el.child(div().text_color(muted).child(format!("×{n}")))
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
                            .child(div().text_xl().font_semibold().child("MCP Servers"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .whitespace_normal()
                                    .child(
                                        "Install a server into every agent, editor and profile at once. Servers with an \
                                         account sign in through the agent's own sign-in; restart the client to pick up changes.",
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_2()
                            .when(checking, |el| {
                                el.child(catalog::pill(Mark::Spinner, "Checking sign-ins…", muted))
                            })
                            .child(
                                Button::new("mcp-reload")
                                    .ghost()
                                    .small()
                                    .icon(Lucide::RefreshCw)
                                    .tooltip(if self.busy.is_empty() {
                                        "Detect Again"
                                    } else {
                                        "Detect Again When the Installs Finish"
                                    })
                                    .loading(self.loading)
                                    // A detection can reorder the clients under a running
                                    // install.
                                    .disabled(!self.busy.is_empty())
                                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                            )
                            .when(pending > 0, |el| {
                                el.child(
                                    Button::new("mcp-sign-in-all")
                                        .primary()
                                        .small()
                                        .icon(Lucide::KeyRound)
                                        .label(format!("Sign In to All ({pending})"))
                                        // Its sheet lists clients by their place in the list,
                                        // which a detection can change.
                                        .disabled(self.loading)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            let queue = this.pending_sign_ins(None);
                                            this.start_sign_in(queue, window, cx)
                                        })),
                                )
                            }),
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
                                .child(if self.targets.is_empty() {
                                    "No AI clients found on this Mac.".to_string()
                                } else {
                                    format!(
                                        "Installs into {} on this Mac:",
                                        plural(self.targets.len(), "target", "targets")
                                    )
                                }),
                        )
                        .children(chips),
                )
            })
    }
}
