//! The Wallet's forms: the price editor, the Add Top-Up form, and Delete Top-Up's question.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::agent_accounts::wallet::{self, Billing};

use crate::accounts::page::AccountsPage;
use crate::accounts::ui;

impl AccountsPage {
    /// The open price editor of `account`: billing, amount, currency and renewal day.
    pub(super) fn price_editor(
        &self,
        account: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
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
    pub(super) fn topup_form(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
                            Button::new("w-topup-account").xsmall().outline().icon(Lucide::Users).label(label).dropdown_caret(true).dropdown_menu(move |menu, _, _| {
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
                    .child(Button::new("w-topup-cancel").xsmall().ghost().icon(Lucide::X).label("Cancel").on_click(cx.listener(|this, _, _, cx| {
                        this.pool_view.adding_topup = false;
                        cx.notify();
                    }))),
            )
    }

    pub(super) fn confirm_delete_topup(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
}
