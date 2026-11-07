//! The pool's Overview (the pick, every account, the agents in Sawit, notifications) and
//! Detailed (grouped by agent or by urgency, filtered, limits as used or left) views.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::pool::{self, PoolAccount, Provider, Status};
use sawit_core::date;

use super::{Grouping, pool_row, provider_logo, status_look};
use crate::accounts::page::AccountsPage;
use crate::accounts::ui::{self, Tone};

impl AccountsPage {
    pub(super) fn render_overview(
        &mut self,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let pick = self.pick_card(false, cx);
        let refresh = self.refresh_button(cx);
        let agents = self.seen_agents(cx);
        let m = self.model.read(cx);
        let data = m.pool.data.as_ref().expect("checked");
        let now = sawit_core::github::now() as f64;
        let muted = cx.theme().muted_foreground;
        let rows: Vec<AnyElement> = data
            .accounts
            .iter()
            .map(|a| pool_row(a, false, narrow, now, cx))
            .collect();
        let unread = data.notices.iter().filter(|n| !n.read).count();
        let notices: Vec<AnyElement> = data
            .notices
            .iter()
            .take(8)
            .map(|n| {
                let st = [
                    Status::PlentyLeft,
                    Status::RunningLow,
                    Status::AlmostOut,
                    Status::LimitReached,
                ]
                .into_iter()
                .find(|s| s.id() == n.status)
                .unwrap_or(Status::NotReported);
                let (tone, icon) = status_look(st);
                let color = match tone {
                    Tone::Good => cx.theme().success,
                    Tone::Warn => cx.theme().warning,
                    Tone::Bad => cx.theme().danger,
                    Tone::Neutral => muted,
                };
                h_flex()
                    .px_4()
                    .py_2p5()
                    .gap_2()
                    .when(n.read, |el| el.opacity(0.7))
                    .child(Icon::new(icon).small().text_color(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .child(n.text.clone()),
                    )
                    .when(!n.read, |el| el.child(ui::badge("new", cx)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(date::when_label(n.t as i64, now as i64)),
                    )
                    .into_any_element()
            })
            .collect();
        let accounts = v_flex()
            .child(
                ui::section_header(Lucide::LayoutDashboard, "Accounts", cx)
                    .child(div().text_xs().text_color(muted).child({
                        let agents = data.accounts.iter().map(|a| a.provider).collect::<std::collections::BTreeSet<_>>().len();
                        let plural = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
                        format!("{} · {}", plural(data.accounts.len(), "account", "accounts"), plural(agents, "agent", "agents"))
                    }))
                    .child(div().flex_1())
                    .child(refresh),
            )
            .child(if rows.is_empty() {
                ui::group_box(cx)
                    .child(ui::setting_row(Lucide::Users, "No accounts yet", "Add a Claude Code account on the Accounts tab; Codex folders (~/.codex) show up by themselves.", div(), false, cx))
                    .into_any_element()
            } else {
                ui::group_box(cx).id("pool-accounts").role(Role::List).aria_label("Pool Accounts").children(ui::rows(rows, cx)).into_any_element()
            });
        let agents_box = v_flex()
            .flex_1()
            .min_w(px(320.))
            .child(ui::section_header(Lucide::Bot, "Agents in Sawit", cx).child(div().text_xs().text_color(muted).child(agents.len().to_string())))
            .child(if agents.is_empty() {
                ui::group_box(cx)
                    .child(ui::setting_row(Lucide::Bot, "No agents at work", "Agents running in Sawit's terminals, and agents using Sawit through MCP, show here with their account.", div(), false, cx))
                    .into_any_element()
            } else {
                let items: Vec<AnyElement> = agents.iter().enumerate().map(|(ix, a)| a.row(ix, window, cx)).collect();
                ui::group_box(cx).children(ui::rows(items, cx)).into_any_element()
            });
        let notices_box = v_flex()
            .w(px(380.))
            .flex_none()
            .child(
                ui::section_header(Lucide::Bell, "Notifications", cx)
                    .when(unread > 0, |el| {
                        el.child(ui::badge(format!("{unread} new"), cx))
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("pool-mark-read")
                            .xsmall()
                            .ghost()
                            .icon(Lucide::Check)
                            .label("Mark All as Read")
                            .disabled(unread == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model.update(cx, |m, cx| m.mark_notices_read(cx))
                            })),
                    ),
            )
            .child(if notices.is_empty() {
                ui::group_box(cx)
                    .child(ui::setting_row(
                        Lucide::Bell,
                        "Nothing yet",
                        "An account running low, running out or getting room back shows here.",
                        div(),
                        false,
                        cx,
                    ))
                    .into_any_element()
            } else {
                ui::group_box(cx)
                    .children(ui::rows(notices, cx))
                    .into_any_element()
            });
        vec![
            pick,
            accounts.into_any_element(),
            h_flex()
                .flex_wrap()
                .items_start()
                .gap_4()
                .child(agents_box)
                .child(notices_box)
                .into_any_element(),
        ]
    }

    pub(super) fn render_detailed(
        &mut self,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let view = &self.pool_view;
        let (grouping, filter, show_left) = (view.grouping, view.provider, view.show_left);
        let mut order = view.order.clone();
        for p in Provider::ALL {
            if !order.contains(p) {
                order.push(*p);
            }
        }
        let m = self.model.read(cx);
        let data = m.pool.data.as_ref().expect("checked");
        let now = sawit_core::github::now() as f64;
        let muted = cx.theme().muted_foreground;
        let shown: Vec<PoolAccount> = data
            .accounts
            .iter()
            .filter(|a| filter.is_none_or(|p| a.provider == p))
            .cloned()
            .collect();
        let controls = h_flex()
            .flex_wrap()
            .gap_3()
            .child(
                ButtonGroup::new("pool-grouping")
                    .outline()
                    .compact()
                    .small()
                    .child(
                        Button::new("pool-by-provider")
                            .label("By Provider")
                            .selected(grouping == Grouping::Provider),
                    )
                    .child(
                        Button::new("pool-by-urgency")
                            .label("By Urgency")
                            .selected(grouping == Grouping::Urgency),
                    )
                    .on_click(cx.listener(|this, picked: &Vec<usize>, _, cx| {
                        this.pool_view.grouping = if picked.first() == Some(&1) {
                            Grouping::Urgency
                        } else {
                            Grouping::Provider
                        };
                        cx.notify();
                    })),
            )
            .child(
                Button::new("pool-filter")
                    .small()
                    .outline()
                    .icon(Lucide::Funnel)
                    .label(filter.map(|p| p.label()).unwrap_or("All Providers"))
                    .dropdown_caret(true)
                    .dropdown_menu({
                        let page = cx.entity();
                        move |menu, _, _| {
                            let set = |p: Option<Provider>| {
                                let page = page.clone();
                                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                    page.update(cx, |this, cx| {
                                        this.pool_view.provider = p;
                                        cx.notify();
                                    })
                                }
                            };
                            let menu = menu.item(
                                PopupMenuItem::new("All Providers")
                                    .icon(crate::menu_icons::ACCOUNT)
                                    .checked(filter.is_none())
                                    .on_click(set(None)),
                            );
                            Provider::ALL.iter().fold(menu, |menu, p| {
                                menu.item(
                                    PopupMenuItem::new(p.label())
                                        .icon(crate::menu_icons::AGENT)
                                        .checked(filter == Some(*p))
                                        .on_click(set(Some(*p))),
                                )
                            })
                        }
                    }),
            )
            .child(div().flex_1())
            .child(div().text_xs().text_color(muted).child("Show limits as"))
            .child(
                ButtonGroup::new("pool-show-as")
                    .outline()
                    .compact()
                    .small()
                    .child(
                        Button::new("pool-show-used")
                            .label("% Used")
                            .selected(!show_left),
                    )
                    .child(
                        Button::new("pool-show-left")
                            .label("% Left")
                            .selected(show_left),
                    )
                    .on_click(cx.listener(|this, picked: &Vec<usize>, _, cx| {
                        this.pool_view.show_left = picked.first() == Some(&1);
                        cx.notify();
                    })),
            );
        let mut out = vec![controls.into_any_element()];
        if shown.is_empty() {
            out.push(
                ui::empty_state(
                    Lucide::Funnel,
                    "No accounts for this provider",
                    "Pick All Providers to see every account.",
                    cx,
                )
                .into_any_element(),
            );
            return out;
        }
        match grouping {
            Grouping::Urgency => {
                let mut sorted = shown.clone();
                sorted.sort_by(|a, b| {
                    pool::status(a, now)
                        .urgency()
                        .cmp(&pool::status(b, now).urgency())
                        .then(
                            a.room(now)
                                .unwrap_or(101.0)
                                .total_cmp(&b.room(now).unwrap_or(101.0)),
                        )
                });
                let rows: Vec<AnyElement> = sorted
                    .iter()
                    .map(|a| pool_row(a, show_left, narrow, now, cx))
                    .collect();
                out.push(
                    v_flex()
                        .child(ui::section_header(
                            Lucide::TriangleAlert,
                            "Most urgent first",
                            cx,
                        ))
                        .child(ui::group_box(cx).children(ui::rows(rows, cx)))
                        .into_any_element(),
                );
            }
            Grouping::Provider => {
                let present: Vec<Provider> = order
                    .iter()
                    .copied()
                    .filter(|p| shown.iter().any(|a| a.provider == *p))
                    .collect();
                let count = present.len();
                for (gi, p) in present.iter().copied().enumerate() {
                    let rows: Vec<AnyElement> = shown
                        .iter()
                        .filter(|a| a.provider == p)
                        .map(|a| pool_row(a, show_left, narrow, now, cx))
                        .collect();
                    let n = rows.len();
                    let page = cx.entity();
                    out.push(
                        v_flex()
                            .child(
                                ui::section_header(Lucide::Bot, p.label(), cx)
                                    .child(div().text_xs().text_color(muted).child(n.to_string()))
                                    .child(div().flex_1())
                                    .child(provider_logo(p, window, cx))
                                    .child(
                                        Button::new(SharedString::from(format!("pool-order-{}", p.id())))
                                            .xsmall()
                                            .ghost()
                                            .icon(Lucide::ArrowUpDown)
                                            .accessibility_label(format!("Move {}", p.label()))
                                            .tooltip("Change the order")
                                            .dropdown_menu(move |menu, _, _| {
                                                let mv = |delta: isize| {
                                                    let page = page.clone();
                                                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                                        page.update(cx, |this, cx| {
                                                            let mut o: Vec<Provider> = Provider::ALL.to_vec();
                                                            if !this.pool_view.order.is_empty() {
                                                                o = this.pool_view.order.clone();
                                                                o.extend(Provider::ALL.iter().filter(|x| !this.pool_view.order.contains(x)));
                                                            }
                                                            if let Some(i) = o.iter().position(|x| *x == p) {
                                                                let j = (i as isize + delta).clamp(0, o.len() as isize - 1) as usize;
                                                                o.swap(i, j);
                                                            }
                                                            this.pool_view.order = o;
                                                            cx.notify();
                                                        })
                                                    }
                                                };
                                                menu.item(PopupMenuItem::new("Move Up").icon(Lucide::ArrowUp).disabled(gi == 0).on_click(mv(-1)))
                                                    .item(PopupMenuItem::new("Move Down").icon(Lucide::ArrowDown).disabled(gi + 1 >= count).on_click(mv(1)))
                                            }),
                                    ),
                            )
                            .child(ui::group_box(cx).children(ui::rows(rows, cx)))
                            .into_any_element(),
                    );
                }
            }
        }
        out
    }
}
