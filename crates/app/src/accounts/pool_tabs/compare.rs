//! The pool's Compare view: the pick and why, the rest and what holds each back.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::pool::{self, PoolAccount};
use sawit_core::date;

use super::status_look;
use crate::accounts::page::AccountsPage;
use crate::accounts::ui;

impl AccountsPage {
    pub(super) fn render_compare(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let pick = self.pick_card(true, cx);
        let m = self.model.read(cx);
        let data = m.pool.data.as_ref().expect("checked");
        let now = sawit_core::github::now() as f64;
        let muted = cx.theme().muted_foreground;
        let mut ranked: Vec<&PoolAccount> = data.accounts.iter().collect();
        ranked.sort_by(|a, b| {
            b.room(now)
                .unwrap_or(-1.0)
                .total_cmp(&a.room(now).unwrap_or(-1.0))
        });
        let head = h_flex()
            .px_4()
            .py_2()
            .gap_3()
            .text_xs()
            .font_semibold()
            .text_color(muted)
            .child(div().w(px(160.)).child("Account"))
            .child(div().w(px(180.)).child("Room left"))
            .child(div().w(px(110.)).child("Held back by"))
            .child(div().flex_1().text_right().child("Resets"))
            .child(div().w(px(140.)).child("Status"));
        let mut rows = vec![head.into_any_element()];
        for a in &ranked {
            let st = pool::status(a, now);
            let (tone, icon) = status_look(st);
            let room = a.room(now);
            let held = a.binding(now);
            rows.push(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_3()
                    .text_sm()
                    .child(
                        h_flex()
                            .w(px(160.))
                            .gap_2()
                            .child(ui::avatar(
                                ui::provider_agent(a.provider),
                                &a.color,
                                20.,
                                cx,
                            ))
                            .child(div().truncate().child(a.name.clone())),
                    )
                    .child(
                        h_flex()
                            .w(px(180.))
                            .gap_2()
                            .child(ui::chart_tip(
                                div().w(px(120.)).py_1().child(ui::track(
                                    (room.unwrap_or(0.0) / 100.0) as f32,
                                    ui::meter_color(100.0 - room.unwrap_or(100.0), cx),
                                    6.,
                                    cx,
                                )),
                                format!("compare-room-{}", a.id),
                                match (room, held) {
                                    (Some(r), Some(l)) => format!(
                                        "{r:.0}% room · the {} limit holds it",
                                        l.kind.label()
                                    ),
                                    (Some(r), None) => format!("{r:.0}% room"),
                                    _ => "No limits reported yet".to_string(),
                                },
                                cx,
                            ))
                            .child(
                                div().text_xs().child(
                                    room.map(|r| format!("{r:.0}%"))
                                        .unwrap_or_else(|| "—".into()),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .w(px(110.))
                            .text_color(muted)
                            .child(held.map(|l| l.kind.label()).unwrap_or("—")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .text_xs()
                            .text_color(muted)
                            .child(
                                held.map(|l| date::when_label(l.resets_at as i64, now as i64))
                                    .unwrap_or_default(),
                            ),
                    )
                    .child(div().w(px(140.)).child(ui::pill(
                        SharedString::from(format!("cmp-{}", a.id)),
                        tone,
                        Some(icon),
                        st.label(),
                        cx,
                    )))
                    .into_any_element(),
            );
        }
        let unreported: Vec<&str> = data
            .accounts
            .iter()
            .filter(|a| a.limits.is_empty())
            .map(|a| a.name.as_str())
            .collect();
        vec![
            pick,
            v_flex()
                .child(ui::section_header(Lucide::GitCompare, "Every account", cx))
                .child(ui::group_box(cx).children(ui::rows(rows, cx)))
                .into_any_element(),
            v_flex()
                .gap_1()
                .text_xs()
                .text_color(muted)
                .child(h_flex().gap_1p5().child(Icon::new(Lucide::Info).xsmall()).child("Room is a share of each plan: 30% of a large plan does more work than 30% of a small one."))
                .when(!unreported.is_empty(), |el| {
                    el.child(h_flex().gap_1p5().child(Icon::new(Lucide::Info).xsmall()).child(format!("Not reported yet: {}. Credit balances and accounts with no limit aren't reported by these agents.", unreported.join(", "))))
                })
                .into_any_element(),
        ]
    }
}
