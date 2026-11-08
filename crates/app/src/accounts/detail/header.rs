//! The detail's header: the avatar, the name renamed in place, `cpN`, plan, who it is, Active
//! or Set Active, Open in Terminal, and ⋯.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::Account;

use super::DetailHead;
use crate::accounts::list::account_menu;
use crate::accounts::page::AccountsPage;
use crate::accounts::ui;

impl AccountsPage {
    pub(super) fn render_detail_header(
        &self,
        a: &Account,
        head: DetailHead,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let DetailHead {
            ix,
            count,
            is_active,
            plan,
            who,
        } = head;
        let name = if self.renaming {
            let id = a.id.clone();
            let save = cx.listener(move |this, _: &ClickEvent, _, cx| {
                let name = this.rename.read(cx).value().to_string();
                this.model.update(cx, |m, cx| m.rename(&id, &name, cx));
                this.renaming = false;
            });
            let _ = window;
            h_flex()
                .gap_1()
                .child(
                    div()
                        .w(px(200.))
                        .child(Input::new(&self.rename).small().aria_label("New Name")),
                )
                .child(
                    Button::new("account-rename-save")
                        .small()
                        .primary()
                        .icon(Lucide::Check)
                        .label("Rename")
                        .on_click(save),
                )
                .child(
                    Button::new("account-rename-cancel")
                        .small()
                        .ghost()
                        .icon(Lucide::X)
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.renaming = false;
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .min_w_0()
                        .truncate()
                        .child(a.name.clone()),
                )
                .child(ui::badge(format!("cp{}", ix + 1), cx))
                .when_some(plan, |el, p| {
                    el.child(ui::pill("account-plan", ui::Tone::Neutral, None, p, cx))
                })
                .into_any_element()
        };
        let page = cx.entity();
        let menu_account = a.clone();
        let id = a.id.clone();
        let terminal_account = a.clone();
        h_flex()
            .flex_wrap()
            .gap_3()
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(220.))
                    .gap_3()
                    .child(ui::avatar(ui::account_agent(a.kind), &a.color, 40., cx))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(name)
                            .when(!who.is_empty(), |el| {
                                let tip = who.clone();
                                el.child(
                                    div()
                                        .id("account-who")
                                        .text_sm()
                                        .text_color(muted)
                                        .truncate()
                                        .child(who)
                                        .tooltip(move |window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                                .build(window, cx)
                                        }),
                                )
                            }),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(if is_active {
                        ui::pill(
                            "account-active",
                            ui::Tone::Good,
                            Some(Lucide::Check),
                            "Active",
                            cx,
                        )
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "New Claude Code sessions use this account",
                            )
                            .build(window, cx)
                        })
                        .into_any_element()
                    } else {
                        Button::new("account-set-active")
                            .small()
                            .outline()
                            .icon(Lucide::Check)
                            .label("Set Active")
                            .tooltip("New Claude Code sessions use this account")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.update(cx, |m, cx| m.set_active(&id, cx))
                            }))
                            .into_any_element()
                    })
                    .child(
                        Button::new("account-open-terminal")
                            .small()
                            .primary()
                            .icon(Lucide::SquareTerminal)
                            .label("Open in Terminal")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_terminal(&terminal_account, cx)
                            })),
                    )
                    .child(
                        Button::new("account-more")
                            .small()
                            .ghost()
                            .icon(Lucide::Ellipsis)
                            .accessibility_label(format!("More for {}", a.name))
                            .tooltip("More")
                            .dropdown_menu(move |menu, _, _| {
                                account_menu(menu, &page, &menu_account, ix, count, is_active)
                            }),
                    ),
            )
    }
}
