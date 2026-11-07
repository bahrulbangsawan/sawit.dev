//! The pool's Timeline view: limit windows on a time axis, the watch list, recent resets.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::pool::{self, LimitKind, Reading, Status};
use sawit_core::date;

use super::status_look;
use crate::accounts::page::AccountsPage;
use crate::accounts::ui;

impl AccountsPage {
    pub(super) fn render_timeline(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let week = self.pool_view.week;
        let m = self.model.read(cx);
        let data = m.pool.data.as_ref().expect("checked");
        let now = sawit_core::github::now() as f64;
        let muted = cx.theme().muted_foreground;
        let (from, to) = if week {
            (now - 86400.0, now + 6.0 * 86400.0)
        } else {
            (now - 3.0 * 3600.0, now + 21.0 * 3600.0)
        };
        let x = |t: f64| ((t - from) / (to - from)).clamp(0.0, 1.0) as f32;
        let range = ButtonGroup::new("pool-range")
            .outline()
            .compact()
            .small()
            .child(
                Button::new("pool-next-24")
                    .label("Next 24 Hours")
                    .selected(!week),
            )
            .child(
                Button::new("pool-this-week")
                    .label("This Week")
                    .selected(week),
            )
            .on_click(cx.listener(|this, picked: &Vec<usize>, _, cx| {
                this.pool_view.week = picked.first() == Some(&1);
                cx.notify();
            }));
        let legend = |color: Hsla, dashed: bool, text: &'static str| {
            h_flex()
                .gap_1()
                .child(
                    div()
                        .size(px(10.))
                        .rounded(px(2.))
                        .when(dashed, |el| el.border_1().border_color(muted))
                        .when(!dashed, |el| el.bg(color)),
                )
                .child(text)
        };
        let primary = cx.theme().primary;
        let danger = cx.theme().danger;
        // Tick labels along the axis.
        let ticks: Vec<AnyElement> = (0..=6)
            .map(|i| {
                let t = from + (to - from) * i as f64 / 6.0;
                let l = date::local(t as i64);
                // The last label ends at the axis's end rather than running past it.
                div()
                    .absolute()
                    .map(|el| {
                        if i == 6 {
                            el.right_0()
                        } else {
                            el.left(relative(i as f32 / 6.0))
                        }
                    })
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(muted)
                    .child(if week {
                        date::WEEKDAYS[l.weekday as usize].to_string()
                    } else {
                        format!("{:02}:00", l.hour)
                    })
                    .into_any_element()
            })
            .collect();
        let mut lanes: Vec<AnyElement> = vec![
            h_flex()
                .px_4()
                .pt_2()
                .child(div().w(px(130.)).flex_none())
                .child(div().relative().flex_1().h(px(16.)).children(ticks))
                .into_any_element(),
        ];
        for a in &data.accounts {
            let mut bars: Vec<AnyElement> = Vec::new();
            let readings: &[Reading] = data
                .readings
                .get(&a.id)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            for kind in LimitKind::ALL {
                let spans = pool::windows(readings, *kind);
                let top = if *kind == LimitKind::FiveHour {
                    3.
                } else {
                    15.
                };
                for w in spans.iter().filter(|w| w.end > from && w.start < to) {
                    let (l, r) = (x(w.start), x(w.end));
                    bars.push(
                        div()
                            .absolute()
                            .top(px(top))
                            .h(px(10.))
                            .left(relative(l))
                            .w(relative((r - l).max(0.004)))
                            .rounded(px(3.))
                            .bg(primary.opacity(if w.end > now { 0.55 } else { 0.25 }))
                            .into_any_element(),
                    );
                    if let Some(hit) = w.hit_at {
                        let hl = x(hit);
                        bars.push(
                            div()
                                .absolute()
                                .top(px(top))
                                .h(px(10.))
                                .left(relative(hl))
                                .w(relative((r - hl).max(0.004)))
                                .rounded(px(3.))
                                .bg(danger)
                                .into_any_element(),
                        );
                    }
                }
                // The next window: it opens at the current one's reset.
                if let Some(cur) = a.limit(*kind).filter(|l| l.current(now)) {
                    let (l, r) = (x(cur.resets_at), x(cur.resets_at + kind.window_secs()));
                    if r > l {
                        bars.push(
                            div()
                                .absolute()
                                .top(px(top))
                                .h(px(10.))
                                .left(relative(l))
                                .w(relative(r - l))
                                .rounded(px(3.))
                                .border_1()
                                .border_color(muted)
                                .into_any_element(),
                        );
                    }
                }
            }
            bars.push(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(x(now)))
                    .w(px(2.))
                    .bg(cx.theme().foreground)
                    .into_any_element(),
            );
            lanes.push(
                h_flex()
                    .id(SharedString::from(format!("timeline-{}", a.id)))
                    .role(Role::Label)
                    .aria_label(format!("{}: {}", a.name, pool::status(a, now).label()))
                    .px_4()
                    .py_1p5()
                    .child(
                        h_flex()
                            .w(px(130.))
                            .flex_none()
                            .gap_2()
                            .child(ui::avatar(&a.name, &a.color, 20., cx))
                            .child(div().text_sm().truncate().child(a.name.clone())),
                    )
                    .child(div().relative().flex_1().h(px(28.)).children(bars))
                    .into_any_element(),
            );
        }
        let watch: Vec<AnyElement> = data
            .accounts
            .iter()
            .filter_map(|a| {
                let st = pool::status(a, now);
                if !matches!(
                    st,
                    Status::RunningLow | Status::AlmostOut | Status::LimitReached
                ) {
                    return None;
                }
                let l = a.binding(now)?;
                Some(
                    h_flex()
                        .px_4()
                        .py_2p5()
                        .gap_2()
                        .child(Icon::new(status_look(st).1).small().text_color(
                            if st == Status::RunningLow {
                                cx.theme().warning
                            } else {
                                danger
                            },
                        ))
                        .child(div().flex_1().text_sm().child(format!(
                            "{} · {}",
                            a.name,
                            l.kind.label().to_lowercase()
                        )))
                        .child(div().text_xs().text_color(muted).child(format!(
                            "{} until {}",
                            st.label(),
                            date::when_label(l.resets_at as i64, now as i64)
                        )))
                        .into_any_element(),
                )
            })
            .collect();
        let mut resets: Vec<(f64, AnyElement)> = Vec::new();
        for a in &data.accounts {
            let readings: &[Reading] = data
                .readings
                .get(&a.id)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            for kind in LimitKind::ALL {
                for w in
                    pool::resets_between(&pool::windows(readings, *kind), now - 7.0 * 86400.0, now)
                {
                    resets.push((
                        w.end,
                        h_flex()
                            .px_4()
                            .py_2p5()
                            .gap_2()
                            .child(div().flex_1().text_sm().child(format!(
                                "{} · {}",
                                a.name,
                                kind.label().to_lowercase()
                            )))
                            .child(div().text_xs().text_color(muted).child(format!(
                                "{} · reset {}",
                                if w.hit_limit {
                                    "Hit the limit".to_string()
                                } else {
                                    format!("Peaked at {:.0}%", w.peak_pct)
                                },
                                date::when_label(w.end as i64, now as i64)
                            )))
                            .into_any_element(),
                    ));
                }
            }
        }
        resets.sort_by(|a, b| b.0.total_cmp(&a.0));
        let resets: Vec<AnyElement> = resets.into_iter().take(12).map(|(_, e)| e).collect();
        let list_box = |items: Vec<AnyElement>, empty: &'static str, cx: &App| {
            if items.is_empty() {
                ui::group_box(cx)
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(empty),
                    )
                    .into_any_element()
            } else {
                ui::group_box(cx)
                    .children(ui::rows(items, cx))
                    .into_any_element()
            }
        };
        vec![
            h_flex()
                .flex_wrap()
                .gap_3()
                .child(range)
                .child(div().flex_1())
                .child(
                    h_flex()
                        .gap_3()
                        .text_xs()
                        .text_color(muted)
                        .child(legend(
                            primary.opacity(0.55),
                            false,
                            "Window (5-hour top, weekly below)",
                        ))
                        .child(legend(primary, true, "Next window"))
                        .child(legend(danger, false, "At its limit")),
                )
                .into_any_element(),
            ui::group_box(cx).pb_2().children(lanes).into_any_element(),
            h_flex()
                .flex_wrap()
                .items_start()
                .gap_4()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(300.))
                        .child(ui::section_header(Lucide::TriangleAlert, "Watch list", cx))
                        .child(list_box(watch, "Every account has room.", cx)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(300.))
                        .child(
                            ui::section_header(Lucide::Clock, "Recent resets", cx)
                                .child(div().text_xs().text_color(muted).child("Last 7 days")),
                        )
                        .child(list_box(resets, "No window reset in the history yet.", cx)),
                )
                .into_any_element(),
        ]
    }
}
