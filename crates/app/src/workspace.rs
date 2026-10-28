//! The main window, laid out like Zed: title bar, left panel, center pane,
//! bottom dock and status bar.

use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::servers::{ServerDetail, ServerList, ServerStore};

pub struct Workspace {
    servers: Entity<ServerStore>,
    server_list: Entity<ServerList>,
    server_detail: Entity<ServerDetail>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let servers = cx.new(ServerStore::new);
        cx.observe(&servers, |_, _, cx| cx.notify()).detach();
        Self {
            server_list: cx.new(|cx| ServerList::new(servers.clone(), cx)),
            server_detail: cx.new(|cx| ServerDetail::new(servers.clone(), window, cx)),
            servers,
        }
    }

    fn render_status_bar(&self, cx: &App) -> impl IntoElement {
        let store = self.servers.read(cx);
        let running = store.running_count();
        let total = store.registry.instances.len();
        h_flex()
            .h(px(24.))
            .px_2()
            .gap_3()
            .justify_between()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .bg(cx.theme().status_bar)
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .child(
                h_flex()
                    .gap_1()
                    .child(div().size(px(6.)).rounded_full().bg(if running > 0 {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    }))
                    .child(format!("{running}/{total} instance berjalan")),
            )
            .child("PG Desk")
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .size_full()
                        .gap_2()
                        .text_sm()
                        .child(div().font_semibold().child("PG Desk"))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("PostgreSQL lokal tanpa Docker"),
                        ),
                ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("workspace-h")
                        .child(
                            resizable_panel()
                                .size(px(240.))
                                .size_range(px(160.)..px(480.))
                                .child(
                                    div()
                                        .size_full()
                                        .border_r_1()
                                        .border_color(cx.theme().border)
                                        .child(self.server_list.clone()),
                                ),
                        )
                        .child(
                            resizable_panel().child(
                                v_resizable("workspace-v")
                                    .child(resizable_panel().child(self.server_detail.clone()))
                                    .child(
                                        resizable_panel()
                                            .size(px(200.))
                                            .size_range(px(80.)..Pixels::MAX)
                                            .child(
                                                v_flex()
                                                    .size_full()
                                                    .bg(cx.theme().background)
                                                    .border_t_1()
                                                    .border_color(cx.theme().border)
                                                    .child(
                                                        div()
                                                            .px_3()
                                                            .py_1()
                                                            .text_xs()
                                                            .font_semibold()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child("TERMINAL"),
                                                    )
                                                    .child(
                                                        div()
                                                            .px_3()
                                                            .text_sm()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child("Terminal (herdr) menyusul di tahap berikutnya."),
                                                    ),
                                            ),
                                    ),
                            ),
                        ),
                ),
            )
            .child(self.render_status_bar(cx))
    }
}
