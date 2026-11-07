//! The pool's Wallet view: prices, renewals, top-ups, removed accounts, exchange rates.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::wallet::{self, Billing};
use sawit_core::date;

use super::{money, span};
use crate::accounts::page::AccountsPage;
use crate::accounts::ui::{self, Tone};

impl AccountsPage {
    pub(super) fn render_wallet(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
}
