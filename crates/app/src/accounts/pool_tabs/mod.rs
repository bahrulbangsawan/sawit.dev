//! The pool's six views on the Accounts page (design: `docs/design/accounts.sawit`, C1 frames):
//! Overview (the pick, every account, the agents in Sawit, notifications), Detailed (grouped by
//! agent or by urgency, filtered, limits as used or left), Compare (the pick and why, the rest
//! and what holds each back), Timeline (limit windows on a time axis, the watch list, recent
//! resets), Analytics (forecast, when work happens, plan fit, windows, moving work) and Wallet
//! (prices, renewals, top-ups, removed accounts, exchange rates).

mod analytics;
mod compare;
mod overview;
mod timeline;
mod wallet_forms;
mod wallet_view;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::pool::{self, LimitKind, PoolAccount, Provider, Status};
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
