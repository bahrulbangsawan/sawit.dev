//! The pool's six views on the Accounts page (design: `docs/design/accounts.sawit`, C1 frames):
//! Overview (the pick, every account, the agents in Sawit, notifications), Detailed (grouped by
//! agent or by urgency, filtered, limits as used or left), Compare (the pick and why, the rest
//! and what holds each back), Timeline (limit windows on a time axis, the watch list, recent
//! resets), Analytics (forecast, when work happens, plan fit, windows, moving work) and Wallet
//! (prices, renewals, top-ups, removed accounts, exchange rates).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::pool::{
    self, FitVerdict, LimitKind, PoolAccount, Provider, Reading, Status,
};
use sawit_core::agent_accounts::wallet::{self, Billing};
use sawit_core::date;

use super::page::{AccountsPage, Tab};
use super::ui::{self, Tone};

/// How the Detailed view groups its rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Grouping {
    #[default]
    Provider,
    Urgency,
}

/// The pool views' own choices, kept while the page is open.
#[derive(Default)]
pub struct PoolView {
    pub grouping: Grouping,
    pub provider: Option<Provider>,
    pub show_left: bool,
    /// The providers' order in Detailed (Move Up and Move Down on a group).
    pub order: Vec<Provider>,
    /// Timeline: the week instead of the next 24 hours.
    pub week: bool,
    /// Analytics: one account, or all.
    pub account: Option<String>,
    /// Plan fit's sort: the column and whether it runs high to low.
    pub sort: (usize, bool),
    /// Wallet: the account whose price is being edited.
    pub editing: Option<String>,
    pub adding_topup: bool,
}

/// The tone, icon and words of a status: never colour alone.
pub fn status_look(s: Status) -> (Tone, Lucide) {
    match s {
        Status::PlentyLeft => (Tone::Good, Lucide::CircleCheck),
        Status::RunningLow => (Tone::Warn, Lucide::TriangleAlert),
        Status::AlmostOut | Status::LimitReached => (Tone::Bad, Lucide::CircleX),
        Status::SignInExpired => (Tone::Neutral, Lucide::LogIn),
        Status::RefreshFailed => (Tone::Bad, Lucide::RefreshCw),
        Status::OutOfDate => (Tone::Neutral, Lucide::Clock),
        Status::NotReported => (Tone::Neutral, Lucide::Info),
    }
}

fn provider_logo(p: Provider, window: &Window, cx: &App) -> AnyElement {
    let kind = match p {
        Provider::ClaudeCode => sawit_core::terminal::AgentKind::ClaudeCode,
        Provider::Codex => sawit_core::terminal::AgentKind::Codex,
    };
    crate::terminal::agents::logo(kind, px(14.), window, cx)
}

/// "in 2h 14m" from seconds.
fn span(secs: f64) -> String {
    let s = secs.max(0.0) as i64;
    match (s / 86400, s % 86400 / 3600, s % 3600 / 60) {
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

fn money(v: f64, currency: &str) -> String {
    // An empty sum is -0.0: it reads as nothing spent.
    let v = if v == 0.0 { 0.0 } else { v };
    let grouped = |whole: i64| {
        let digits = whole.abs().to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        if whole < 0 { format!("-{out}") } else { out }
    };
    let sym = match currency {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        _ => "",
    };
    let cents = ((v.abs() * 100.0).round() as i64) % 100;
    let whole = grouped(v.trunc() as i64);
    if sym.is_empty() {
        format!("{whole}.{cents:02} {currency}")
    } else if cents == 0 {
        format!("{sym}{whole}")
    } else {
        format!("{sym}{whole}.{cents:02}")
    }
}

/// One limit as a cell: "23% used" (or left) with the time to its reset over a bar.
fn limit_cell(a: &PoolAccount, kind: LimitKind, show_left: bool, now: f64, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let cell = v_flex().w(px(160.)).flex_none().gap_1();
    match a.limit(kind).filter(|l| l.current(now)) {
        Some(l) => {
            let value = if show_left {
                format!("{}% left", l.left_pct().round())
            } else {
                format!("{}% used", l.used_pct.round())
            };
            cell.child(
                h_flex()
                    .text_xs()
                    .child(format!("{} {value}", kind.short()))
                    .child(div().flex_1())
                    .child(div().text_color(muted).child(span(l.resets_at - now))),
            )
            .child(ui::track(
                (l.used_pct / 100.0) as f32,
                ui::meter_color(l.used_pct, cx),
                6.,
                cx,
            ))
        }
        None if a.limit(kind).is_some() => cell
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{} · reset since", kind.short())),
            )
            .child(ui::track(0., muted, 6., cx)),
        None => cell
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{} · not reported", kind.short())),
            )
            .child(ui::track(0., muted, 6., cx)),
    }
}

