//! The Analytics tab's weekday-by-hour heatmap of 5-hour use: darker is more use, a dot marks the
//! hours spent at the limit, and each cell says its hour, its use and its time at the limit when
//! hovered.

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::pool::Heatmap;
use sawit_core::date;

use crate::accounts::ui;

/// The heatmap of `heat`, whose busiest slot is `busiest` (weekday, hour, points).
pub(super) fn grid(
    heat: &Heatmap,
    busiest: Option<(usize, usize, f64)>,
    cx: &App,
) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let max = heat.max().max(1e-9);
    let primary = cx.theme().primary;
    v_flex()
        .id("an-heatmap")
        .role(Role::Image)
        .aria_label(match busiest {
            Some((d, h, _)) => format!(
                "Use by weekday and hour; busiest {} {:02}:00",
                date::WEEKDAYS[d],
                h
            ),
            None => "Use by weekday and hour: no use in the history yet".into(),
        })
        .gap(px(2.))
        .children((0..7).map(|d| {
            h_flex()
                .gap(px(2.))
                .child(
                    div()
                        .w(px(32.))
                        .text_xs()
                        .text_color(muted)
                        .child(date::WEEKDAYS[d]),
                )
                .children((0..24).map(|h| {
                    let used = heat.used[d][h];
                    let v = used / max;
                    let locked = heat.locked[d][h];
                    let mut said = format!(
                        "{} {h:02}:00–{:02}:00 · {}",
                        date::WEEKDAYS[d],
                        (h + 1) % 24,
                        if used > 0.0 {
                            format!(
                                "{used:.0} points of 5-hour use ({:.0}% of the busiest hour)",
                                v * 100.0
                            )
                        } else {
                            "no use".to_string()
                        }
                    );
                    if locked > 0 {
                        said.push_str(&format!(
                            " · at the limit in {locked} {}",
                            if locked == 1 { "reading" } else { "readings" }
                        ));
                    }
                    let cell = div()
                        .flex_1()
                        .h(px(14.))
                        .rounded(px(2.))
                        .bg(primary.opacity(0.06 + 0.8 * v as f32))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(locked > 0, |el| {
                            el.child(div().size(px(4.)).rounded_full().bg(cx.theme().danger))
                        });
                    ui::chart_tip(cell, format!("an-heat-{d}-{h}"), said, cx)
                }))
        }))
        .child(
            h_flex()
                .gap(px(2.))
                .child(div().w(px(32.)))
                .children((0..24).map(|h| {
                    div()
                        .flex_1()
                        .text_size(px(10.))
                        .text_color(muted)
                        .child(if h % 6 == 0 {
                            format!("{h:02}")
                        } else {
                            String::new()
                        })
                })),
        )
}
