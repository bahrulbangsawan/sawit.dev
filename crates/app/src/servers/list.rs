//! Sidebar list of PostgreSQL instances, Zed project-panel style.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::pgctl::PgCtlStatus;

use super::store::{ServerStore, Status};
use crate::states::Placeholder;

pub struct ServerList {
    store: Entity<ServerStore>,
}

impl ServerList {
    pub fn new(store: Entity<ServerStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
    }
}

pub fn status_color(status: &Status, cx: &App) -> Hsla {
    match status {
        Status::Known(PgCtlStatus::Running { .. }) => cx.theme().success,
        Status::Known(PgCtlStatus::Stopped) => cx.theme().muted_foreground,
        Status::Unknown => cx.theme().border,
        _ => cx.theme().danger,
    }
}

impl Render for ServerList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let clashes = store.registry.port_clashes();
        let rows: Vec<_> = store
            .registry
            .instances
            .iter()
            .map(|inst| {
                let selected =
                    store.selected.as_deref() == Some(inst.name.as_str()) && !store.creating;
                let name = inst.name.clone();
                let color = status_color(store.status(&inst.name), cx);
                let clash = clashes
                    .iter()
                    .any(|(a, b, _)| *a == inst.name || *b == inst.name);
                h_flex()
                    .id(SharedString::from(format!("inst-{}", inst.name)))
                    .w_full()
                    .h(px(26.))
                    .px_2()
                    .gap_2()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .when(selected, |el| el.bg(cx.theme().list_active))
                    .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
                    .child(div().size(px(7.)).rounded_full().bg(color))
                    .child(div().flex_1().truncate().text_sm().child(inst.name.clone()))
                    .when(clash, |el| {
                        el.child(
                            Icon::new(IconName::TriangleAlert)
                                .small()
                                .text_color(cx.theme().warning),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(":{}", inst.port)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.store.update(cx, |s, cx| s.select(&name, cx));
                    }))
            })
            .collect();
        let empty = rows.is_empty();

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(Lucide::Server).xsmall())
                            .child("INSTANCES"),
                    )
                    .child(
                        Button::new("new-instance")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New Instance")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.update(cx, |s, cx| s.show_create(cx));
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("server-rows")
                    .flex_1()
                    .p_1()
                    .gap_px()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |el| {
                        el.child(
                            Placeholder::new(Lucide::Server, "No instances yet")
                                .compact()
                                .detail("Create a local PostgreSQL server, or add one you already run.")
                                .action(
                                    Button::new("empty-new-instance")
                                        .small()
                                        .primary()
                                        .icon(IconName::Plus)
                                        .label("New Instance")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.store.update(cx, |s, cx| s.show_create(cx));
                                        })),
                                ),
                        )
                    }),
            )
    }
}