/// A pool row: avatar, name with agent and plan, the two limits, the status.
fn pool_row(a: &PoolAccount, show_left: bool, narrow: bool, now: f64, cx: &App) -> AnyElement {
    let st = pool::status(a, now);
    let (tone, icon) = status_look(st);
    let sub = match &a.plan {
        Some(p) => format!("{} · {p}", a.provider.label()),
        None => a.provider.label().to_string(),
    };
    h_flex()
        .id(SharedString::from(format!("pool-row-{}", a.id)))
        .role(Role::ListItem)
        .aria_label(format!("{}, {}", a.name, st.label()))
        .flex_wrap()
        .gap_3()
        .px_4()
        .py_2p5()
        .child(ui::avatar(&a.name, &a.color, 28., cx))
        .child(
            v_flex()
                .w(px(150.))
                .min_w_0()
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .truncate()
                                .child(a.name.clone()),
                        )
                        .when(a.active, |el| el.child(ui::badge("active", cx))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(sub),
                ),
        )
        .when(!narrow, |el| {
            el.child(limit_cell(a, LimitKind::FiveHour, show_left, now, cx))
                .child(limit_cell(a, LimitKind::Weekly, show_left, now, cx))
        })
        .child(div().flex_1())
        .child(ui::pill(
            SharedString::from(format!("pool-status-{}", a.id)),
            tone,
            Some(icon),
            st.label(),
            cx,
        ))
        .into_any_element()
}

