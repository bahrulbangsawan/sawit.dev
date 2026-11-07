//! The pool's Analytics view: forecast, when work happens, plan fit, windows, moving work.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::pool::{self, FitVerdict, LimitKind, PoolAccount, Reading};
use sawit_core::date;

use super::money;
use crate::accounts::page::AccountsPage;
use crate::accounts::ui;

impl AccountsPage {
    pub(super) fn render_analytics(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (account, sort) = (self.pool_view.account.clone(), self.pool_view.sort);
        let m = self.model.read(cx);
        let days = m.pool.days.max(30);
        let data = m.pool.data.as_ref().expect("checked");
        let now = sawit_core::github::now() as f64;
        let muted = cx.theme().muted_foreground;
        let chosen: Vec<&PoolAccount> = data
            .accounts
            .iter()
            .filter(|a| account.as_ref().is_none_or(|id| &a.id == id))
            .collect();
        let all_readings: Vec<Reading> = chosen
            .iter()
            .flat_map(|a| data.readings.get(&a.id).cloned().unwrap_or_default())
            .collect();
        let controls = h_flex()
            .flex_wrap()
            .gap_3()
            .child(
                ButtonGroup::new("pool-days")
                    .outline()
                    .compact()
                    .small()
                    .child(Button::new("pool-30").label("30 Days").selected(days == 30))
                    .child(Button::new("pool-90").label("90 Days").selected(days == 90))
                    .on_click(cx.listener(|this, picked: &Vec<usize>, _, cx| {
                        let d = if picked.first() == Some(&1) { 90 } else { 30 };
                        this.model.update(cx, |m, cx| m.set_pool_days(d, cx));
                    })),
            )
            .child(
                Button::new("pool-analytics-account")
                    .small()
                    .outline()
                    .icon(Lucide::Users)
                    .label(
                        account
                            .as_ref()
                            .and_then(|id| data.accounts.iter().find(|a| &a.id == id))
                            .map(|a| a.name.clone())
                            .unwrap_or_else(|| "All Accounts".into()),
                    )
                    .dropdown_caret(true)
                    .dropdown_menu({
                        let page = cx.entity();
                        let names: Vec<(String, String)> = data
                            .accounts
                            .iter()
                            .map(|a| (a.id.clone(), a.name.clone()))
                            .collect();
                        let current = account.clone();
                        move |menu, _, _| {
                            let set = |id: Option<String>| {
                                let page = page.clone();
                                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                    page.update(cx, |this, cx| {
                                        this.pool_view.account = id.clone();
                                        cx.notify();
                                    })
                                }
                            };
                            let menu = menu.item(
                                PopupMenuItem::new("All Accounts")
                                    .icon(crate::menu_icons::ACCOUNT)
                                    .checked(current.is_none())
                                    .on_click(set(None)),
                            );
                            names.iter().fold(menu, |menu, (id, name)| {
                                menu.item(
                                    PopupMenuItem::new(name.clone())
                                        .icon(crate::menu_icons::ACCOUNT)
                                        .checked(current.as_ref() == Some(id))
                                        .on_click(set(Some(id.clone()))),
                                )
                            })
                        }
                    }),
            );
        // Forecast: the account whose current window is heading over the limit soonest.
        let forecast = chosen
            .iter()
            .flat_map(|a| {
                a.current(now)
                    .filter_map(|l| pool::forecast(l, now))
                    .map(move |f| (a, f))
            })
            .max_by(|(_, x), (_, y)| x.pace.total_cmp(&y.pace));
        let forecast_tile = match forecast {
            Some((a, f)) => match f.runs_out_at {
                Some(t) => (
                    format!("Forecast · {}", a.name),
                    format!("Runs out {}", date::when_label(t as i64, now as i64)),
                    format!(
                        "{:.1}× an even pace in its {} window",
                        f.pace,
                        f.kind.label().to_lowercase()
                    ),
                ),
                None => (
                    format!("Forecast · {}", a.name),
                    format!("{:.0}% at the reset", f.projected_pct.min(100.0)),
                    format!("{:.1}× an even pace: room to spare", f.pace),
                ),
            },
            None => (
                "Forecast".into(),
                "—".into(),
                "No window has run long enough to judge".into(),
            ),
        };
        let heat = pool::heatmap(&all_readings, LimitKind::FiveHour, &|t| {
            date::local_offset(t as i64)
        });
        let busiest = heat.busiest();
        let hits: usize = chosen
            .iter()
            .map(|a| {
                let r = data
                    .readings
                    .get(&a.id)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                LimitKind::ALL
                    .iter()
                    .map(|k| pool::windows(r, *k).iter().filter(|w| w.hit_limit).count())
                    .sum::<usize>()
            })
            .sum();
        let tile = |id: &'static str, label: String, value: String, note: String, cx: &App| {
            v_flex()
                .id(id)
                .role(Role::Label)
                .aria_label(format!("{label}: {value}. {note}"))
                .flex_1()
                .min_w(px(200.))
                .px_4()
                .py_3()
                .rounded(cx.theme().radius_lg)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().sidebar)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label),
                )
                .child(div().text_lg().font_medium().child(value))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(note),
                )
        };
        let tiles = h_flex()
            .flex_wrap()
            .gap_3()
            .child(tile(
                "an-forecast",
                forecast_tile.0,
                forecast_tile.1,
                forecast_tile.2,
                cx,
            ))
            .child(tile(
                "an-busiest",
                "Busiest hour".into(),
                busiest
                    .map(|(d, h, _)| format!("{} {:02}:00", date::WEEKDAYS[d], h))
                    .unwrap_or_else(|| "—".into()),
                "From the 5-hour use in the history, local time".into(),
                cx,
            ))
            .child(tile(
                "an-hits",
                "Hit the limit".into(),
                match hits {
                    1 => "Once".into(),
                    n => format!("{n} times"),
                },
                format!("In the last {days} days"),
                cx,
            ));
        // Weekday by hour: darker is more use; a dot marks hours spent at the limit.
        let max = heat.max().max(1e-9);
        let primary = cx.theme().primary;
        let grid = v_flex()
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
                        let v = heat.used[d][h] / max;
                        div()
                            .flex_1()
                            .h(px(14.))
                            .rounded(px(2.))
                            .bg(primary.opacity(0.06 + 0.8 * v as f32))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(heat.locked[d][h] > 0, |el| {
                                el.child(div().size(px(4.)).rounded_full().bg(cx.theme().danger))
                            })
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
            );
        // Plan fit, sortable by its columns.
        let mut fits: Vec<(String, pool::PlanFit)> = chosen
            .iter()
            .map(|a| {
                let r = data
                    .readings
                    .get(&a.id)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                (
                    a.name.clone(),
                    pool::plan_fit(
                        &pool::windows(r, LimitKind::Weekly),
                        data.wallet.monthly(&a.id),
                    ),
                )
            })
            .collect();
        let (col, desc) = sort;
        fits.sort_by(|(an, a), (bn, b)| {
            let o = match col {
                1 => a.avg_peak_pct.total_cmp(&b.avg_peak_pct),
                2 => a.hits.cmp(&b.hits),
                3 => a.verdict.label().cmp(b.verdict.label()),
                4 => a
                    .unused_per_month
                    .unwrap_or(-1.0)
                    .total_cmp(&b.unused_per_month.unwrap_or(-1.0)),
                _ => an.cmp(bn),
            };
            if desc { o.reverse() } else { o }
        });
        const COLS: [&str; 5] = [
            "Account",
            "Avg weekly peak",
            "Hits",
            "Verdict",
            "Unused / month",
        ];
        let header = h_flex()
            .px_4()
            .py_1p5()
            .gap_3()
            .children(COLS.iter().enumerate().map(|(i, name)| {
                let arrow = if i == col {
                    if desc { " ↓" } else { " ↑" }
                } else {
                    ""
                };
                Button::new(SharedString::from(format!("fit-sort-{i}")))
                    .xsmall()
                    .ghost()
                    .label(format!("{name}{arrow}"))
                    .accessibility_label(format!("Sort by {name}"))
                    .map(|b| if i == 0 { b.w(px(140.)) } else { b })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let (c, d) = this.pool_view.sort;
                        this.pool_view.sort = (i, if c == i { !d } else { i != 0 });
                        cx.notify();
                    }))
            }));
        let currency = data.wallet.currency.clone();
        let mut fit_rows = vec![header.into_any_element()];
        for (name, f) in &fits {
            fit_rows.push(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_3()
                    .text_sm()
                    .child(div().w(px(148.)).truncate().child(name.clone()))
                    .child(div().w(px(120.)).text_right().child(if f.windows == 0 {
                        "—".into()
                    } else {
                        format!("{:.0}%", f.avg_peak_pct)
                    }))
                    .child(div().w(px(56.)).text_right().child(f.hits.to_string()))
                    .child(
                        div()
                            .flex_1()
                            .pl_4()
                            .text_color(if f.verdict == FitVerdict::TooSmall {
                                cx.theme().warning
                            } else {
                                cx.theme().foreground
                            })
                            .child(f.verdict.label()),
                    )
                    .child(
                        div().w(px(110.)).text_right().text_color(muted).child(
                            f.unused_per_month
                                .map(|v| money(v, &currency))
                                .unwrap_or_else(|| "Price not set".into()),
                        ),
                    )
                    .into_any_element(),
            );
        }
        // Windows: one cell per window, as wide as its share of the period, filled by its peak.
        let period_start = now - days as f64 * 86400.0;
        let window_rows: Vec<AnyElement> = chosen
            .iter()
            .map(|a| {
                let r = data
                    .readings
                    .get(&a.id)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let spans = pool::windows(r, LimitKind::Weekly);
                let avg = if spans.is_empty() {
                    None
                } else {
                    Some(spans.iter().map(|w| w.peak_pct).sum::<f64>() / spans.len() as f64)
                };
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_3()
                    .child(div().w(px(120.)).text_sm().truncate().child(a.name.clone()))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .h(px(14.))
                            .rounded(px(3.))
                            .bg(cx.theme().accent)
                            .children(spans.iter().filter(|w| w.end > period_start).map(|w| {
                                let l = ((w.start.max(period_start) - period_start)
                                    / (now - period_start))
                                    .clamp(0.0, 1.0) as f32;
                                let r = ((w.end.min(now) - period_start) / (now - period_start))
                                    .clamp(0.0, 1.0) as f32;
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(relative(l))
                                    .w(relative((r - l).max(0.003)))
                                    .rounded(px(3.))
                                    .border_1()
                                    .border_color(cx.theme().background)
                                    .bg(ui::meter_color(w.peak_pct, cx).opacity(
                                        0.25 + 0.75 * (w.peak_pct / 100.0).min(1.0) as f32,
                                    ))
                            })),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .text_xs()
                            .text_color(muted)
                            .text_right()
                            .child(match avg {
                                Some(v) if spans.len() == 1 => format!("1 window · peak {v:.0}%"),
                                Some(v) => format!("{} windows · avg peak {v:.0}%", spans.len()),
                                None => "No weekly window yet".into(),
                            }),
                    )
                    .into_any_element()
            })
            .collect();
        let moves: Vec<AnyElement> = pool::move_work(&data.accounts, now)
            .into_iter()
            .map(|mv| {
                let name = |id: &str| {
                    data.accounts
                        .iter()
                        .find(|a| a.id == id)
                        .map(|a| a.name.clone())
                        .unwrap_or_default()
                };
                h_flex()
                    .px_4()
                    .py_2p5()
                    .gap_2()
                    .items_start()
                    .child(Icon::new(Lucide::GitCompare).small().text_color(muted))
                    .child(
                        v_flex()
                            .flex_1()
                            .child(div().text_sm().child(format!(
                                "{} → {}",
                                name(&mv.from),
                                name(&mv.to)
                            )))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .whitespace_normal()
                                    .child(mv.why),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        vec![
            controls.into_any_element(),
            tiles.into_any_element(),
            v_flex()
                .child(ui::section_header(Lucide::Flame, "When work happens", cx).child(div().text_xs().text_color(muted).child("5-hour use by weekday and hour, local time; a red dot marks hours at the limit")))
                .child(ui::group_box(cx).p_4().child(grid))
                .into_any_element(),
            v_flex()
                .child(ui::section_header(Lucide::ChartColumn, "Weekly windows", cx))
                .child(ui::group_box(cx).children(ui::rows(window_rows, cx)))
                .into_any_element(),
            h_flex()
                .flex_wrap()
                .items_start()
                .gap_4()
                .child(v_flex().flex_1().min_w(px(420.)).child(ui::section_header(Lucide::Gauge, "Plan fit", cx)).child(ui::group_box(cx).children(ui::rows(fit_rows, cx))))
                .child(
                    v_flex()
                        .w(px(340.))
                        .child(ui::section_header(Lucide::TrendingUp, "Move work", cx))
                        .child(if moves.is_empty() {
                            ui::group_box(cx).child(div().px_4().py_3().text_sm().text_color(muted).child("Nothing to move: no account is running out.")).into_any_element()
                        } else {
                            ui::group_box(cx).children(ui::rows(moves, cx)).into_any_element()
                        }),
                )
                .into_any_element(),
        ]
    }
}