impl AccountsPage {
    /// The pool's data, or the state in its place: loading, an error, or nothing yet.
    fn pool_or_state(&mut self, cx: &mut Context<Self>) -> Result<(), AnyElement> {
        let m = self.model.read(cx);
        if m.pool.data.is_some() {
            return Ok(());
        }
        let el = match &m.pool.error {
            Some(e) => ui::empty_state(Lucide::CircleX, "Unable to read the pool", e.clone(), cx)
                .child(
                    Button::new("pool-retry")
                        .small()
                        .icon(Lucide::RefreshCw)
                        .label("Try Again")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.model.update(cx, |m, cx| m.refresh_pool(cx))
                        })),
                ),
            None => ui::empty_state(
                Lucide::LoaderCircle,
                "Reading the accounts…",
                "Limits, history and prices, from this Mac.",
                cx,
            ),
        };
        Err(div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(el)
            .into_any_element())
    }

    pub(super) fn render_pool(
        &mut self,
        tab: Tab,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Err(state) = self.pool_or_state(cx) {
            return state;
        }
        let body = match tab {
            Tab::Overview => self.render_overview(narrow, window, cx),
            Tab::Detailed => self.render_detailed(narrow, window, cx),
            Tab::Compare => self.render_compare(cx),
            Tab::Timeline => self.render_timeline(cx),
            Tab::Analytics => self.render_analytics(cx),
            _ => self.render_wallet(cx),
        };
        let (scroll, content) = ui::tab_body("accounts-pool", px(1040.));
        scroll.child(content.children(body)).into_any_element()
    }

    /// Refresh, with when the pool was last read.
    fn refresh_button(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let loading = self.model.read(cx).pool.loading;
        Button::new("pool-refresh")
            .xsmall()
            .ghost()
            .icon(Lucide::RefreshCw)
            .label(if loading { "Refreshing…" } else { "Refresh" })
            .loading(loading)
            .on_click(
                cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.refresh_pool(cx))),
            )
    }

    /// "Most room right now": the pick, why, and the next best.
    fn pick_card(&self, big: bool, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let data = m.pool.data.as_ref().expect("checked");
        let now = sawit_core::github::now() as f64;
        let name = |id: &str| data.accounts.iter().find(|a| a.id == id);
        let Some(r) = pool::recommend(&data.accounts, None, now) else {
            return ui::group_box(cx)
                .child(ui::setting_row(
                    Lucide::Gauge,
                    "No pick yet",
                    "No account has reported its limits. Turn on usage tracking on the Accounts tab, or run a session.",
                    div(),
                    false,
                    cx,
                ))
                .into_any_element();
        };
        let best = name(&r.account).cloned();
        let Some(best) = best else {
            return div().into_any_element();
        };
        let mut why = vec![];
        if let Some((id, room)) = &r.next
            && let Some(n) = name(id)
        {
            why.push(format!("Next best: {}, {room:.0}% left", n.name));
        }
        for (n, reason) in &r.skipped {
            why.push(format!("{n}: {reason}"));
        }
        let held = r.holding.and_then(|k| best.limit(k)).map(|l| {
            format!(
                "Held back by its {} limit · resets {}",
                l.kind.label().to_lowercase(),
                date::when_label(l.resets_at as i64, now as i64)
            )
        });
        let terminal_account = m.accounts.get(&best.id).cloned();
        h_flex()
            .id("pool-pick")
            .role(Role::Label)
            .aria_label(format!("Most room right now: {}. {}", best.name, r.reason))
            .gap_4()
            .px_5()
            .py(px(if big { 20. } else { 14. }))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().primary.opacity(0.35))
            .bg(cx.theme().sidebar)
            .child(ui::avatar(&best.name, &best.color, 40., cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if big {
                                "Recommended right now"
                            } else {
                                "Most room right now"
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(if big { 20. } else { 16. }))
                            .font_semibold()
                            .child(format!("{} · {}", best.name, best.provider.label())),
                    )
                    .child(div().text_sm().child(r.reason.clone()))
                    .when_some(held, |el, h| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(h),
                        )
                    })
                    .when(!why.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .whitespace_normal()
                                .child(why.join(" · ")),
                        )
                    }),
            )
            .map(|el| match (big, terminal_account) {
                (true, Some(acc)) => el.child(
                    Button::new("pool-open-pick")
                        .small()
                        .primary()
                        .icon(Lucide::SquareTerminal)
                        .label("Open in Terminal")
                        .on_click(cx.listener(move |this, _, _, cx| this.open_terminal(&acc, cx))),
                ),
                (false, _) => el.child(
                    Button::new("pool-to-compare")
                        .small()
                        .outline()
                        .icon(Lucide::GitCompare)
                        .label("Compare")
                        .on_click(cx.listener(|this, _, _, cx| this.show_tab(Tab::Compare, cx))),
                ),
                _ => el,
            })
            .into_any_element()
    }

    fn render_overview(
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

    fn render_detailed(
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

    fn render_compare(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                            .child(ui::avatar(&a.name, &a.color, 20., cx))
                            .child(div().truncate().child(a.name.clone())),
                    )
                    .child(
                        h_flex()
                            .w(px(180.))
                            .gap_2()
                            .child(div().w(px(120.)).child(ui::track(
                                (room.unwrap_or(0.0) / 100.0) as f32,
                                ui::meter_color(100.0 - room.unwrap_or(100.0), cx),
                                6.,
                                cx,
                            )))
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

    fn render_timeline(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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

    fn render_analytics(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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

    fn render_wallet(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let editing = self.pool_view.editing.clone();
        let adding = self.pool_view.adding_topup;
        // A copy: the rows below take the page's context for their buttons.
        let data = self.model.read(cx).pool.data.clone().expect("checked");
        let data = &data;
        let w = &data.wallet;
        let now = sawit_core::github::now();
        let offset = date::local_offset(now);
        let muted = cx.theme().muted_foreground;
        let cur = w.currency.clone();
        let (per_month, per_year) = w.totals();
        let priced = data
            .accounts
            .iter()
            .filter(|a| w.monthly(&a.id).is_some())
            .count();
        let renewals = w.renewals(now, 30, offset);
        let renew_total: f64 = renewals.iter().map(|r| r.2).sum();
        let months = w.topups_by_month(now, 6, offset);
        let expiring = w.expiring(now);
        let name_of = |id: &str| {
            data.accounts
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| id.to_string())
        };
        let tile =
            |id: &'static str, label: &'static str, value: String, note: String, cx: &App| {
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
                    .child(div().text_2xl().font_medium().child(value))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(note),
                    )
            };
        let tiles = h_flex()
            .flex_wrap()
            .gap_3()
            .child(tile(
                "w-month",
                "Per month",
                money(per_month, &cur),
                format!(
                    "{} a year · {priced} of {} priced",
                    money(per_year, &cur),
                    data.accounts.len()
                ),
                cx,
            ))
            .child(tile(
                "w-renew",
                "Renewing in 30 days",
                money(renew_total, &cur),
                if renewals.is_empty() {
                    "Set a renewal day to see renewals".into()
                } else {
                    renewals
                        .iter()
                        .map(|r| {
                            format!(
                                "{} {}",
                                name_of(&r.0),
                                date::when_label(r.1, now).split(' ').next().unwrap_or("")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                cx,
            ))
            .child(tile(
                "w-topups",
                "Top-ups this month",
                money(months.last().map(|m| m.2).unwrap_or(0.0), &cur),
                match expiring.len() {
                    0 => "None expiring soon".into(),
                    1 => "1 expires soon".into(),
                    n => format!("{n} expire soon"),
                },
                cx,
            ));
        // Subscriptions: one row per account, with its price editor when open.
        let mut subs: Vec<AnyElement> = Vec::new();
        for a in &data.accounts {
            let price = w.prices.get(&a.id).cloned().unwrap_or_default();
            let text = match &price.billing {
                Billing::NotSet => match a.plan.as_deref().and_then(wallet::list_price) {
                    Some(lp) => format!("Price not set · list price {} / month", money(lp, "USD")),
                    None => "Price not set".into(),
                },
                Billing::Monthly { amount } => format!(
                    "{} / month",
                    money(
                        *amount,
                        if price.currency.is_empty() {
                            &cur
                        } else {
                            &price.currency
                        }
                    )
                ),
                Billing::Yearly { amount } => format!(
                    "{} / year",
                    money(
                        *amount,
                        if price.currency.is_empty() {
                            &cur
                        } else {
                            &price.currency
                        }
                    )
                ),
                Billing::Included { with } => format!("Included with {with}"),
            };
            let renew = match (&price.billing, price.renews_day) {
                (Billing::Monthly { .. }, Some(d)) => format!("Renews on day {d}"),
                (Billing::Yearly { .. }, Some(d)) => {
                    format!("Renews {d}/{}", price.renews_month.unwrap_or(1))
                }
                _ => String::new(),
            };
            let id = a.id.clone();
            let open = editing.as_deref() == Some(a.id.as_str());
            subs.push(
                v_flex()
                    .child(
                        h_flex()
                            .px_4()
                            .py_2()
                            .gap_3()
                            .child(ui::avatar(&a.name, &a.color, 20., cx))
                            .child(
                                div()
                                    .w(px(120.))
                                    .text_sm()
                                    .font_semibold()
                                    .truncate()
                                    .child(a.name.clone()),
                            )
                            .child(
                                div()
                                    .w(px(90.))
                                    .text_xs()
                                    .text_color(muted)
                                    .truncate()
                                    .child(a.plan.clone().unwrap_or_default()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .when(matches!(price.billing, Billing::NotSet), |el| {
                                        el.text_color(muted)
                                    })
                                    .child(text),
                            )
                            .child(div().text_xs().text_color(muted).child(renew))
                            .child(
                                Button::new(SharedString::from(format!("w-edit-{}", a.id)))
                                    .xsmall()
                                    .ghost()
                                    .icon(Lucide::Pencil)
                                    .label(if open { "Close" } else { "Edit Price" })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let open =
                                            this.pool_view.editing.as_deref() == Some(id.as_str());
                                        this.pool_view.editing = (!open).then(|| id.clone());
                                        if !open {
                                            let w =
                                                this.model.read(cx).pool.data.as_ref().map(|d| {
                                                    d.wallet
                                                        .prices
                                                        .get(&id)
                                                        .cloned()
                                                        .unwrap_or_default()
                                                });
                                            let (amount, day) = match w {
                                                Some(p) => (
                                                    match p.billing {
                                                        Billing::Monthly { amount }
                                                        | Billing::Yearly { amount } => {
                                                            format!("{amount}")
                                                        }
                                                        _ => String::new(),
                                                    },
                                                    p.renews_day
                                                        .map(|d| d.to_string())
                                                        .unwrap_or_default(),
                                                ),
                                                None => (String::new(), String::new()),
                                            };
                                            this.price_input.update(cx, |s, cx| {
                                                s.set_value(amount, window, cx)
                                            });
                                            this.day_input
                                                .update(cx, |s, cx| s.set_value(day, window, cx));
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(open, |el| el.child(self.price_editor(&a.id, cx)))
                    .into_any_element(),
            );
        }
        let topups: Vec<AnyElement> = w
            .topups
            .iter()
            .rev()
            .map(|t| {
                let soon = expiring.iter().any(|e| e.id == t.id);
                let id = t.id.clone();
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .child(div().flex_1().min_w_0().text_sm().truncate().child(format!(
                        "{} · {}{}{}",
                        name_of(&t.account),
                        money(t.amount, if t.currency.is_empty() { &cur } else { &t.currency }),
                        t.credits.map(|c| format!(" · {c:.0} credits")).unwrap_or_default(),
                        if t.note.is_empty() { String::new() } else { format!(" · {}", t.note) }
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(date::when_label(t.at, now)),
                    )
                    .map(|el| match t.expires_at {
                        Some(e) if soon => el.child(ui::pill(
                            SharedString::from(format!("w-exp-{}", t.id)),
                            Tone::Warn,
                            Some(Lucide::Clock),
                            format!("Expires in {}", span((e - now) as f64)),
                            cx,
                        )),
                        Some(e) if e <= now => {
                            el.child(div().text_xs().text_color(muted).child("Expired"))
                        }
                        Some(e) => el.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Expires {}", date::when_label(e, now))),
                        ),
                        None => el,
                    })
                    .child(
                        Button::new(SharedString::from(format!("w-del-{}", t.id)))
                            .xsmall()
                            .ghost()
                            .icon(Lucide::Trash)
                            .accessibility_label("Delete Top-Up")
                            .tooltip("Delete top-up")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_delete_topup(id.clone(), window, cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let bar_max = months.iter().map(|m| m.2).fold(0.0, f64::max).max(1e-9);
        let chart = h_flex()
            .id("w-months")
            .role(Role::Image)
            .aria_label(format!(
                "Paid top-ups per month: {}",
                months
                    .iter()
                    .map(|(_, mo, v)| format!("{} {}", mo, money(*v, &cur)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .h(px(120.))
            .items_end()
            .gap_3()
            .children(months.iter().map(|(_, mo, v)| {
                const M: [&str; 12] = [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ];
                v_flex()
                    .flex_1()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(muted)
                            .child(if *v > 0.0 {
                                money(*v, &cur)
                            } else {
                                String::new()
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px((80.0 * v / bar_max).max(2.0) as f32))
                            .rounded(px(3.))
                            .bg(cx.theme().primary)
                            .when(*v == 0.0, |el| el.opacity(0.3)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(M[(*mo - 1) as usize]),
                    )
            }));
        let removed: Vec<AnyElement> = w
            .removed
            .iter()
            .map(|r| {
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .text_sm()
                    .child(div().flex_1().child(format!("{} · {}", r.name, r.provider)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("Removed {}", date::when_label(r.removed_at, now))),
                    )
                    .into_any_element()
            })
            .collect();
        let rates = if w.rates.is_empty() {
            format!("Totals in {cur}. Prices in another currency need its rate.")
        } else {
            format!(
                "Totals in {cur} · {}",
                w.rates
                    .iter()
                    .map(|(k, v)| format!("1 {k} = {v} {cur}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
        };
        let mut out = vec![
            tiles.into_any_element(),
            v_flex()
                .child(
                    ui::section_header(Lucide::Wallet, "Subscriptions", cx)
                        .child(div().flex_1())
                        .child(div().text_xs().text_color(muted).child(rates)),
                )
                .child(ui::group_box(cx).children(ui::rows(subs, cx)))
                .into_any_element(),
            h_flex()
                .flex_wrap()
                .items_start()
                .gap_4()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(360.))
                        .child(
                            ui::section_header(Lucide::Plus, "Top-ups", cx)
                                .child(div().flex_1())
                                .child(
                                    Button::new("w-add-topup")
                                        .xsmall()
                                        .outline()
                                        .icon(Lucide::Plus)
                                        .label("Add Top-Up")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.pool_view.adding_topup =
                                                !this.pool_view.adding_topup;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when(adding, |el| el.child(self.topup_form(cx)))
                        .child(if topups.is_empty() {
                            ui::group_box(cx)
                                .child(div().px_4().py_3().text_sm().text_color(muted).child(
                                    "No top-ups yet. Add credits you bought to track their expiry.",
                                ))
                                .into_any_element()
                        } else {
                            ui::group_box(cx)
                                .children(ui::rows(topups, cx))
                                .into_any_element()
                        }),
                )
                .child(
                    v_flex()
                        .w(px(360.))
                        .child(ui::section_header(
                            Lucide::ChartColumn,
                            "Paid top-ups per month",
                            cx,
                        ))
                        .child(ui::group_box(cx).p_4().child(chart)),
                )
                .into_any_element(),
        ];
        if !removed.is_empty() {
            out.push(
                v_flex()
                    .child(ui::section_header(Lucide::Archive, "Removed accounts", cx))
                    .child(ui::group_box(cx).children(ui::rows(removed, cx)))
                    .into_any_element(),
            );
        }
        out
    }

    /// The open price editor of `account`: billing, amount, currency and renewal day.
    fn price_editor(&self, account: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let m = self.model.read(cx);
        let price = m
            .pool
            .data
            .as_ref()
            .and_then(|d| d.wallet.prices.get(account).cloned())
            .unwrap_or_default();
        let others: Vec<String> = m
            .pool
            .data
            .as_ref()
            .map(|d| {
                d.accounts
                    .iter()
                    .filter(|a| a.id != account)
                    .map(|a| a.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let kind = match price.billing {
            Billing::NotSet => 3,
            Billing::Monthly { .. } => 0,
            Billing::Yearly { .. } => 1,
            Billing::Included { .. } => 2,
        };
        let id = account.to_string();
        let save_id = id.clone();
        h_flex()
            .flex_wrap()
            .gap_3()
            .px_4()
            .py_3()
            .bg(cx.theme().background)
            .child(
                ButtonGroup::new("w-billing")
                    .outline()
                    .compact()
                    .xsmall()
                    .child(
                        Button::new("w-monthly")
                            .label("Monthly")
                            .selected(kind == 0),
                    )
                    .child(Button::new("w-yearly").label("Yearly").selected(kind == 1))
                    .child(
                        Button::new("w-included")
                            .label("Included")
                            .selected(kind == 2),
                    )
                    .child(Button::new("w-notset").label("Not Set").selected(kind == 3))
                    .on_click(cx.listener(move |this, picked: &Vec<usize>, _, cx| {
                        let Some(&ix) = picked.first() else { return };
                        let amount: f64 = this
                            .price_input
                            .read(cx)
                            .value()
                            .trim()
                            .parse()
                            .unwrap_or(0.0);
                        let with = others
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "another subscription".into());
                        let id = id.clone();
                        this.model.update(cx, |m, cx| {
                            m.edit_wallet(cx, |w| {
                                let p = w.prices.entry(id).or_default();
                                p.billing = match ix {
                                    0 => Billing::Monthly { amount },
                                    1 => Billing::Yearly { amount },
                                    2 => Billing::Included { with },
                                    _ => Billing::NotSet,
                                };
                            })
                        });
                    })),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().font_medium().child("Amount"))
                    .child(
                        div()
                            .w(px(110.))
                            .child(Input::new(&self.price_input).xsmall().aria_label("Amount")),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().font_medium().child("Renews on day"))
                    .child(
                        div().w(px(80.)).child(
                            Input::new(&self.day_input)
                                .xsmall()
                                .aria_label("Renews on Day"),
                        ),
                    ),
            )
            .child(
                Button::new("w-save-price")
                    .xsmall()
                    .primary()
                    .icon(Lucide::Check)
                    .label("Save")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let amount: Option<f64> = this
                            .price_input
                            .read(cx)
                            .value()
                            .trim()
                            .parse()
                            .ok()
                            .filter(|v: &f64| *v >= 0.0);
                        let day: Option<u32> = this
                            .day_input
                            .read(cx)
                            .value()
                            .trim()
                            .parse()
                            .ok()
                            .filter(|d| (1..=28).contains(d));
                        let id = save_id.clone();
                        this.model.update(cx, |m, cx| {
                            m.edit_wallet(cx, |w| {
                                let p = w.prices.entry(id).or_default();
                                p.billing = match (&p.billing, amount) {
                                    (Billing::Yearly { .. }, Some(a)) => {
                                        Billing::Yearly { amount: a }
                                    }
                                    (Billing::Included { with }, _) => {
                                        Billing::Included { with: with.clone() }
                                    }
                                    (_, Some(a)) => Billing::Monthly { amount: a },
                                    (b, None) => b.clone(),
                                };
                                p.renews_day = day.or(p.renews_day);
                            })
                        });
                        this.pool_view.editing = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Days 1–28"),
            )
    }

    /// The Add Top-Up form: account, amount, credits, days until it expires, note.
    fn topup_form(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let m = self.model.read(cx);
        let names: Vec<(String, String)> = m
            .pool
            .data
            .as_ref()
            .map(|d| {
                d.accounts
                    .iter()
                    .map(|a| (a.id.clone(), a.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let chosen = self
            .topup_account
            .clone()
            .or_else(|| names.first().map(|n| n.0.clone()));
        let label = chosen
            .as_ref()
            .and_then(|id| names.iter().find(|n| &n.0 == id))
            .map(|n| n.1.clone())
            .unwrap_or_else(|| "Account".into());
        let field =
            |name: &'static str, input: &Entity<gpui_kit::component::input::InputState>, w: f32| {
                v_flex()
                    .gap_1()
                    .child(div().text_xs().font_medium().child(name))
                    .child(
                        div()
                            .w(px(w))
                            .child(Input::new(input).xsmall().aria_label(name)),
                    )
            };
        let page = cx.entity();
        ui::group_box(cx)
            .mb_2()
            .p_3()
            .gap_3()
            .child(
                h_flex()
                    .flex_wrap()
                    .items_end()
                    .gap_3()
                    .child(
                        v_flex().gap_1().child(div().text_xs().font_medium().child("Account")).child(
                            Button::new("w-topup-account").xsmall().outline().label(label).dropdown_caret(true).dropdown_menu(move |menu, _, _| {
                                names.iter().fold(menu, |menu, (id, name)| {
                                    let (page, id) = (page.clone(), id.clone());
                                    menu.item(PopupMenuItem::new(name.clone()).icon(crate::menu_icons::ACCOUNT).on_click(move |_, _, cx| {
                                        page.update(cx, |this, cx| {
                                            this.topup_account = Some(id.clone());
                                            cx.notify();
                                        })
                                    }))
                                })
                            }),
                        ),
                    )
                    .child(field("Amount", &self.topup_amount, 90.))
                    .child(field("Credits", &self.topup_credits, 90.))
                    .child(field("Expires in days", &self.topup_days, 90.))
                    .child(field("Note", &self.topup_note, 160.)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("w-topup-save")
                            .xsmall()
                            .primary()
                            .icon(Lucide::Plus)
                            .label("Add Top-Up")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let Some(account) = chosen.clone() else { return };
                                let read = |e: &Entity<gpui_kit::component::input::InputState>, cx: &App| e.read(cx).value().trim().to_string();
                                let Ok(amount) = read(&this.topup_amount, cx).parse::<f64>() else {
                                    this.model.update(cx, |m, cx| {
                                        m.error = Some("Enter the top-up's amount as a number, like 10 or 9.99.".into());
                                        cx.notify();
                                    });
                                    return;
                                };
                                let credits = read(&this.topup_credits, cx).parse::<f64>().ok();
                                let days = read(&this.topup_days, cx).parse::<i64>().ok().filter(|d| *d > 0);
                                let note = read(&this.topup_note, cx);
                                let now = sawit_core::github::now();
                                this.model.update(cx, |m, cx| {
                                    m.edit_wallet(cx, |w| {
                                        w.add_topup(wallet::TopUp {
                                            id: String::new(),
                                            account,
                                            amount,
                                            currency: String::new(),
                                            credits,
                                            at: now,
                                            expires_at: days.map(|d| now + d * 86400),
                                            note,
                                        });
                                    })
                                });
                                for e in [&this.topup_amount, &this.topup_credits, &this.topup_days, &this.topup_note] {
                                    e.update(cx, |s, cx| s.set_value("", window, cx));
                                }
                                this.pool_view.adding_topup = false;
                                cx.notify();
                            })),
                    )
                    .child(Button::new("w-topup-cancel").xsmall().ghost().label("Cancel").on_click(cx.listener(|this, _, _, cx| {
                        this.pool_view.adding_topup = false;
                        cx.notify();
                    }))),
            )
    }

    fn confirm_delete_topup(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let answer = crate::confirm::Confirm::new("Delete this top-up?", "Delete Top-Up")
            .detail("Its amount leaves the monthly totals. Nothing changes with the provider.")
            .ask(window, cx);
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await {
                model.update(cx, |m, cx| {
                    m.edit_wallet(cx, |w| w.topups.retain(|t| t.id != id))
                });
            }
        })
        .detach();
    }

    /// The agents Sawit sees, from the window this page is in.
    fn seen_agents(&self, cx: &App) -> Vec<super::agents_seen::SeenAgent> {
        self.agents_source
            .as_ref()
            .map(|f| f(cx))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{money, span, status_look};
    use sawit_core::agent_accounts::pool::Status;

    #[test]
    fn money_spans_and_status_looks() {
        assert_eq!(money(200.0, "USD"), "$200");
        assert_eq!(money(9.99, "EUR"), "€9.99");
        assert_eq!(money(5.0, "JPY"), "5.00 JPY");
        assert_eq!(money(-0.0, "USD"), "$0");
        assert_eq!(money(2400.0, "USD"), "$2,400");
        assert_eq!(money(1234567.5, "USD"), "$1,234,567.50");
        assert_eq!(span(90.0), "1m");
        assert_eq!(span(2.0 * 3600.0 + 14.0 * 60.0), "2h 14m");
        assert_eq!(span(3.0 * 86400.0 + 4.0 * 3600.0), "3d 4h");
        // Every status has an icon, so none is told by colour alone.
        for s in [
            Status::PlentyLeft,
            Status::RunningLow,
            Status::AlmostOut,
            Status::LimitReached,
            Status::NotReported,
            Status::OutOfDate,
            Status::RefreshFailed,
            Status::SignInExpired,
        ] {
            let _ = status_look(s);
        }
    }
}
